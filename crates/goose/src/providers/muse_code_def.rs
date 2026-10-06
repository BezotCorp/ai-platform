use crate::config::Config;
use crate::config::paths::Paths;
use crate::providers::base::ProviderDef;
use crate::providers::oauth_device_flow::{
    DeviceFlowConfig, DeviceFlowTokens, RequestEncoding, run_device_flow,
};
use crate::providers::private_file::write_private_file;
use anyhow::Result;
use async_trait::async_trait;
use chrono::{DateTime, Duration, Utc};
use futures::future::BoxFuture;
use bcaip_provider_types::base::{
    ConfigKey, MessageStream, ModelInfo, Provider, ProviderDescriptor, ProviderMetadata,
    model_info_for_provider_model,
};
use bcaip_provider_types::conversations::Message;
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::formats::AnthropicFormatOptions;
use bcaip_provider_types::model::ModelConfig;
use goose_providers::anthropic::{
    ANTHROPIC_API_VERSION, AnthropicProvider, AnthropicProviderBuilder,
};
use goose_providers::api_client::{ApiClient, AuthMethod, AuthProvider, TlsConfig};
use reqwest::Client;
use reqwest::header::{ACCEPT, HeaderMap, HeaderValue, USER_AGENT};
use rmcp::model::Tool;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration as StdDuration;
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex as TokioMutex;
pub const MUSE_CODE_PROVIDER_NAME: &str = "muse_code";
pub const MUSE_CODE_API_HOST: &str = "https://api.meta.ai";
pub const MUSE_CODE_AUTH_HOST: &str = "https://auth.meta.com";
pub const MUSE_CODE_CLIENT_ID: &str = "1031625952748946";
const MUSE_CODE_DOC_URL: &str = "https://ai.developer.meta.com/docs/muse-code/subscriptions";
const MUSE_CODE_USER_AGENT: &str = "goose-muse-code";

const MUSE_CODE_KNOWN_MODELS: &[&str] = &[
    "muse-spark-1.3",
    "muse-spark-1.2",
    "muse-spark-1.1",
    "muse-spark-1.2-contributor",
    "muse-spark-1.3-contributor",
];

const REFRESH_THRESHOLD_SECS: i64 = 300;
/// Minted Model API keys are valid for about a day. The identity token is not
/// renewable: auth.meta.com does not issue a refresh_token and rejects
/// grant_type=refresh_token. Re-mint from the stored identity token instead.
const API_KEY_LIFETIME_SECS: i64 = 24 * 60 * 60;

static GLOBAL_MUSE_REFRESH_MUTEX: std::sync::OnceLock<TokioMutex<()>> = std::sync::OnceLock::new();

fn global_muse_refresh_mutex() -> &'static TokioMutex<()> {
    GLOBAL_MUSE_REFRESH_MUTEX.get_or_init(|| TokioMutex::new(()))
}

static MUSE_TOKEN_GENERATION: AtomicU64 = AtomicU64::new(0);

pub struct MuseCodeProviderDef;

pub struct MuseCodeProvider {
    inner: AnthropicProvider,
    auth: Arc<MuseCodeAuth>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
struct MuseToken {
    access_token: String,
    #[serde(default)]
    refresh_token: String,
    #[serde(default)]
    api_key: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct MintedKey {
    api_key: Option<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct TokenCache {
    path: PathBuf,
}

struct MuseCodeAuth {
    cache: TokenCache,
    client: Client,
    api_host: String,
    auth_host: String,
    client_id: String,
}

struct SharedAuthProvider(Arc<MuseCodeAuth>);

fn known_models() -> Vec<ModelInfo> {
    MUSE_CODE_KNOWN_MODELS
        .iter()
        .map(|&name| model_info_for_provider_model(MUSE_CODE_PROVIDER_NAME, name))
        .collect()
}

fn tokens_to_muse(tokens: DeviceFlowTokens) -> MuseToken {
    MuseToken {
        access_token: tokens.access_token,
        refresh_token: String::new(),
        api_key: String::new(),
        expires_at: Utc::now(),
    }
}

fn minted_key_expiry() -> DateTime<Utc> {
    Utc::now() + Duration::seconds(API_KEY_LIFETIME_SECS)
}

fn mint_error(status: reqwest::StatusCode, body: &str) -> ProviderError {
    let details = format!("key mint failed ({status}): {body}");
    if status == reqwest::StatusCode::UNAUTHORIZED || status == reqwest::StatusCode::FORBIDDEN {
        return ProviderError::Authentication(format!(
            "Meta session expired ({status}). Sign in to Muse Code again. {body}"
        ));
    }
    if status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        return ProviderError::RateLimitExceeded {
            details,
            retry_delay: None,
        };
    }
    if status.is_server_error() {
        return ProviderError::ServerError(details);
    }
    ProviderError::RequestFailed(details)
}

fn join_url(host: &str, path: &str) -> String {
    format!("{}{}", host.trim_end_matches('/'), path)
}

pub(crate) fn has_configured_token() -> bool {
    TokenCache::new().has_token() || muse_cli_token().is_some()
}

fn muse_cli_auth_path() -> PathBuf {
    if let Ok(path) = std::env::var("MUSE_AUTH_PATH") {
        if !path.is_empty() {
            return PathBuf::from(path);
        }
    }
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return PathBuf::from(xdg).join("muse/auth.json");
        }
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_else(|_| ".".to_string()))
        .join(".config/muse/auth.json")
}

fn muse_cli_keychain_token() -> Option<MuseToken> {
    #[cfg(feature = "system-keyring")]
    {
        if std::env::var("MUSE_AUTH_PATH").is_ok() {
            return None;
        }

        #[derive(Deserialize)]
        struct Stored {
            #[serde(default)]
            api_key: String,
            #[serde(default)]
            access_token: String,
        }

        let entry = keyring::Entry::new("ai.meta.dev.credentials", "meta").ok()?;
        let raw = entry.get_password().ok()?;
        let stored: Stored = serde_json::from_str(&raw).ok()?;
        if stored.api_key.is_empty() {
            return None;
        }
        Some(MuseToken {
            access_token: stored.access_token,
            refresh_token: String::new(),
            api_key: stored.api_key,
            expires_at: minted_key_expiry(),
        })
    }

    #[cfg(not(feature = "system-keyring"))]
    {
        None
    }
}

fn muse_cli_token() -> Option<MuseToken> {
    if let Some(token) = muse_cli_keychain_token() {
        return Some(token);
    }

    #[derive(Deserialize)]
    struct AuthFile {
        providers: Providers,
    }
    #[derive(Deserialize)]
    struct Providers {
        meta: MetaSlot,
    }
    #[derive(Deserialize)]
    struct MetaSlot {
        mechanism: String,
        #[serde(default)]
        access_token: String,
        #[serde(default)]
        api_key: String,
        expires_at: Option<f64>,
    }

    let raw = std::fs::read_to_string(muse_cli_auth_path()).ok()?;
    let parsed: AuthFile = serde_json::from_str(&raw).ok()?;
    if parsed.providers.meta.mechanism != "oauth" {
        return None;
    }
    let api_key = parsed.providers.meta.api_key;
    let access_token = parsed.providers.meta.access_token;
    if api_key.is_empty() && access_token.is_empty() {
        return None;
    }
    let expires_at = parsed
        .providers
        .meta
        .expires_at
        .and_then(|ts| DateTime::from_timestamp(ts as i64, 0))
        .unwrap_or_else(minted_key_expiry);
    Some(MuseToken {
        access_token,
        refresh_token: String::new(),
        api_key,
        expires_at,
    })
}

impl TokenCache {
    fn new() -> Self {
        Self {
            path: Paths::in_config_dir("muse_code/token.json"),
        }
    }

    fn load(&self) -> Option<MuseToken> {
        let raw = std::fs::read_to_string(&self.path).ok()?;
        match serde_json::from_str(&raw) {
            Ok(token) => Some(token),
            Err(e) => {
                tracing::warn!(
                    "muse_code token cache at {:?} is corrupted ({}); ignoring",
                    self.path,
                    e
                );
                None
            }
        }
    }

    pub(crate) fn has_token(&self) -> bool {
        self.load().is_some()
    }

    fn save(&self, token: &MuseToken) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        write_private_file(&self.path, &serde_json::to_string(token)?)?;
        Ok(())
    }

    fn clear(&self) -> anyhow::Result<()> {
        let result = match std::fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        };
        if result.is_ok() {
            MUSE_TOKEN_GENERATION.fetch_add(1, Ordering::SeqCst);
        }
        result
    }
}

impl MuseCodeAuth {
    fn oauth_headers() -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(ACCEPT, HeaderValue::from_static("application/json"));
        headers.insert(USER_AGENT, HeaderValue::from_static(MUSE_CODE_USER_AGENT));
        headers
    }

    fn device_flow_config<'a>(
        &'a self,
        device_auth_url: &'a str,
        token_url: &'a str,
    ) -> DeviceFlowConfig<'a> {
        DeviceFlowConfig {
            device_auth_url: Some(device_auth_url),
            token_url,
            client_id: &self.client_id,
            scopes: None,
            extra_headers: Self::oauth_headers(),
            encoding: RequestEncoding::Form,
        }
    }

    async fn device_flow_login(&self) -> Result<MuseToken> {
        let device_auth_url = join_url(&self.auth_host, "/oidc/device/authorization/");
        let token_url = join_url(&self.auth_host, "/oidc/device/token/");
        let cfg = self.device_flow_config(&device_auth_url, &token_url);
        let tokens = run_device_flow(&self.client, &cfg).await?;
        let mut token = tokens_to_muse(tokens);
        token.api_key = self.mint_api_key(&token.access_token).await?;
        token.expires_at = minted_key_expiry();
        Ok(token)
    }

    async fn mint_api_key(&self, access_token: &str) -> Result<String> {
        let url = join_url(&self.api_host, "/muse-code/key");
        let response = self
            .client
            .post(url)
            .header(ACCEPT, "application/json")
            .header(USER_AGENT, MUSE_CODE_USER_AGENT)
            .header("x-api-version", "1.0.0")
            .bearer_auth(access_token)
            .json(&serde_json::json!({}))
            .send()
            .await?;
        let status = response.status();
        let bytes = response.bytes().await?;
        if !status.is_success() {
            return Err(mint_error(status, &String::from_utf8_lossy(&bytes)).into());
        }
        let minted: MintedKey = serde_json::from_slice(&bytes)?;
        minted
            .api_key
            .filter(|key| !key.is_empty())
            .ok_or_else(|| anyhow::anyhow!("mint response missing api_key"))
    }

    async fn renew_api_key(&self, token: &MuseToken) -> Result<MuseToken, ProviderError> {
        if token.access_token.is_empty() {
            return Err(ProviderError::Authentication(
                "Meta session cannot be renewed. Sign in to Muse Code again.".to_string(),
            ));
        }
        let api_key = self
            .mint_api_key(&token.access_token)
            .await
            .map_err(|error| match error.downcast::<ProviderError>() {
                Ok(provider_error) => provider_error,
                Err(error) => {
                    ProviderError::Authentication(format!("Failed to renew Muse API key: {error}"))
                }
            })?;
        Ok(MuseToken {
            api_key,
            expires_at: minted_key_expiry(),
            ..token.clone()
        })
    }

    async fn get_valid_token(&self) -> Result<MuseToken, ProviderError> {
        let generation = MUSE_TOKEN_GENERATION.load(Ordering::SeqCst);
        if let Some(token) = self.cache.load() {
            match self.use_or_refresh(token, generation).await {
                Ok(token) => return self.ensure_api_key(token, generation).await,
                Err(ProviderError::NotConfigured) => {}
                Err(error) => return Err(error),
            }
        }

        if let Some(token) = muse_cli_token() {
            if token.expires_at > Utc::now() {
                return self.ensure_api_key(token, generation).await;
            }
        }

        Err(ProviderError::NotConfigured)
    }

    async fn ensure_api_key(
        &self,
        mut token: MuseToken,
        generation: u64,
    ) -> Result<MuseToken, ProviderError> {
        if !token.api_key.is_empty() {
            return Ok(token);
        }
        if token.access_token.is_empty() {
            return Err(ProviderError::NotConfigured);
        }
        token.api_key = self
            .mint_api_key(&token.access_token)
            .await
            .map_err(|error| {
                ProviderError::Authentication(format!("Failed to mint Muse API key: {error}"))
            })?;
        self.save_unless_cleaned(&token, generation).await?;
        Ok(token)
    }

    async fn save_unless_cleaned(
        &self,
        token: &MuseToken,
        generation: u64,
    ) -> Result<(), ProviderError> {
        let _guard = global_muse_refresh_mutex().lock().await;
        if MUSE_TOKEN_GENERATION.load(Ordering::SeqCst) != generation {
            return Err(ProviderError::Authentication(
                "Muse Code credential was removed during authentication".to_string(),
            ));
        }
        self.cache.save(token).map_err(|error| {
            ProviderError::Authentication(format!("Failed to save Muse Code credential: {error}"))
        })
    }

    async fn use_or_refresh(
        &self,
        token: MuseToken,
        generation: u64,
    ) -> Result<MuseToken, ProviderError> {
        if !token.api_key.is_empty()
            && token.expires_at - Utc::now() > Duration::seconds(REFRESH_THRESHOLD_SECS)
        {
            return Ok(token);
        }

        if token.access_token.is_empty() {
            if !token.api_key.is_empty() && token.expires_at > Utc::now() {
                return Ok(token);
            }
            let _ = self.cache.clear();
            return Err(ProviderError::Authentication(
                "Meta session expired. Sign in to Muse Code again.".to_string(),
            ));
        }

        let _guard = global_muse_refresh_mutex().lock().await;
        if MUSE_TOKEN_GENERATION.load(Ordering::SeqCst) != generation {
            return Err(ProviderError::Authentication(
                "Muse Code credential was removed during authentication".to_string(),
            ));
        }
        if let Some(reloaded) = self.cache.load() {
            if reloaded != token
                && !reloaded.api_key.is_empty()
                && reloaded.expires_at - Utc::now() > Duration::seconds(REFRESH_THRESHOLD_SECS)
            {
                return Ok(reloaded);
            }
        }
        drop(_guard);

        match self.renew_api_key(&token).await {
            Ok(renewed) => {
                self.save_unless_cleaned(&renewed, generation).await?;
                Ok(renewed)
            }
            Err(error) => {
                if matches!(error, ProviderError::Authentication(_)) {
                    let _guard = global_muse_refresh_mutex().lock().await;
                    if MUSE_TOKEN_GENERATION.load(Ordering::SeqCst) == generation {
                        let _ = self.cache.clear();
                    }
                }
                Err(error)
            }
        }
    }
}

#[async_trait]
impl AuthProvider for SharedAuthProvider {
    async fn get_auth_header(&self) -> Result<(String, String)> {
        let token = self.0.get_valid_token().await.map_err(anyhow::Error::new)?;
        if token.api_key.is_empty() {
            return Err(ProviderError::Authentication(
                "Muse Code API key is missing. Sign in again.".to_string(),
            )
            .into());
        }
        Ok((
            "Authorization".to_string(),
            format!("Bearer {}", token.api_key),
        ))
    }
}

impl MuseCodeProvider {
    pub async fn cleanup() -> Result<()> {
        let _guard = global_muse_refresh_mutex().lock().await;
        TokenCache::new().clear()?;
        Ok(())
    }
}

async fn from_env(tls_config: Option<TlsConfig>) -> Result<MuseCodeProvider> {
    let config = Config::global();
    let host: String = config
        .get_param("MUSE_CODE_HOST")
        .unwrap_or_else(|_| MUSE_CODE_API_HOST.to_string());
    let auth_host: String = config
        .get_param("MUSE_CODE_AUTH_HOST")
        .unwrap_or_else(|_| MUSE_CODE_AUTH_HOST.to_string());
    let client_id: String = config
        .get_param("MUSE_CODE_CLIENT_ID")
        .unwrap_or_else(|_| MUSE_CODE_CLIENT_ID.to_string());

    let auth = Arc::new(MuseCodeAuth {
        cache: TokenCache::new(),
        client: ApiClient::http_client(tls_config.as_ref())?,
        api_host: host.clone(),
        auth_host,
        client_id,
    });

    let api_client = ApiClient::with_timeout_and_tls(
        host,
        AuthMethod::Custom(Box::new(SharedAuthProvider(Arc::clone(&auth)))),
        StdDuration::from_secs(goose_providers::api_client::DEFAULT_PROVIDER_TIMEOUT_SECS),
        tls_config,
    )?
    .with_request_builder(crate::session_context::session_id_request_builder())
    .with_header("anthropic-version", ANTHROPIC_API_VERSION)?;

    Ok(MuseCodeProvider {
        inner: AnthropicProviderBuilder::new(api_client)
            .name(MUSE_CODE_PROVIDER_NAME)
            .custom_models(Some(known_models()))
            .format_options(AnthropicFormatOptions {
                // Meta's Messages adapter is stateless. Replay thinking so
                // multi-turn tool loops keep the chain of thought.
                preserve_unsigned_thinking: true,
                preserve_thinking_context: true,
                ..AnthropicFormatOptions::default()
            })
            .build(),
        auth,
    })
}

impl ProviderDescriptor for MuseCodeProviderDef {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::with_models(
            MUSE_CODE_PROVIDER_NAME,
            "Meta Muse Code",
            "Muse Spark models from a Meta Muse Code subscription",
            "muse-spark-1.3",
            known_models(),
            MUSE_CODE_DOC_URL,
            vec![
                ConfigKey::new_oauth_device_code("MUSE_CODE_TOKEN", true, true, None, false),
                ConfigKey::new(
                    "MUSE_CODE_HOST",
                    false,
                    false,
                    Some(MUSE_CODE_API_HOST),
                    false,
                ),
                ConfigKey::new(
                    "MUSE_CODE_AUTH_HOST",
                    false,
                    false,
                    Some(MUSE_CODE_AUTH_HOST),
                    false,
                ),
                ConfigKey::new(
                    "MUSE_CODE_CLIENT_ID",
                    false,
                    false,
                    Some(MUSE_CODE_CLIENT_ID),
                    false,
                ),
            ],
        )
        .with_setup_steps(vec![
            "Run `goose configure` and select 'Meta Muse Code'",
            "A browser window will open — sign in to Meta and confirm the displayed code",
            "Once authorized, Goose will save your token automatically",
        ])
        .with_setup(
            bcaip_provider_types::ProviderSetupMetadata::new(
                bcaip_provider_types::ProviderSetupCategory::Model,
                bcaip_provider_types::ProviderSetupMethod::OauthDeviceCode,
                bcaip_provider_types::ProviderSetupGroup::Default,
            )
            .with_docs_url(MUSE_CODE_DOC_URL)
            .with_native_connect_query("Meta Muse Code")
            .with_capabilities(false, true, false),
        )
    }
}

impl ProviderDef for MuseCodeProviderDef {
    type Provider = MuseCodeProvider;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(from_env(tls_config))
    }
}

#[async_trait]
impl Provider for MuseCodeProvider {
    fn get_name(&self) -> &str {
        self.inner.get_name()
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        self.auth.get_valid_token().await?;
        self.inner
            .stream(model_config, system, messages, tools)
            .await
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        self.auth.get_valid_token().await?;
        self.inner.fetch_supported_models().await
    }

    async fn configure_oauth(&self) -> Result<(), ProviderError> {
        let generation = MUSE_TOKEN_GENERATION.load(Ordering::SeqCst);
        let token = self
            .auth
            .device_flow_login()
            .await
            .map_err(|e| ProviderError::Authentication(format!("OAuth flow failed: {e}")))?;
        self.auth.save_unless_cleaned(&token, generation).await
    }
}

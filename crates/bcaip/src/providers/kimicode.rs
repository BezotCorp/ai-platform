use super::base::ProviderDef;
use super::oauth_device_flow::{
    DeviceFlowConfig, DeviceFlowTokenRefreshError, DeviceFlowTokens, RequestEncoding,
    refresh_device_flow_token, run_device_flow,
};
use crate::config::paths::Paths;
use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use bcaip_provider_types::base::{ConfigKey, MessageStream, Provider, ProviderMetadata};
use bcaip_provider_types::conversations::Message;
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::formats::AnthropicFormatOptions;
use bcaip_provider_types::formats::{
    create_request_anthropic, response_to_streaming_message_anthropic,
};
use bcaip_provider_types::model::ModelConfig;
use bcaip_provider_types::request_log::{LoggerHandleExt, start_log};
use bcaip_provider_types::retry::ProviderRetry;
use bcaip_providers::api_client::RequestBuilderDecorator;
use bcaip_providers::api_client::{DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_PROVIDER_TIMEOUT_SECS};
use bcaip_providers::openai_compatible::handle_status;
use chrono::{DateTime, Duration, Utc};
use futures::TryStreamExt;
use futures::future::BoxFuture;
use reqwest::Client;
use reqwest::header::{HeaderMap, HeaderValue};
use rmcp::model::Tool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{io, time::Duration as StdDuration};
use tokio::pin;
use tokio_util::io::StreamReader;
use uuid::Uuid;
const KIMI_CODE_PROVIDER_NAME: &str = "kimi_code";
pub const KIMI_CODE_DEFAULT_MODEL: &str = "kimi-for-coding";
/// Known models for the provider metadata registration. The live catalogue is
/// fetched from `/v1/models` at request time; this constant is only used for
/// `ProviderMetadata`, e.g. when the catalogue fetch fails. As of 2026-07 the
/// platform serves these ids verbatim (`k3`, not `kimi-k3`).
pub const KIMI_CODE_KNOWN_MODELS: &[&str] = &[
    "kimi-for-coding",
    "kimi-for-coding-highspeed",
    "k3",
    "k3-256k",
];

const KIMI_CODE_DOC_URL: &str = "https://www.kimi.com/code/docs/en/";
const KIMI_CODE_CLIENT_ID: &str = "17e5f671-d194-4dfb-9706-5516cb48c098";
const KIMI_AUTH_HOST: &str = "https://auth.kimi.com";
const KIMI_API_BASE: &str = "https://api.kimi.com/coding";
const KIMI_MSH_PLATFORM: &str = "kimi_cli";
const KIMI_MSH_VERSION: &str = "0.1.0";

/// Refresh the access token if it expires within this many seconds.
const REFRESH_THRESHOLD_SECS: i64 = 300;

/// Fallback access-token lifetime when the server omits `expires_in`.
const DEFAULT_TOKEN_LIFETIME_SECS: i64 = 3600;

// ── Token persistence ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
struct KimiToken {
    access_token: String,
    refresh_token: String,
    expires_at: DateTime<Utc>,
}

/// Normalize helper output into the on-disk `KimiToken` shape. When the helper
/// returns `None` for `refresh_token` or `expires_at`, fall back to the prior
/// refresh token (per RFC 6749 §6) and a default lifetime.
fn tokens_to_kimi(tokens: DeviceFlowTokens, prior_refresh: Option<&str>) -> KimiToken {
    let refresh_token = tokens
        .refresh_token
        .or_else(|| prior_refresh.map(str::to_string))
        .unwrap_or_default();
    let expires_at = tokens
        .expires_at
        .unwrap_or_else(|| Utc::now() + Duration::seconds(DEFAULT_TOKEN_LIFETIME_SECS));
    KimiToken {
        access_token: tokens.access_token,
        refresh_token,
        expires_at,
    }
}

#[derive(Debug)]
struct TokenCache {
    path: std::path::PathBuf,
}

pub(crate) fn has_configured_token() -> bool {
    std::fs::read_to_string(TokenCache::new().path)
        .ok()
        .and_then(|raw| serde_json::from_str::<KimiToken>(&raw).ok())
        .is_some()
}

impl TokenCache {
    fn new() -> Self {
        Self {
            path: Paths::in_config_dir("kimicode/token.json"),
        }
    }

    async fn load(&self) -> Option<KimiToken> {
        let raw = tokio::fs::read_to_string(&self.path).await.ok()?;
        match serde_json::from_str(&raw) {
            Ok(token) => Some(token),
            Err(e) => {
                tracing::warn!(
                    "kimicode token cache at {:?} is corrupted ({}); ignoring and re-authenticating",
                    self.path,
                    e
                );
                None
            }
        }
    }

    async fn save(&self, token: &KimiToken) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&self.path, serde_json::to_string(token)?).await?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tokio::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(0o600)).await?;
        }
        Ok(())
    }

    async fn clear(&self) -> Result<()> {
        match tokio::fs::remove_file(&self.path).await {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.into()),
        }
    }
}

// ── Provider ─────────────────────────────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct KimiCodeProvider {
    #[serde(skip)]
    client: Client,
    #[serde(skip)]
    token_cache: TokenCache,
    #[serde(skip)]
    cached_token: tokio::sync::Mutex<Option<KimiToken>>,
    #[serde(skip)]
    device_id: String,
    #[serde(skip)]
    auth_host: String,
    #[serde(skip)]
    api_base: String,
    #[serde(skip)]
    name: String,
    #[serde(skip)]
    request_builder: RequestBuilderDecorator,
}

impl KimiCodeProvider {
    pub async fn cleanup() -> Result<()> {
        TokenCache::new().clear().await
    }

    pub async fn from_env(
        _tls_config: Option<bcaip_providers::api_client::TlsConfig>,
    ) -> Result<Self> {
        let client = Client::builder()
            .connect_timeout(StdDuration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS))
            .read_timeout(StdDuration::from_secs(DEFAULT_PROVIDER_TIMEOUT_SECS))
            .build()?;
        let device_id = Self::get_or_create_device_id().await?;
        Ok(Self {
            client,
            token_cache: TokenCache::new(),
            cached_token: tokio::sync::Mutex::new(None),
            device_id,
            auth_host: KIMI_AUTH_HOST.to_string(),
            api_base: KIMI_API_BASE.to_string(),
            name: KIMI_CODE_PROVIDER_NAME.to_string(),
            request_builder: crate::session_context::session_id_request_builder(),
        })
    }

    fn is_valid_device_id(id: &str) -> bool {
        !id.is_empty() && HeaderValue::from_str(id).is_ok()
    }

    async fn get_or_create_device_id() -> Result<String> {
        let path = Paths::in_config_dir("kimicode/device_id");
        if let Ok(raw) = tokio::fs::read_to_string(&path).await {
            let id = raw.trim().to_string();
            if Self::is_valid_device_id(&id) {
                return Ok(id);
            }
            tracing::warn!("kimicode device_id at {:?} is invalid; regenerating", path);
        }
        let id = Uuid::new_v4().to_string().replace('-', "");
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        tokio::fs::write(&path, &id).await?;
        Ok(id)
    }

    fn kimi_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            "X-Msh-Platform",
            HeaderValue::from_static(KIMI_MSH_PLATFORM),
        );
        headers.insert("X-Msh-Version", HeaderValue::from_static(KIMI_MSH_VERSION));
        // Normally validated in `get_or_create_device_id`; skip the header if
        // that validation was bypassed (e.g. test-constructed provider).
        if let Ok(value) = HeaderValue::from_str(&self.device_id) {
            headers.insert("X-Msh-Device-Id", value);
        }
        headers
    }

    // ── Token management ─────────────────────────────────────────────────────

    async fn get_access_token(&self) -> Result<String, ProviderError> {
        Ok(self.ensure_token().await?.access_token)
    }

    async fn ensure_token(&self) -> Result<KimiToken, ProviderError> {
        let mut guard = self.cached_token.lock().await;

        if let Some(token) = guard.clone() {
            let usable = self.use_or_refresh(token).await?;
            *guard = Some(usable.clone());
            return Ok(usable);
        }

        if let Some(token) = self.token_cache.load().await {
            let usable = self.use_or_refresh(token).await?;
            *guard = Some(usable.clone());
            return Ok(usable);
        }

        Err(ProviderError::NotConfigured)
    }

    async fn use_or_refresh(&self, mut token: KimiToken) -> Result<KimiToken, ProviderError> {
        let mut reloaded = false;

        loop {
            if token.expires_at - Utc::now() > Duration::seconds(REFRESH_THRESHOLD_SECS) {
                return Ok(token);
            }
            match self.do_refresh_token(&token.refresh_token).await {
                Ok(refreshed) => {
                    tracing::debug!("kimicode: token refreshed");
                    if let Err(e) = self.token_cache.save(&refreshed).await {
                        tracing::warn!("failed to persist refreshed kimicode token: {}", e);
                    }
                    return Ok(refreshed);
                }
                Err(error) => {
                    tracing::debug!("kimicode: token refresh failed: {}", error);
                    if !reloaded {
                        reloaded = true;
                        if let Some(persisted) = self.token_cache.load().await
                            && persisted != token
                        {
                            token = persisted;
                            continue;
                        }
                    }
                    if token.expires_at > Utc::now() {
                        tracing::debug!("kimicode: falling back to still-unexpired token");
                        return Ok(token);
                    }
                    return Err(kimi_refresh_error(error));
                }
            }
        }
    }

    async fn device_flow_login(&self) -> Result<KimiToken> {
        let device_auth_url = format!("{}/api/oauth/device_authorization", self.auth_host);
        let token_url = format!("{}/api/oauth/token", self.auth_host);
        let cfg = DeviceFlowConfig {
            device_auth_url: Some(&device_auth_url),
            token_url: &token_url,
            client_id: KIMI_CODE_CLIENT_ID,
            scopes: None,
            extra_headers: self.kimi_headers(),
            encoding: RequestEncoding::Form,
        };
        let tokens = run_device_flow(&self.client, &cfg).await?;
        Ok(tokens_to_kimi(tokens, None))
    }

    async fn do_refresh_token(&self, refresh_token: &str) -> Result<KimiToken> {
        let token_url = format!("{}/api/oauth/token", self.auth_host);
        let cfg = DeviceFlowConfig {
            device_auth_url: None,
            token_url: &token_url,
            client_id: KIMI_CODE_CLIENT_ID,
            scopes: None,
            extra_headers: self.kimi_headers(),
            encoding: RequestEncoding::Form,
        };
        let tokens = refresh_device_flow_token(&self.client, &cfg, refresh_token).await?;
        // RFC 6749 §6: the server MAY omit `refresh_token` from a refresh
        // response, in which case the client should keep reusing the prior one.
        Ok(tokens_to_kimi(tokens, Some(refresh_token)))
    }

    // ── HTTP ─────────────────────────────────────────────────────────────────

    async fn post(&self, payload: &Value) -> Result<reqwest::Response, ProviderError> {
        let access_token = self.get_access_token().await?;

        let builder = self
            .client
            .post(format!("{}/v1/messages", self.api_base))
            .bearer_auth(access_token)
            .headers(self.kimi_headers())
            .json(payload);

        let request = (self.request_builder)(builder)
            .map_err(|e| ProviderError::ExecutionError(e.to_string()))?;
        bcaip_providers::http_status::send_bounded(
            request,
            StdDuration::from_secs(DEFAULT_PROVIDER_TIMEOUT_SECS),
        )
        .await
    }
}

fn kimi_refresh_error(error: anyhow::Error) -> ProviderError {
    let refresh_error = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<DeviceFlowTokenRefreshError>());
    let status = refresh_error.map(|error| error.status).or_else(|| {
        error
            .chain()
            .find_map(|cause| cause.downcast_ref::<reqwest::Error>())
            .and_then(reqwest::Error::status)
    });
    let details = error.to_string();

    if refresh_error.and_then(|error| error.error.as_deref()) == Some("invalid_grant") {
        return ProviderError::Authentication(details);
    }

    match status {
        Some(reqwest::StatusCode::TOO_MANY_REQUESTS) => ProviderError::RateLimitExceeded {
            details,
            retry_delay: None,
        },
        Some(status) if status.is_server_error() => ProviderError::ServerError(details),
        Some(_) => ProviderError::RequestFailed(details),
        _ => ProviderError::from(error),
    }
}

// ── ProviderDef ───────────────────────────────────────────────────────────────

impl bcaip_provider_types::base::ProviderDescriptor for KimiCodeProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            KIMI_CODE_PROVIDER_NAME,
            "Kimi Code",
            "Kimi Code AI models optimized for coding tasks",
            KIMI_CODE_DEFAULT_MODEL,
            KIMI_CODE_KNOWN_MODELS.to_vec(),
            KIMI_CODE_DOC_URL,
            vec![ConfigKey::new_oauth_device_code(
                "KIMI_CODE_TOKEN",
                true,
                true,
                None,
                false,
            )],
        )
        .with_setup_steps(vec![
            "Run `bcaip configure` and select 'Kimi Code'",
            "A browser window will open — log in to kimi.com and enter the displayed code",
            "Once authorized, Bcaip will save your token automatically",
        ])
    }
}

impl ProviderDef for KimiCodeProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<bcaip_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(Self::from_env(tls_config))
    }
}

// ── Provider trait ────────────────────────────────────────────────────────────

#[async_trait]
impl Provider for KimiCodeProvider {
    fn get_name(&self) -> &str {
        &self.name
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let mut payload = create_request_anthropic(
            KIMI_CODE_PROVIDER_NAME,
            model_config,
            system,
            messages,
            tools,
            AnthropicFormatOptions::default(),
        )
        .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;
        payload
            .as_object_mut()
            .unwrap()
            .insert("stream".to_string(), Value::Bool(true));

        let mut log = start_log(model_config, &payload)
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;

        let response = self
            .with_retry(|| async {
                let resp = self.post(&payload).await?;
                handle_status(resp).await
            })
            .await
            .inspect_err(|e| {
                let _ = log.error(e);
            })?;

        let stream = response.bytes_stream().map_err(io::Error::other);

        Ok(Box::pin(try_stream! {
            let stream_reader = StreamReader::new(stream);
            let framed = tokio_util::codec::FramedRead::new(
                stream_reader,
                tokio_util::codec::LinesCodec::new(),
            )
            .map_err(anyhow::Error::from);

            let message_stream = response_to_streaming_message_anthropic(framed);
            pin!(message_stream);
            while let Some(message) = futures::StreamExt::next(&mut message_stream).await {
                let (message, usage) = message.map_err(ProviderError::from_stream_error)?;
                log.write(&message, usage.as_ref().map(|f| f.usage).as_ref())?;
                yield (message, usage);
            }
        }))
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        #[derive(Deserialize)]
        struct ModelEntry {
            id: String,
        }
        #[derive(Deserialize)]
        struct ModelsResp {
            data: Vec<ModelEntry>,
        }

        let access_token = self.get_access_token().await?;

        let resp = self
            .client
            .get(format!("{}/v1/models", self.api_base))
            .timeout(StdDuration::from_secs(DEFAULT_PROVIDER_TIMEOUT_SECS))
            .bearer_auth(access_token)
            .headers(self.kimi_headers())
            .send()
            .await
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;
        let resp = handle_status(resp).await?;

        let parsed: ModelsResp = resp.json().await.map_err(|e| {
            ProviderError::RequestFailed(format!("/v1/models body is not valid JSON: {}", e))
        })?;
        let mut models: Vec<String> = parsed.data.into_iter().map(|m| m.id).collect();
        models.sort();
        Ok(models)
    }

    async fn configure_oauth(&self) -> Result<(), ProviderError> {
        match self.ensure_token().await {
            Ok(_) => {}
            Err(ProviderError::NotConfigured | ProviderError::Authentication(_)) => {
                let token = self.device_flow_login().await.map_err(|e| {
                    ProviderError::Authentication(format!("OAuth flow failed: {}", e))
                })?;
                self.token_cache.save(&token).await.map_err(|e| {
                    ProviderError::Authentication(format!("Failed to save OAuth token: {}", e))
                })?;
                *self.cached_token.lock().await = Some(token);
            }
            Err(error) => return Err(error),
        }

        Ok(())
    }
}

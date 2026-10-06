use super::base::ProviderDef;
use super::command_auth::CommandAuthProvider;
use super::huggingface_auth;
use crate::config::{Config, ConfigError};
use crate::session_context::{
    session_id_request_builder, session_id_request_builder_with_header_override,
};
use anyhow::{Result, anyhow};
use futures::future::BoxFuture;
use bcaip_provider_types::base::{ConfigKey, MessageStream, ModelInfo, Provider, ProviderMetadata};
use bcaip_provider_types::conversations::Message;
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::model::ModelConfig;
use goose_providers::api_client::DEFAULT_PROVIDER_TIMEOUT_SECS;
use goose_providers::api_client::{ApiClient, AuthMethod, AuthProvider};
use goose_providers::declarative::DeclarativeProviderConfig;
use goose_providers::openai_compatible::OpenAiCompatibleProvider;
use rmcp::model::Tool;
pub const HUGGINGFACE_API_HOST: &str = "https://router.huggingface.co/v1";
pub const HUGGINGFACE_DOC_URL: &str = "https://huggingface.co/docs/inference-providers";
pub const HUGGINGFACE_DEFAULT_MODEL: &str = "Qwen/Qwen3-Coder-480B-A35B-Instruct";
pub const HUGGINGFACE_KNOWN_MODELS: &[&str] = &[
    "MiniMaxAI/MiniMax-M2.1",
    "MiniMaxAI/MiniMax-M2.5",
    "MiniMaxAI/MiniMax-M2.7",
    "Qwen/Qwen3-235B-A22B-Thinking",
    "Qwen/Qwen3-Coder-480B-A35B-Instruct",
    "Qwen/Qwen3-Coder-Next",
    "Qwen/Qwen3-Embedding-4B",
    "Qwen/Qwen3-Embedding-8B",
    "Qwen/Qwen3-Next-80B-A3B-Instruct",
    "Qwen/Qwen3-Next-80B-A3B-Thinking",
    "Qwen/Qwen3.5-397B-A17B",
    "XiaomiMiMo/MiMo-V2-Flash",
    "deepseek-ai/DeepSeek-R1",
    "deepseek-ai/DeepSeek-V3.2",
    "deepseek-ai/DeepSeek-V4-Pro",
    "moonshotai/Kimi-K2-Instruct",
    "moonshotai/Kimi-K2-Thinking",
    "moonshotai/Kimi-K2.5",
    "moonshotai/Kimi-K2.6",
    "zai-org/GLM-4.7",
    "zai-org/GLM-4.7-Flash",
    "zai-org/GLM-5",
    "zai-org/GLM-5.1",
];

type QueryParams = Vec<(String, String)>;
type EndpointParts = (String, String, QueryParams);

pub struct HuggingFaceProvider {
    inner: OpenAiCompatibleProvider,
    custom_models: Option<Vec<ModelInfo>>,
    dynamic_models: Option<bool>,
}

struct HuggingFaceAuthProvider;

#[async_trait::async_trait]
impl AuthProvider for HuggingFaceAuthProvider {
    async fn get_auth_header(&self) -> Result<(String, String)> {
        let token = huggingface_auth::resolve_token_async()
            .await?
            .ok_or_else(missing_token_error)?;
        Ok(("Authorization".to_string(), format!("Bearer {}", token)))
    }
}

impl HuggingFaceProvider {
    pub fn matches_declarative_config(config: &DeclarativeProviderConfig) -> bool {
        config.name == huggingface_auth::HUGGINGFACE_PROVIDER_NAME
            || config.catalog_provider_id.as_deref()
                == Some(huggingface_auth::HUGGINGFACE_PROVIDER_NAME)
    }

    pub fn from_custom_config(
        config: DeclarativeProviderConfig,
        tls_config: Option<goose_providers::api_client::TlsConfig>,
    ) -> Result<Self> {
        let custom_models = static_models(&config);
        if config.dynamic_models == Some(false) && custom_models.is_none() {
            return Err(anyhow!(
                "Provider '{}' has dynamic_models: false but no static models listed; \
                 at least one entry in `models` is required.",
                config.name
            ));
        }

        config.validate_auth()?;
        let auth_method = match config.auth.as_ref() {
            Some(auth_config) => AuthMethod::Custom(Box::new(CommandAuthProvider::new(
                auth_config,
                "Authorization",
                "Bearer ",
            ))),
            None => custom_auth_method(&config)?,
        };
        let (host, completions_prefix, query_params) =
            openai_compatible_endpoint_parts(&config.base_url, config.base_path.as_deref())?;

        let request_builder = session_id_request_builder_with_header_override(
            config.session_id_header_override.as_deref(),
        )?;
        let timeout_secs = config
            .timeout_seconds
            .unwrap_or(DEFAULT_PROVIDER_TIMEOUT_SECS);
        let mut api_client = ApiClient::with_timeout_and_tls(
            host,
            auth_method,
            std::time::Duration::from_secs(timeout_secs),
            tls_config,
        )?
        .with_request_builder(request_builder)
        .with_query(query_params);

        if let Some(headers) = &config.headers {
            let mut header_map = reqwest::header::HeaderMap::new();
            for (key, value) in headers {
                let header_name = reqwest::header::HeaderName::from_bytes(key.as_bytes())?;
                let header_value = reqwest::header::HeaderValue::from_str(value)?;
                header_map.insert(header_name, header_value);
            }
            api_client = api_client.with_headers(header_map)?;
        }

        Ok(Self {
            inner: OpenAiCompatibleProvider::new(
                config.name.clone(),
                api_client,
                completions_prefix,
            )
            .with_supports_streaming(config.supports_streaming.unwrap_or(true)),
            custom_models,
            dynamic_models: config.dynamic_models,
        })
    }

    pub async fn cleanup() -> Result<()> {
        huggingface_auth::clear_oauth_token()
    }
}

#[async_trait::async_trait]
impl Provider for HuggingFaceProvider {
    fn get_name(&self) -> &str {
        self.inner.get_name()
    }

    async fn get_context_limit(&self, model: &str, override_limit: Option<usize>) -> usize {
        let configured_limits = self
            .custom_models
            .iter()
            .flatten()
            .filter_map(|model| model.context_limit.map(|limit| (model.name.clone(), limit)));
        bcaip_provider_types::context_limit::ContextLimitResolver::new(self.get_name())
            .with_configured_limits(configured_limits)
            .resolve(model, override_limit, || async { Ok(None) })
            .await
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        if let Some(custom_models) = &self.custom_models {
            if self.dynamic_models == Some(false) {
                return Ok(custom_models
                    .iter()
                    .map(|model| model.name.clone())
                    .collect());
            }

            match self.inner.fetch_supported_models().await {
                Ok(models) => return Ok(models),
                Err(e) if e.is_endpoint_not_found() => {
                    tracing::debug!(
                        "Models endpoint not implemented for Hugging Face provider '{}' ({}), using predefined list",
                        self.inner.get_name(),
                        e
                    );
                    return Ok(custom_models
                        .iter()
                        .map(|model| model.name.clone())
                        .collect());
                }
                Err(e) => return Err(e),
            }
        }

        self.inner.fetch_supported_models().await
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        self.inner
            .stream(model_config, system, messages, tools)
            .await
    }
}

impl bcaip_provider_types::base::ProviderDescriptor for HuggingFaceProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            huggingface_auth::HUGGINGFACE_PROVIDER_NAME,
            huggingface_auth::HUGGINGFACE_DISPLAY_NAME,
            "Hugging Face Inference Providers via the Hugging Face Router",
            HUGGINGFACE_DEFAULT_MODEL,
            HUGGINGFACE_KNOWN_MODELS.to_vec(),
            HUGGINGFACE_DOC_URL,
            vec![
                ConfigKey::new(
                    huggingface_auth::HUGGINGFACE_TOKEN_SECRET_KEY,
                    true,
                    true,
                    None,
                    true,
                ),
                ConfigKey::new("HF_HOST", false, false, Some(HUGGINGFACE_API_HOST), false),
            ],
        )
        .with_setup(
            bcaip_provider_types::ProviderSetupMetadata::api_key(
                bcaip_provider_types::ProviderSetupGroup::Default,
            )
            .with_docs_url("https://huggingface.co/docs/inference-providers")
            .with_aliases(&["huggingface", "hf"]),
        )
    }
}

impl ProviderDef for HuggingFaceProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<goose_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(async move {
            let config = Config::global();
            let auth_method =
                refreshable_huggingface_auth_method(huggingface_auth::has_configured_token)?;
            let host: String = config
                .get_param("HF_HOST")
                .unwrap_or_else(|_| HUGGINGFACE_API_HOST.to_string());
            let api_client = ApiClient::new_with_tls(host, auth_method, tls_config)?
                .with_request_builder(session_id_request_builder());

            Ok(Self {
                inner: OpenAiCompatibleProvider::new(
                    huggingface_auth::HUGGINGFACE_PROVIDER_NAME.to_string(),
                    api_client,
                    String::new(),
                ),
                custom_models: None,
                dynamic_models: None,
            })
        })
    }
}

fn missing_token_error() -> anyhow::Error {
    anyhow!(
        "Hugging Face token is not configured. Sign in from Settings > Auth or configure HF_TOKEN."
    )
}

fn configured_api_key(config: &DeclarativeProviderConfig) -> Result<Option<String>> {
    if config.api_key_env.is_empty() {
        return Ok(None);
    }

    match Config::global().get_secret::<String>(&config.api_key_env) {
        Ok(token) => Ok(Some(token)),
        Err(ConfigError::NotFound(_)) => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn static_models(config: &DeclarativeProviderConfig) -> Option<Vec<ModelInfo>> {
    (!config.models.is_empty()).then(|| config.models.clone())
}

fn custom_auth_method(config: &DeclarativeProviderConfig) -> Result<AuthMethod> {
    let configured_key = if config.requires_auth {
        configured_api_key(config)?
    } else {
        None
    };
    custom_auth_method_with_provider_token(config.requires_auth, configured_key)
}

fn custom_auth_method_with_provider_token(
    requires_auth: bool,
    provider_token: Option<String>,
) -> Result<AuthMethod> {
    custom_auth_method_from_sources(
        requires_auth,
        provider_token,
        huggingface_auth::has_configured_token,
    )
}

fn custom_auth_method_from_sources(
    requires_auth: bool,
    provider_token: Option<String>,
    has_global_token: impl FnOnce() -> Result<bool>,
) -> Result<AuthMethod> {
    if !requires_auth {
        return Ok(AuthMethod::NoAuth);
    }

    if let Some(token) = provider_token {
        return Ok(AuthMethod::BearerToken(token));
    }

    refreshable_huggingface_auth_method(has_global_token)
}

fn refreshable_huggingface_auth_method(
    has_configured_token: impl FnOnce() -> Result<bool>,
) -> Result<AuthMethod> {
    if !has_configured_token()? {
        return Err(missing_token_error());
    }

    Ok(AuthMethod::Custom(Box::new(HuggingFaceAuthProvider)))
}

fn openai_compatible_endpoint_parts(
    base_url: &str,
    base_path: Option<&str>,
) -> Result<EndpointParts> {
    let url =
        url::Url::parse(base_url).map_err(|e| anyhow!("Invalid base URL '{}': {}", base_url, e))?;
    let mut host = if let Some(port) = url.port() {
        format!(
            "{}://{}:{}",
            url.scheme(),
            url.host_str().unwrap_or_default(),
            port
        )
    } else {
        format!("{}://{}", url.scheme(), url.host_str().unwrap_or_default())
    };
    let query_params = url
        .query_pairs()
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect::<Vec<_>>();

    if let Some(path) = base_path {
        return Ok((host, completions_prefix(path), query_params));
    }

    let path = url.path().trim_matches('/');
    if path.is_empty() {
        return Ok((host, String::new(), query_params));
    }

    if let Some(parent) = path
        .strip_suffix("/chat/completions")
        .or_else(|| (path == "chat/completions").then_some(""))
    {
        if !parent.is_empty() {
            host.push('/');
            host.push_str(parent);
        }
        return Ok((host, String::new(), query_params));
    }

    host.push('/');
    host.push_str(path);
    Ok((host, String::new(), query_params))
}

fn completions_prefix(path: &str) -> String {
    let path = path.trim_matches('/');
    if path.is_empty() {
        return String::new();
    }

    let parent = path
        .strip_suffix("/chat/completions")
        .or_else(|| (path == "chat/completions").then_some(""))
        .unwrap_or(path);

    if parent.is_empty() {
        String::new()
    } else {
        format!("{}/", parent)
    }
}

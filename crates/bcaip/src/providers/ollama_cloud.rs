use super::base::ProviderDef;
use crate::config::Config;
use crate::session_context::session_id_request_builder_with_header_override;
use anyhow::Result;
use bcaip_provider_types::base::{ConfigKey, MessageStream, Provider, ProviderMetadata};
use bcaip_provider_types::base::{ModelInfo, ProviderDescriptor};
use bcaip_provider_types::conversations::Message;
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::model::ModelConfig;
use bcaip_providers::api_client::{ApiClient, AuthMethod, TlsConfig};
use bcaip_providers::declarative::DeclarativeProviderConfig;
use bcaip_providers::ollama::fetch_ollama_model_names;
use bcaip_providers::openai::OpenAiProvider;
use futures::future::BoxFuture;
use rmcp::model::Tool;
use serde_json::Value;
use std::{collections::HashMap, sync::Mutex};
use tokio::sync::OnceCell;
const OLLAMA_CLOUD_PROVIDER_NAME: &str = "ollama_cloud";
const SHOW_INFO_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct OllamaCloudProvider {
    inner: OpenAiProvider,
    ollama_api_client: ApiClient,
    model_names: OnceCell<Vec<String>>,
    context_limits: Mutex<HashMap<String, Option<usize>>>,
    custom_models: Option<Vec<ModelInfo>>,
    dynamic_models: Option<bool>,
}

impl OllamaCloudProvider {
    pub fn matches_declarative_config(config: &DeclarativeProviderConfig) -> bool {
        config.name == OLLAMA_CLOUD_PROVIDER_NAME
            || config.catalog_provider_id.as_deref() == Some(OLLAMA_CLOUD_PROVIDER_NAME)
    }

    pub fn from_custom_config(
        config: DeclarativeProviderConfig,
        tls_config: Option<TlsConfig>,
    ) -> Result<Self> {
        let inner =
            crate::providers::openai_def::from_custom_config(config.clone(), tls_config.clone())?;

        let custom_models = if !config.models.is_empty() {
            Some(config.models.clone())
        } else {
            None
        };

        if config.dynamic_models == Some(false) && custom_models.is_none() {
            return Err(anyhow::anyhow!(
                "Provider '{}' has dynamic_models: false but no static models listed; \
                 at least one entry in `models` is required.",
                config.name
            ));
        }

        let ollama_api_client = build_ollama_api_client(&config, tls_config)?;

        Ok(Self {
            inner,
            ollama_api_client,
            model_names: OnceCell::new(),
            context_limits: Mutex::new(HashMap::new()),
            custom_models,
            dynamic_models: config.dynamic_models,
        })
    }

    async fn get_or_fetch_model_names(&self) -> Result<Vec<String>, ProviderError> {
        self.model_names
            .get_or_try_init(|| {
                Box::pin(async {
                    Ok(fetch_ollama_model_names(&self.ollama_api_client)
                        .await?
                        .unwrap_or_default())
                })
            })
            .await
            .map(|v| v.to_vec())
    }

    async fn fetch_context_limit_from_show(&self, model_name: &str) -> Option<usize> {
        let payload = serde_json::json!({ "model": model_name });
        let response = self
            .ollama_api_client
            .request("api/show")
            .response_post(&payload)
            .await
            .ok()?;

        if !response.status().is_success() {
            return None;
        }

        let json: Value = response.json().await.ok()?;
        json.get("model_info")
            .and_then(|info| info.as_object())
            .and_then(|obj| {
                obj.iter().find_map(|(key, value)| {
                    key.ends_with(".context_length")
                        .then(|| value.as_u64().map(|n| n as usize))
                        .flatten()
                })
            })
    }
}

fn build_ollama_api_client(
    config: &DeclarativeProviderConfig,
    tls_config: Option<TlsConfig>,
) -> Result<ApiClient> {
    let normalized_base_url = bcaip_providers::openai::ensure_url_scheme(&config.base_url);
    let url = url::Url::parse(&normalized_base_url)
        .map_err(|e| anyhow::anyhow!("Invalid base URL '{}': {}", config.base_url, e))?;
    let host = url[..url::Position::BeforePath].to_string();

    let api_key = crate::providers::openai_def::resolve_api_key(config, &|key| {
        Config::global().get_secret(key)
    })?;

    let timeout_secs = config
        .timeout_seconds
        .unwrap_or(bcaip_providers::api_client::DEFAULT_PROVIDER_TIMEOUT_SECS);

    let auth = match api_key {
        Some(key) if !key.is_empty() => AuthMethod::BearerToken(key),
        _ => AuthMethod::NoAuth,
    };

    let mut api_client = ApiClient::with_timeout_and_tls(
        host,
        auth,
        std::time::Duration::from_secs(timeout_secs),
        tls_config,
    )?;

    if let Some(query) = url.query() {
        let query_params = url::form_urlencoded::parse(query.as_bytes())
            .map(|(key, value)| (key.into_owned(), value.into_owned()))
            .collect();
        api_client = api_client.with_query(query_params);
    }

    if let Some(headers) = &config.headers {
        let mut header_map = reqwest::header::HeaderMap::new();
        for (key, value) in headers {
            let header_name = reqwest::header::HeaderName::from_bytes(key.as_bytes())?;
            let header_value = reqwest::header::HeaderValue::from_str(value)?;
            header_map.insert(header_name, header_value);
        }
        api_client = api_client.with_headers(header_map)?;
    }

    let request_builder = session_id_request_builder_with_header_override(
        config.session_id_header_override.as_deref(),
    )?;
    Ok(api_client.with_request_builder(request_builder))
}

#[async_trait::async_trait]
impl Provider for OllamaCloudProvider {
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
        self.inner
            .stream(model_config, system, messages, tools)
            .await
    }

    fn skip_canonical_filtering(&self) -> bool {
        self.inner.skip_canonical_filtering()
    }

    fn retry_config(&self) -> bcaip_provider_types::retry::RetryConfig {
        self.inner.retry_config()
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        if let Some(custom_models) = &self.custom_models {
            if self.dynamic_models == Some(false) {
                return Ok(custom_models
                    .iter()
                    .map(|model| model.name.clone())
                    .collect());
            }

            match self.get_or_fetch_model_names().await {
                Ok(models) => return Ok(models),
                Err(e) if e.is_endpoint_not_found() => {
                    tracing::debug!(
                        "Ollama api/tags not available for provider '{}', using static model list",
                        self.inner.get_name(),
                    );
                    return Ok(custom_models
                        .iter()
                        .map(|model| model.name.clone())
                        .collect());
                }
                Err(e) => return Err(e),
            }
        }

        self.get_or_fetch_model_names().await
    }

    async fn get_context_limit(&self, model: &str, override_limit: Option<usize>) -> usize {
        let configured_limits = self
            .custom_models
            .iter()
            .flatten()
            .filter_map(|model| model.context_limit.map(|limit| (model.name.clone(), limit)));
        bcaip_provider_types::context_limit::ContextLimitResolver::new(self.get_name())
            .with_configured_limits(configured_limits)
            .resolve(model, override_limit, || async {
                if let Some(cached) = self
                    .context_limits
                    .lock()
                    .ok()
                    .and_then(|cache| cache.get(model).copied())
                {
                    return Ok(cached);
                }

                let limit = tokio::time::timeout(
                    SHOW_INFO_TIMEOUT,
                    self.fetch_context_limit_from_show(model),
                )
                .await
                .ok()
                .flatten();
                if let Ok(mut cache) = self.context_limits.lock() {
                    cache.insert(model.to_string(), limit);
                }
                Ok(limit)
            })
            .await
    }
}

impl ProviderDescriptor for OllamaCloudProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            OLLAMA_CLOUD_PROVIDER_NAME,
            "Ollama Cloud",
            "Access hosted models on ollama.com via OpenAI-compatible API",
            "qwen3-coder:480b-cloud",
            vec![],
            "https://ollama.com/library",
            vec![ConfigKey::new(
                "ollama_cloud_api_key",
                false,
                true,
                None,
                true,
            )],
        )
    }
}

impl ProviderDef for OllamaCloudProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        _tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(async {
            anyhow::bail!(
                "Ollama Cloud must be configured as a declarative provider. \
                 Run `BCAIP configure` to set it up."
            )
        })
    }
}

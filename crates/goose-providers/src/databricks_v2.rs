use crate::api_client::{ApiClient, AuthMethod, TlsConfig};
use crate::http_status::{read_error_body, read_json_response};
use anyhow::{Result, anyhow};
use async_stream::try_stream;
use async_trait::async_trait;
use bcaip_provider_types::base::{
    ConfigKey, MessageStream, Provider, ProviderDescriptor, ProviderMetadata,
};
use bcaip_provider_types::formats::{
    ANTHROPIC_PROVIDER_NAME, AnthropicFormatOptions, create_request_anthropic,
    create_request_openai, create_responses_request, extract_reasoning_effort,
    is_openai_responses_model, response_to_streaming_message_anthropic,
};
use bcaip_provider_types::images::ImageFormat;
use futures::TryStreamExt;
use serde::Serialize;
use serde_json::Value;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use std::{collections::HashSet, io};
use tokio::pin;
use tokio_util::io::StreamReader;
const DEFAULT_PROVIDER_TIMEOUT_SECS: u64 = 600;
use crate::databricks_auth::{
    DatabricksAuth, DatabricksAuthProvider, DatabricksOauthTokenProvider, DatabricksRefreshHook,
    DatabricksTokenResolver,
};
use crate::openai_compatible::{handle_status, stream_openai_compat, stream_responses_compat};
use bcaip_provider_types::conversations::Message;
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::model::ModelConfig;
use bcaip_provider_types::request_log::{LoggerHandleExt, start_log};
use bcaip_provider_types::retry::ProviderRetry;
use bcaip_provider_types::retry::{
    DEFAULT_BACKOFF_MULTIPLIER, DEFAULT_INITIAL_RETRY_INTERVAL_MS, DEFAULT_MAX_RETRIES,
    DEFAULT_MAX_RETRY_INTERVAL_MS, RetryConfig,
};
use bcaip_provider_types::thinking::ThinkingEffort;
use rmcp::model::Tool;
const DATABRICKS_V2_PROVIDER_NAME: &str = "databricks_v2";
const DATABRICKS_V2_DEFAULT_GATEWAY_PATH: &str = "ai-gateway";
const DATABRICKS_V2_ROUTE_SUFFIXES: [&str; 3] = [
    "openai/v1/responses",
    "anthropic/v1/messages",
    "mlflow/v1/chat/completions",
];
const DATABRICKS_V2_LIST_ENDPOINTS_PATH: &str = "api/ai-gateway/v2/endpoints";
const DATABRICKS_V2_LIST_MODEL_SERVICES_PATH: &str = "api/2.1/unity-catalog/model-services";
const DATABRICKS_V2_MODEL_SERVICE_PREFIX: &str = "model-services/";
const DATABRICKS_V2_CATALOG_PAGE_SIZE: usize = 100;
const DATABRICKS_V2_MAX_CATALOG_PAGES: usize = 100;
// Model-services intermittently uses 499 for transient gateway timeouts.
const DATABRICKS_V2_TRANSIENT_GATEWAY_STATUS: u16 = 499;

#[derive(Clone, Copy)]
struct ModelCatalog {
    path: &'static str,
    items_key: &'static str,
    name_prefix: Option<&'static str>,
    view: Option<&'static str>,
    label: &'static str,
}

const DATABRICKS_V2_ENDPOINTS_CATALOG: ModelCatalog = ModelCatalog {
    path: DATABRICKS_V2_LIST_ENDPOINTS_PATH,
    items_key: "endpoints",
    name_prefix: None,
    view: None,
    label: "AI Gateway endpoints",
};

// LIST can include metadata-only services but omits caller-effective grants.
// Inference enforces EXECUTE.
const DATABRICKS_V2_MODEL_SERVICES_CATALOG: ModelCatalog = ModelCatalog {
    path: DATABRICKS_V2_LIST_MODEL_SERVICES_PATH,
    items_key: "model_services",
    name_prefix: Some(DATABRICKS_V2_MODEL_SERVICE_PREFIX),
    view: Some("FULL"),
    label: "model services",
};

pub const DATABRICKS_V2_DEFAULT_MODEL: &str = "databricks-gpt-5-5";
pub const DATABRICKS_V2_KNOWN_MODELS: &[&str] =
    &["databricks-gpt-5-5", "databricks-claude-opus-4-7"];

pub const DATABRICKS_V2_DOC_URL: &str = "https://docs.databricks.com/en/generative-ai/ai-gateway/";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DatabricksV2Route {
    OpenAiResponses,
    AnthropicMessages,
    MlflowChatCompletions,
}

#[derive(Serialize)]
pub struct DatabricksV2Provider {
    #[serde(skip)]
    api_client: ApiClient,
    #[serde(skip)]
    retry_config: RetryConfig,
    #[serde(skip)]
    name: String,
    #[serde(skip)]
    token_cache: Arc<Mutex<Option<String>>>,
    #[serde(skip)]
    refresh_hook: Option<DatabricksRefreshHook>,
    #[serde(skip)]
    gateway_path: String,
}

impl DatabricksV2Provider {
    pub async fn cleanup() -> Result<()> {
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new(
        host: String,
        auth: DatabricksAuth,
        retry_config: RetryConfig,
        tls_config: Option<TlsConfig>,
        oauth_token_provider: Option<DatabricksOauthTokenProvider>,
        token_resolver: Option<DatabricksTokenResolver>,
        request_builder: Option<crate::api_client::RequestBuilderDecorator>,
        refresh_hook: Option<DatabricksRefreshHook>,
    ) -> Result<Self> {
        let token_cache = Arc::new(Mutex::new(match &auth {
            DatabricksAuth::Token(t) => Some(t.clone()),
            _ => None,
        }));

        let auth_method = AuthMethod::Custom(Box::new(DatabricksAuthProvider {
            auth: auth.clone(),
            token_cache: token_cache.clone(),
            oauth_token_provider,
            token_resolver,
        }));

        let mut api_client = ApiClient::with_timeout_and_tls(
            host,
            auth_method,
            Duration::from_secs(DEFAULT_PROVIDER_TIMEOUT_SECS),
            tls_config,
        )?;
        if let Some(request_builder) = request_builder {
            api_client = api_client.with_request_builder(request_builder);
        }

        Ok(Self {
            api_client,
            retry_config,
            name: DATABRICKS_V2_PROVIDER_NAME.to_string(),
            token_cache,
            refresh_hook,
            gateway_path: DATABRICKS_V2_DEFAULT_GATEWAY_PATH.to_string(),
        })
    }

    /// Routes requests through a gateway deployment served under a different
    /// base path. Accepts either the base path (`my-gateway`) or a full route
    /// (`my-gateway/openai/v1/responses`), since callers configure the latter.
    pub fn with_gateway_path(mut self, gateway_path: &str) -> Result<Self> {
        let trimmed = gateway_path.trim().trim_matches('/');
        if trimmed.is_empty() {
            return Err(anyhow!(
                "Databricks gateway path must not be empty; omit it to use the default `{DATABRICKS_V2_DEFAULT_GATEWAY_PATH}`"
            ));
        }
        if trimmed.contains("://") {
            return Err(anyhow!(
                "Databricks gateway path must be a path such as `{DATABRICKS_V2_DEFAULT_GATEWAY_PATH}`, not a URL; configure the workspace URL as the provider host instead"
            ));
        }

        self.gateway_path = DATABRICKS_V2_ROUTE_SUFFIXES
            .iter()
            .find_map(|suffix| trimmed.strip_suffix(suffix))
            .map(|base| base.trim_matches('/'))
            .unwrap_or(trimmed)
            .to_string();

        Ok(self)
    }

    fn route_path(&self, route: DatabricksV2Route) -> String {
        let suffix = match route {
            DatabricksV2Route::OpenAiResponses => "openai/v1/responses",
            DatabricksV2Route::AnthropicMessages => "anthropic/v1/messages",
            DatabricksV2Route::MlflowChatCompletions => "mlflow/v1/chat/completions",
        };
        format!("{}/{suffix}", self.gateway_path)
    }

    pub fn load_retry_config(get_param: impl Fn(&str) -> Option<String>) -> RetryConfig {
        let max_retries = get_param("DATABRICKS_MAX_RETRIES")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_RETRIES);

        let initial_interval_ms = get_param("DATABRICKS_INITIAL_RETRY_INTERVAL_MS")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_INITIAL_RETRY_INTERVAL_MS);

        let backoff_multiplier = get_param("DATABRICKS_BACKOFF_MULTIPLIER")
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(DEFAULT_BACKOFF_MULTIPLIER);

        let max_interval_ms = get_param("DATABRICKS_MAX_RETRY_INTERVAL_MS")
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_MAX_RETRY_INTERVAL_MS);

        RetryConfig::new(
            max_retries,
            initial_interval_ms,
            backoff_multiplier,
            max_interval_ms,
        )
    }

    fn route_for_model(model_name: &str) -> DatabricksV2Route {
        let is_model_service = Self::is_model_service_fqn(model_name);
        let routing_name = if is_model_service {
            model_name.rsplit('.').next().unwrap_or(model_name)
        } else {
            model_name
        };
        let (clean_name, _) = extract_reasoning_effort(routing_name);
        let lower = clean_name.to_lowercase();

        // Claude model services also serve `anthropic/v1/messages`; the MLflow
        // chat route drops the prompt-cache breakpoints Anthropic needs.
        if is_openai_responses_model(&clean_name) || Self::looks_like_gpt5(&lower) {
            DatabricksV2Route::OpenAiResponses
        } else if Self::is_claude_model(&lower) {
            DatabricksV2Route::AnthropicMessages
        } else {
            DatabricksV2Route::MlflowChatCompletions
        }
    }

    fn is_model_service_fqn(model_name: &str) -> bool {
        let Some((catalog, remainder)) = model_name.split_once('.') else {
            return false;
        };
        let Some((schema, service)) = remainder.split_once('.') else {
            return false;
        };
        !catalog.is_empty() && !schema.is_empty() && !service.is_empty()
    }

    fn looks_like_gpt5(model_name: &str) -> bool {
        model_name.contains("gpt-5") || model_name.contains("gpt5")
    }

    fn is_claude_model(model_name: &str) -> bool {
        model_name.contains("claude")
    }

    fn always_on_reasoning_effort(model_config: &ModelConfig) -> Option<&'static str> {
        if !model_config.is_reasoning_model()
            || !(model_config.is_glm_5_3_reasoning_model()
                || model_config.is_kimi_k3_reasoning_model())
        {
            return None;
        }

        Some(match model_config.thinking_effort() {
            Some(ThinkingEffort::Off | ThinkingEffort::Low) => "low",
            Some(ThinkingEffort::Medium | ThinkingEffort::High) => "high",
            Some(ThinkingEffort::Max) | None => "max",
        })
    }

    fn name_looks_chat_capable(name: &str) -> bool {
        if name.to_ascii_lowercase().contains("embedding") {
            return false;
        }
        !name
            .split(|c: char| !c.is_ascii_alphanumeric())
            .any(|segment| {
                segment.eq_ignore_ascii_case("bge") || segment.eq_ignore_ascii_case("gte")
            })
    }

    fn model_service_supports_chat(item: &Value, fallback_name: &str) -> bool {
        let Some(api_types) = item.get("supported_api_types") else {
            // Older workspaces omit capabilities; only the service leaf is safe to inspect.
            return Self::name_looks_chat_capable(fallback_name);
        };
        let Some(api_types) = api_types.as_array() else {
            return false;
        };

        api_types.iter().filter_map(Value::as_str).any(|api_type| {
            api_type.eq_ignore_ascii_case("chat")
                || api_type.eq_ignore_ascii_case("mlflow/v1/chat/completions")
        })
    }

    fn parse_catalog_page(
        json: &Value,
        catalog: &ModelCatalog,
    ) -> Result<(Vec<String>, Option<String>), ProviderError> {
        let items = json
            .get(catalog.items_key)
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ProviderError::RequestFailed(format!(
                    "Unexpected response format from Databricks {} API",
                    catalog.label
                ))
            })?;

        let models: Vec<String> = items
            .iter()
            .filter_map(|item| {
                let name = item.get("name").and_then(Value::as_str)?;
                let (name, is_chat_capable) = match catalog.name_prefix {
                    Some(prefix) => {
                        let name = name.strip_prefix(prefix)?;
                        let leaf = name.rsplit('.').next().unwrap_or(name);
                        (name, Self::model_service_supports_chat(item, leaf))
                    }
                    None => (name, Self::name_looks_chat_capable(name)),
                };
                (!name.is_empty() && is_chat_capable).then(|| name.to_string())
            })
            .collect();

        let next_page_token = json
            .get("next_page_token")
            .and_then(|v| v.as_str())
            .filter(|token| !token.is_empty())
            .map(str::to_string);

        Ok((models, next_page_token))
    }

    async fn stream_openai_responses(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let mut payload = create_responses_request(model_config, system, messages, tools)?;
        payload["stream"] = Value::Bool(true);
        let mut log = start_log(model_config, &payload)?;

        let response = self
            .with_retry(|| async {
                let resp = self
                    .api_client
                    .request(&self.route_path(DatabricksV2Route::OpenAiResponses))
                    .model_headers(model_config)?
                    .streaming(true)
                    .response_post(&payload)
                    .await?;
                handle_status(resp).await
            })
            .await
            .inspect_err(|e| {
                let _ = log.error(e);
            })?;

        stream_responses_compat(response, log)
    }

    async fn stream_mlflow_chat_completions(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let is_model_service = Self::is_model_service_fqn(&model_config.model_name);
        let mut format_config = model_config.clone();
        if is_model_service {
            // Keep UC namespace text out of OpenAI format heuristics.
            format_config.model_name = "model-service".to_string();
        }
        let mut payload = create_request_openai(
            &format_config,
            system,
            messages,
            tools,
            &ImageFormat::OpenAi,
            true,
        )?;
        if is_model_service {
            payload["model"] = Value::String(model_config.model_name.clone());
        }
        if let Some(effort) = Self::always_on_reasoning_effort(model_config) {
            payload["reasoning_effort"] = Value::String(effort.to_string());
        }
        if payload.get("max_tokens").is_none() {
            payload["max_tokens"] = Value::from(model_config.max_output_tokens());
        }
        let mut log = start_log(model_config, &payload)?;

        let response = self
            .with_retry(|| async {
                let resp = self
                    .api_client
                    .request(&self.route_path(DatabricksV2Route::MlflowChatCompletions))
                    .model_headers(model_config)?
                    .streaming(true)
                    .response_post(&payload)
                    .await?;
                handle_status(resp).await
            })
            .await
            .inspect_err(|e| {
                let _ = log.error(e);
            })?;

        stream_openai_compat(response, log)
    }

    async fn stream_anthropic_messages(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let mut payload = create_request_anthropic(
            ANTHROPIC_PROVIDER_NAME,
            model_config,
            system,
            messages,
            tools,
            AnthropicFormatOptions::default(),
        )?;
        payload["stream"] = Value::Bool(true);
        let mut log = start_log(model_config, &payload)?;

        let response = self
            .with_retry(|| async {
                let resp = self
                    .api_client
                    .request(&self.route_path(DatabricksV2Route::AnthropicMessages))
                    .model_headers(model_config)?
                    .streaming(true)
                    .response_post(&payload)
                    .await?;
                handle_status(resp).await
            })
            .await
            .inspect_err(|e| {
                let _ = log.error(e);
            })?;

        let stream = response.bytes_stream().map_err(io::Error::other);

        Ok(Box::pin(try_stream! {
            let stream_reader = StreamReader::new(stream);
            let framed = tokio_util::codec::FramedRead::new(stream_reader, tokio_util::codec::LinesCodec::new())
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
}

impl ProviderDescriptor for DatabricksV2Provider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            DATABRICKS_V2_PROVIDER_NAME,
            "Databricks AI Gateway",
            "Models on Databricks AI Gateway v2",
            DATABRICKS_V2_DEFAULT_MODEL,
            DATABRICKS_V2_KNOWN_MODELS.to_vec(),
            DATABRICKS_V2_DOC_URL,
            vec![
                ConfigKey::new("DATABRICKS_HOST", true, false, None, true),
                ConfigKey::new("DATABRICKS_TOKEN", false, true, None, true),
            ],
        )
    }
}

#[async_trait]
impl Provider for DatabricksV2Provider {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn retry_config(&self) -> RetryConfig {
        self.retry_config.clone()
    }

    async fn refresh_credentials(&self) -> Result<(), ProviderError> {
        if let Some(refresh_hook) = &self.refresh_hook {
            refresh_hook();
        }
        *self.token_cache.lock().unwrap() = None;
        Ok(())
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        match Self::route_for_model(&model_config.model_name) {
            DatabricksV2Route::OpenAiResponses => {
                self.stream_openai_responses(model_config, system, messages, tools)
                    .await
            }
            DatabricksV2Route::AnthropicMessages => {
                self.stream_anthropic_messages(model_config, system, messages, tools)
                    .await
            }
            DatabricksV2Route::MlflowChatCompletions => {
                self.stream_mlflow_chat_completions(model_config, system, messages, tools)
                    .await
            }
        }
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        let (endpoint_result, service_result) = tokio::join!(
            self.fetch_model_catalog(&DATABRICKS_V2_ENDPOINTS_CATALOG),
            self.fetch_model_catalog(&DATABRICKS_V2_MODEL_SERVICES_CATALOG),
        );

        let mut names = Vec::new();
        let mut failures = Vec::new();
        let mut any_catalog_succeeded = false;
        for (catalog, result) in [
            (&DATABRICKS_V2_ENDPOINTS_CATALOG, endpoint_result),
            (&DATABRICKS_V2_MODEL_SERVICES_CATALOG, service_result),
        ] {
            match result {
                Ok(models) => {
                    any_catalog_succeeded = true;
                    names.extend(models);
                }
                Err(error) => failures.push((catalog.label, error)),
            }
        }

        if !any_catalog_succeeded {
            let details = failures
                .into_iter()
                .map(|(_, error)| match error {
                    ProviderError::RequestFailed(message) => message,
                    error => error.to_string(),
                })
                .collect::<Vec<_>>()
                .join("; ");
            return Err(ProviderError::RequestFailed(details));
        }
        for (label, error) in failures {
            tracing::warn!(catalog = label, %error, "Failed to fetch Databricks model catalog");
        }

        names.sort();
        names.dedup();
        Ok(names)
    }
}

impl DatabricksV2Provider {
    async fn fetch_model_catalog(
        &self,
        catalog: &ModelCatalog,
    ) -> Result<Vec<String>, ProviderError> {
        let ModelCatalog { path, label, .. } = catalog;
        let mut models = Vec::new();
        let mut page_token: Option<String> = None;
        let mut seen_page_tokens = HashSet::new();

        for _ in 0..DATABRICKS_V2_MAX_CATALOG_PAGES {
            let mut path_with_query = format!("{path}?page_size={DATABRICKS_V2_CATALOG_PAGE_SIZE}");
            if let Some(view) = catalog.view {
                path_with_query.push_str(&format!("&view={}", urlencoding::encode(view)));
            }
            if let Some(token) = &page_token {
                path_with_query.push_str(&format!("&page_token={}", urlencoding::encode(token)));
            }

            let json: Value = self
                .with_retry_config(
                    || async {
                        let response = self.api_client.response_get(&path_with_query).await?;
                        if response.status().as_u16() == DATABRICKS_V2_TRANSIENT_GATEWAY_STATUS {
                            let detail = read_error_body(response).await.unwrap_or_default();
                            return Err(ProviderError::ServerError(format!(
                                "Databricks {label} returned {DATABRICKS_V2_TRANSIENT_GATEWAY_STATUS}: {detail}"
                            )));
                        }
                        read_json_response(handle_status(response).await?).await
                    },
                    self.retry_config.clone().transient_only(),
                )
                .await
                .map_err(|error| {
                    ProviderError::RequestFailed(format!(
                        "Failed to fetch Databricks {label}: {error}"
                    ))
                })?;

            let (page_models, next_page_token) = Self::parse_catalog_page(&json, catalog)?;
            models.extend(page_models);

            let Some(next_page_token) = next_page_token else {
                return Ok(models);
            };
            if !seen_page_tokens.insert(next_page_token.clone()) {
                return Err(ProviderError::RequestFailed(format!(
                    "Databricks {label} returned a repeated page token"
                )));
            }
            page_token = Some(next_page_token);
        }

        Err(ProviderError::RequestFailed(format!(
            "Databricks {label} pagination exceeded {DATABRICKS_V2_MAX_CATALOG_PAGES} pages"
        )))
    }
}

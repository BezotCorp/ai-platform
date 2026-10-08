//! In-process uniffi bindings for the GDK.
//!
//! This is the API surface exposed to Python and Kotlin. It focuses on native
//! Bcaip providers and mirrors the provider message/tool/streaming model closely
//! enough for Kotlin agent frameworks to avoid JSON-only shims for common paths.

use std::{collections::HashMap, future::Future, sync::Arc, sync::OnceLock, time::Duration};

use crate::observability::{RequestDescriptor, RequestObserver, RequestOperation};
use base64::Engine as _;
use bcaip_provider_types::base::Provider as BcaipProvider;
use bcaip_provider_types::base::{MessageStream, ModelInfo};
use bcaip_provider_types::conversations::MessageContent as BcaipMessageContent;
use bcaip_provider_types::conversations::{Message, ProviderUsage};
use bcaip_provider_types::document_format::{
    SUPPORTED_DOCUMENT_MEDIA_TYPES, document_media_type_is_supported,
};
use bcaip_provider_types::model::ModelConfig;
use bcaip_provider_types::utils::sanitize_unicode_tags;
use bcaip_providers::anthropic::AnthropicProviderBuilder;
use bcaip_providers::api_client::{ApiClient, AuthMethod};
use bcaip_providers::databricks::DatabricksProvider as BcaipDatabricksProvider;
use bcaip_providers::databricks_auth::DatabricksAuth;
use bcaip_providers::databricks_v2::DatabricksV2Provider as BcaipDatabricksV2Provider;
use bcaip_providers::decision::DecisionAnswer as BcaipDecisionAnswer;
use bcaip_providers::decision::DecisionProvider as BcaipDecisionProvider;
use bcaip_providers::decision::DecisionQuestion as BcaipDecisionQuestion;
use bcaip_providers::decision::DecisionRequest as BcaipDecisionRequest;
use bcaip_providers::decision::NoulCriteria as BcaipNoulCriteria;
use bcaip_providers::declarative::{DeclarativeProviderConfig, EnvKeyResolver};
use bcaip_providers::openai::{
    OPEN_AI_DEFAULT_BASE_PATH, OPEN_AI_VERSIONLESS_BASE_PATH, OpenAiProviderBuilder,
    parse_openai_base_url,
};
use futures::StreamExt;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, ErrorCode, ErrorData, Role, Tool,
};
use serde_json::Value;

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum BcaipError {
    #[error("Rate limit exceeded{retry_after_suffix}")]
    RateLimited {
        retry_after_ms: Option<u64>,
        retry_after_suffix: String,
    },
    #[error("Output token limit exceeded: {details}")]
    OutputTokenLimitExceeded { details: String },
    #[error("Context length exceeded: {details}")]
    ContextLengthExceeded { details: String },
    #[error("Authentication error: {details}")]
    Authentication { details: String },
    #[error("Timeout: {details}")]
    Timeout { details: String },
    #[error("Provider unavailable: {details}")]
    ProviderUnavailable { details: String },
    #[error("{details}")]
    Generic { details: String },
}

impl BcaipError {
    fn generic(error: impl ToString) -> Self {
        Self::Generic {
            details: error.to_string(),
        }
    }
}

impl From<anyhow::Error> for BcaipError {
    fn from(error: anyhow::Error) -> Self {
        Self::generic(error)
    }
}

impl From<bcaip_provider_types::errors::ProviderError> for BcaipError {
    fn from(error: bcaip_provider_types::errors::ProviderError) -> Self {
        match error {
            bcaip_provider_types::errors::ProviderError::Authentication(message) => {
                Self::Authentication { details: message }
            }
            bcaip_provider_types::errors::ProviderError::ContextLengthExceeded(message) => {
                Self::ContextLengthExceeded { details: message }
            }
            bcaip_provider_types::errors::ProviderError::RateLimitExceeded {
                retry_delay, ..
            } => {
                let retry_after_ms = retry_delay.map(|delay| delay.as_millis() as u64);
                let retry_after_suffix = retry_after_ms
                    .map(|ms| format!("; retry after {ms}ms"))
                    .unwrap_or_default();
                Self::RateLimited {
                    retry_after_ms,
                    retry_after_suffix,
                }
            }
            bcaip_provider_types::errors::ProviderError::ServerError(message)
            | bcaip_provider_types::errors::ProviderError::EndpointNotFound(message)
            | bcaip_provider_types::errors::ProviderError::CreditsExhausted {
                details: message,
                ..
            } => Self::ProviderUnavailable { details: message },
            bcaip_provider_types::errors::ProviderError::NetworkError(message)
                if is_timeout(&message) =>
            {
                Self::Timeout { details: message }
            }
            bcaip_provider_types::errors::ProviderError::RequestFailed(message)
                if is_timeout(&message) =>
            {
                Self::Timeout { details: message }
            }
            bcaip_provider_types::errors::ProviderError::ExecutionError(message)
                if is_output_token_limit(&message) =>
            {
                Self::OutputTokenLimitExceeded { details: message }
            }
            other => Self::generic(other),
        }
    }
}

impl From<serde_json::Error> for BcaipError {
    fn from(error: serde_json::Error) -> Self {
        Self::generic(error)
    }
}

fn is_timeout(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("timed out") || message.contains("timeout")
}

fn is_output_token_limit(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("output token")
        || message.contains("max_tokens")
        || message.contains("max tokens")
}

/// Receives provider request logs as JSONL records.
///
/// `start` returns an identifier that is passed to `write` for every record in
/// that request, allowing callers to keep concurrent request logs separate.
#[uniffi::export(callback_interface)]
pub trait RequestLogger: Send + Sync {
    fn start(&self) -> Result<u64, BcaipError>;
    fn write(&self, request_id: u64, record: String) -> Result<(), BcaipError>;
}

struct RequestLoggerAdapter {
    logger: Arc<dyn RequestLogger>,
}

impl bcaip_provider_types::request_log::RequestLogger for RequestLoggerAdapter {
    fn start(
        &self,
    ) -> Result<
        Box<dyn bcaip_provider_types::request_log::RequestLogHandle>,
        Box<dyn std::error::Error + Send + Sync>,
    > {
        Ok(Box::new(RequestLogHandleAdapter {
            request_id: self.logger.start()?,
            logger: Arc::clone(&self.logger),
        }))
    }
}

struct RequestLogHandleAdapter {
    request_id: u64,
    logger: Arc<dyn RequestLogger>,
}

impl bcaip_provider_types::request_log::RequestLogHandle for RequestLogHandleAdapter {
    fn write(&mut self, record: &str) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        self.logger.write(self.request_id, record.to_string())?;
        Ok(())
    }
}

/// Installs the process-wide provider request logger.
///
/// A logger can only be installed once for the lifetime of the process.
#[uniffi::export]
pub fn install_request_logger(logger: Box<dyn RequestLogger>) -> Result<(), BcaipError> {
    bcaip_provider_types::request_log::install_logger(RequestLoggerAdapter {
        logger: Arc::from(logger),
    })
    .map_err(|error| BcaipError::generic(error.to_string()))
}

/// A text message passed to a provider.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ProviderMessage {
    pub role: MessageRole,
    pub content: Vec<MessageContent>,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum MessageContent {
    Text {
        text: String,
    },
    Image {
        mime_type: String,
        data: Vec<u8>,
    },
    Document {
        mime_type: String,
        data: Vec<u8>,
        name: Option<String>,
    },
    ToolRequest {
        id: String,
        name: String,
        arguments_json: String,
        #[uniffi(default = None)]
        provider_metadata_json: Option<String>,
        #[uniffi(default = None)]
        tool_error_json: Option<String>,
    },
    ToolResult {
        id: String,
        success: bool,
        content_json: String,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    RedactedThinking {
        data: String,
    },
}

impl ProviderMessage {
    fn to_bcaip_message(&self) -> Result<Option<Message>, BcaipError> {
        let role = match self.role {
            MessageRole::User | MessageRole::Tool => Role::User,
            MessageRole::Assistant => Role::Assistant,
        };
        let mut message = Message::new(role, chrono_now(), Vec::new());
        for content in &self.content {
            message = message.with_content(content.to_bcaip_content()?);
        }
        Ok(Some(message))
    }
}

impl MessageContent {
    fn to_bcaip_content(&self) -> Result<BcaipMessageContent, BcaipError> {
        match self {
            MessageContent::Text { text } => {
                Ok(BcaipMessageContent::text(sanitize_unicode_tags(text)))
            }
            MessageContent::Image { mime_type, data } => Ok(BcaipMessageContent::image(
                base64::engine::general_purpose::STANDARD.encode(data),
                mime_type.clone(),
            )),
            MessageContent::Document {
                mime_type,
                data,
                name,
            } => {
                if !document_media_type_is_supported(mime_type) {
                    return Err(BcaipError::generic(format!(
                        "unsupported document media type {mime_type}: supported types are {}",
                        SUPPORTED_DOCUMENT_MEDIA_TYPES.join(", ")
                    )));
                }
                Ok(BcaipMessageContent::document(
                    base64::engine::general_purpose::STANDARD.encode(data),
                    mime_type.clone(),
                    name.clone(),
                ))
            }
            MessageContent::ToolRequest {
                id,
                name,
                arguments_json,
                provider_metadata_json,
                tool_error_json,
            } => {
                let metadata = provider_metadata_json
                    .as_deref()
                    .map(parse_json_object)
                    .transpose()?;
                let tool_call = match tool_error_json {
                    Some(error_json) => Err(serde_json::from_str(error_json)?),
                    None => {
                        let arguments = parse_json_object(arguments_json)?;
                        Ok(CallToolRequestParams::new(name.clone()).with_arguments(arguments))
                    }
                };
                Ok(BcaipMessageContent::tool_request_with_metadata(
                    id.clone(),
                    tool_call,
                    metadata.as_ref(),
                ))
            }
            MessageContent::ToolResult {
                id,
                success,
                content_json,
            } => {
                let value: Value = serde_json::from_str(content_json)?;
                let tool_result = if *success {
                    Ok(call_tool_result(value, false))
                } else {
                    Err(ErrorData::new(
                        ErrorCode::INTERNAL_ERROR,
                        value.to_string(),
                        None,
                    ))
                };
                Ok(BcaipMessageContent::tool_response(id.clone(), tool_result))
            }
            MessageContent::Thinking {
                thinking,
                signature,
            } => Ok(BcaipMessageContent::thinking(
                thinking.clone(),
                signature.clone(),
            )),
            MessageContent::RedactedThinking { data } => {
                Ok(BcaipMessageContent::redacted_thinking(data.clone()))
            }
        }
    }

    /// Maps provider output back onto the binding surface so callers can replay
    /// an assistant turn without reparsing `message_json`. Thinking signatures
    /// and redacted payloads are carried through verbatim: providers reject
    /// replayed thinking blocks whose signature was dropped or altered.
    fn from_bcaip_content(content: &BcaipMessageContent) -> Option<Self> {
        match content {
            BcaipMessageContent::Text(text) => Some(MessageContent::Text {
                text: text.text.clone(),
            }),
            BcaipMessageContent::Image(image) => Some(MessageContent::Image {
                mime_type: image.mime_type.clone(),
                data: base64::engine::general_purpose::STANDARD
                    .decode(&image.data)
                    .ok()?,
            }),
            BcaipMessageContent::ToolRequest(request) => {
                let provider_metadata_json = request
                    .metadata
                    .as_ref()
                    .and_then(|metadata| serde_json::to_string(metadata).ok());
                match &request.tool_call {
                    Ok(tool_call) => Some(MessageContent::ToolRequest {
                        id: request.id.clone(),
                        name: tool_call.name.to_string(),
                        arguments_json: serde_json::to_string(
                            &tool_call.arguments.clone().unwrap_or_default(),
                        )
                        .ok()?,
                        provider_metadata_json,
                        tool_error_json: None,
                    }),
                    Err(error) => Some(MessageContent::ToolRequest {
                        id: request.id.clone(),
                        name: String::new(),
                        arguments_json: "{}".to_string(),
                        provider_metadata_json,
                        tool_error_json: Some(serde_json::to_string(error).ok()?),
                    }),
                }
            }
            BcaipMessageContent::Thinking(thinking) => Some(MessageContent::Thinking {
                thinking: thinking.thinking.clone(),
                signature: thinking.signature.clone(),
            }),
            BcaipMessageContent::RedactedThinking(redacted) => {
                Some(MessageContent::RedactedThinking {
                    data: redacted.data.clone(),
                })
            }
            _ => None,
        }
    }
}

fn chrono_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() as i64)
        .unwrap_or_default()
}

fn call_tool_result(value: Value, is_error: bool) -> CallToolResult {
    let content = match value {
        Value::Array(items) => items.into_iter().map(value_to_content).collect(),
        other => vec![value_to_content(other)],
    };

    if is_error {
        CallToolResult::error(content)
    } else {
        CallToolResult::success(content)
    }
}

fn value_to_content(value: Value) -> ContentBlock {
    match value {
        Value::String(text) => ContentBlock::text(text),
        Value::Object(object) => match object.get("type").and_then(|value| value.as_str()) {
            Some("text") => object
                .get("text")
                .and_then(|value| value.as_str().map(str::to_owned))
                .map(ContentBlock::text)
                .unwrap_or_else(|| ContentBlock::text(Value::Object(object).to_string())),
            Some("image") => {
                let mime_type = object
                    .get("mimeType")
                    .or_else(|| object.get("mime_type"))
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_else(|| "image/png".to_string());
                let data = object
                    .get("data")
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default();
                ContentBlock::image(data, mime_type)
            }
            _ => ContentBlock::text(Value::Object(object).to_string()),
        },
        other => ContentBlock::text(other.to_string()),
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProviderTool {
    pub name: String,
    pub description: String,
    pub input_schema_json: String,
    #[uniffi(default = None)]
    pub annotations_json: Option<String>,
}

impl ProviderTool {
    fn to_bcaip_tool(&self) -> Result<Tool, BcaipError> {
        let schema = parse_json_object(&self.input_schema_json)?;
        let mut tool = Tool::new(self.name.clone(), self.description.clone(), schema);
        if let Some(annotations_json) = &self.annotations_json {
            tool.annotations = Some(serde_json::from_str(annotations_json)?);
        }
        Ok(tool)
    }
}

fn parse_json_object(json: &str) -> Result<serde_json::Map<String, Value>, BcaipError> {
    match serde_json::from_str(json)? {
        Value::Object(object) => Ok(object),
        _ => Err(BcaipError::generic("expected a JSON object")),
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProviderModelConfig {
    pub model_name: String,
    #[uniffi(default = None)]
    pub context_limit: Option<i32>,
    #[uniffi(default = None)]
    pub temperature: Option<f32>,
    #[uniffi(default = None)]
    pub max_tokens: Option<i32>,
    #[uniffi(default = false)]
    pub toolshim: bool,
    #[uniffi(default = None)]
    pub toolshim_model: Option<String>,
    #[uniffi(default = None)]
    pub request_params_json: Option<String>,
    #[uniffi(default = None)]
    pub provider_params_json: Option<String>,
    #[uniffi(default = None)]
    pub reasoning: Option<bool>,
    #[uniffi(default = None)]
    pub timeout_ms: Option<u64>,
    /// Per-request HTTP headers attached to the outgoing provider call.
    /// These override any static headers configured on the provider.
    #[uniffi(default = None)]
    pub request_headers: Option<HashMap<String, String>>,
}

/// A model advertised by the provider. Providers without model discovery
/// return an empty list rather than an error.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ProviderModelInfo {
    pub name: String,
    pub context_limit: Option<u64>,
    pub supports_reasoning: bool,
}

impl From<ModelInfo> for ProviderModelInfo {
    fn from(info: ModelInfo) -> Self {
        Self {
            name: info.name,
            context_limit: info.context_limit.map(|limit| limit as u64),
            supports_reasoning: info.reasoning,
        }
    }
}

impl ProviderModelConfig {
    fn to_bcaip_model_config(&self, provider_name: &str) -> Result<ModelConfig, BcaipError> {
        let mut config = ModelConfig::new(&self.model_name)
            .with_canonical_vision_support(provider_name)
            .with_temperature(self.temperature)
            .with_max_tokens(self.max_tokens)
            .with_toolshim(self.toolshim)
            .with_toolshim_model(self.toolshim_model.clone());

        let mut request_params = serde_json::Map::new();
        merge_params(&mut request_params, self.request_params_json.as_ref())?;
        merge_params(&mut request_params, self.provider_params_json.as_ref())?;
        if !request_params.is_empty() {
            config = config.with_merged_request_params(request_params.into_iter().collect());
        }

        config = config.with_request_headers(self.request_headers.clone());
        config.reasoning = self.reasoning;
        Ok(config)
    }
}

fn merge_params(
    target: &mut serde_json::Map<String, Value>,
    params_json: Option<&String>,
) -> Result<(), BcaipError> {
    if let Some(params_json) = params_json {
        for (key, value) in parse_json_object(params_json)? {
            target.insert(key, value);
        }
    }
    Ok(())
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct Usage {
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
    pub cache_read_input_tokens: Option<i32>,
    pub cache_creation_input_tokens: Option<i32>,
    pub reasoning_tokens: Option<i32>,
    pub model: String,
    pub provider_metadata_json: Option<String>,
    /// Provider-specific response fields as a JSON object, present only when the
    /// provider reported fields with no canonical `Usage` equivalent.
    pub additional_data_json: Option<String>,
}

impl Usage {
    fn from_provider_usage(usage: &ProviderUsage) -> Result<Self, BcaipError> {
        Ok(Self {
            input_tokens: usage.usage.input_tokens,
            output_tokens: usage.usage.output_tokens,
            total_tokens: usage.usage.total_tokens,
            cache_read_input_tokens: usage.usage.cache_read_input_tokens,
            cache_creation_input_tokens: usage.usage.cache_write_input_tokens,
            reasoning_tokens: None,
            model: usage.model.clone(),
            provider_metadata_json: Some(serde_json::to_string(usage)?),
            additional_data_json: usage
                .additional_data
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?,
        })
    }
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum StreamChunk {
    TextChunk {
        text: String,
    },
    ToolChunk {
        id: String,
        name: String,
        arguments_json: String,
        index: Option<i32>,
        #[uniffi(default = None)]
        provider_metadata_json: Option<String>,
    },
    ThinkingChunk {
        thinking: String,
        signature: String,
    },
    RedactedThinkingChunk {
        data: String,
    },
    EndChunk {
        usage: Option<Usage>,
    },
    ErrorChunk {
        error: BcaipStreamError,
    },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct BcaipStreamError {
    pub kind: BcaipStreamErrorKind,
    pub message: String,
    pub retry_after_ms: Option<u64>,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum BcaipStreamErrorKind {
    RateLimited,
    OutputTokenLimitExceeded,
    ContextLengthExceeded,
    Authentication,
    Timeout,
    ProviderUnavailable,
    Generic,
}

impl From<&BcaipError> for BcaipStreamError {
    fn from(error: &BcaipError) -> Self {
        match error {
            BcaipError::RateLimited {
                retry_after_ms,
                retry_after_suffix,
            } => Self {
                kind: BcaipStreamErrorKind::RateLimited,
                message: format!("Rate limit exceeded{retry_after_suffix}"),
                retry_after_ms: *retry_after_ms,
            },
            BcaipError::OutputTokenLimitExceeded { details } => Self {
                kind: BcaipStreamErrorKind::OutputTokenLimitExceeded,
                message: details.clone(),
                retry_after_ms: None,
            },
            BcaipError::ContextLengthExceeded { details } => Self {
                kind: BcaipStreamErrorKind::ContextLengthExceeded,
                message: details.clone(),
                retry_after_ms: None,
            },
            BcaipError::Authentication { details } => Self {
                kind: BcaipStreamErrorKind::Authentication,
                message: details.clone(),
                retry_after_ms: None,
            },
            BcaipError::Timeout { details } => Self {
                kind: BcaipStreamErrorKind::Timeout,
                message: details.clone(),
                retry_after_ms: None,
            },
            BcaipError::ProviderUnavailable { details } => Self {
                kind: BcaipStreamErrorKind::ProviderUnavailable,
                message: details.clone(),
                retry_after_ms: None,
            },
            BcaipError::Generic { details } => Self {
                kind: BcaipStreamErrorKind::Generic,
                message: details.clone(),
                retry_after_ms: None,
            },
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct ProviderCompletion {
    pub message_json: String,
    /// The assistant turn as binding types, ready to append to history and
    /// replay on the next request without reparsing `message_json`.
    pub content: Vec<MessageContent>,
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum Feature {
    Tools,
    Streaming,
    Images,
    Documents,
    JsonSchema,
    Reasoning,
}

static RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

fn runtime() -> Result<&'static tokio::runtime::Runtime, BcaipError> {
    if let Some(runtime) = RUNTIME.get() {
        return Ok(runtime);
    }

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .map_err(BcaipError::generic)?;

    if let Err(unused) = RUNTIME.set(runtime) {
        // Lost the init race; dropping a runtime inside an async context
        // panics, so shut it down without blocking.
        unused.shutdown_background();
    }
    Ok(RUNTIME.get().expect("runtime was initialized"))
}

struct AbortOnDrop<T> {
    handle: tokio::task::JoinHandle<T>,
}

impl<T> Drop for AbortOnDrop<T> {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

async fn run_on_runtime<T>(
    future: impl Future<Output = T> + Send + 'static,
) -> Result<T, BcaipError>
where
    T: Send + 'static,
{
    let mut task = AbortOnDrop {
        handle: runtime()?.spawn(future),
    };
    (&mut task.handle).await.map_err(|error| {
        if error.is_cancelled() {
            BcaipError::Timeout {
                details: "runtime task was cancelled".to_string(),
            }
        } else {
            BcaipError::generic(error)
        }
    })
}

struct ProviderHandle {
    provider: Arc<dyn BcaipProvider>,
}

impl ProviderHandle {
    fn new(provider: Box<dyn BcaipProvider>) -> Self {
        Self {
            provider: Arc::from(provider),
        }
    }

    fn name(&self) -> String {
        self.provider.get_name().to_string()
    }

    async fn context_limit(&self, model: ProviderModelConfig) -> Result<usize, BcaipError> {
        let normalized_model = ModelConfig::new(&model.model_name);
        let override_limit = model
            .context_limit
            .and_then(|limit| (limit > 0).then_some(limit as usize));
        let provider = Arc::clone(&self.provider);
        run_on_runtime(async move {
            provider
                .get_context_limit(&normalized_model.model_name, override_limit)
                .await
        })
        .await
    }

    async fn list_models(&self) -> Result<Vec<ProviderModelInfo>, BcaipError> {
        let provider = Arc::clone(&self.provider);
        let models =
            run_on_runtime(async move { provider.fetch_supported_model_info().await }).await??;
        Ok(models.into_iter().map(ProviderModelInfo::from).collect())
    }

    async fn stream(
        &self,
        model: ProviderModelConfig,
        system: String,
        messages: Vec<ProviderMessage>,
        tools: Vec<ProviderTool>,
    ) -> Result<Arc<ProviderStream>, BcaipError> {
        let timeout_ms = model.timeout_ms;
        let model = model.to_bcaip_model_config(self.provider.get_name())?;
        let messages = convert_messages(messages)?;
        let tools = convert_tools(tools)?;
        let observer = Arc::new(RequestObserver::start(RequestDescriptor {
            provider: self.provider.get_name(),
            model: &model.model_name,
            operation: RequestOperation::Stream,
            system: &system,
            messages: &messages,
            tools: &tools,
        }));
        let provider = Arc::clone(&self.provider);
        let stream = match run_provider_future(timeout_ms, async move {
            provider.stream(&model, &system, &messages, &tools).await
        })
        .await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => return Err(observer.fail(BcaipError::from(error))),
            Err(error) => return Err(observer.fail(error)),
        };
        observer.response_started();

        Ok(Arc::new(ProviderStream {
            state: Arc::new(tokio::sync::Mutex::new(ProviderStreamState {
                stream,
                pending: Vec::new(),
                final_usage: None,
                ended: false,
            })),
            timeout_ms,
            observer,
        }))
    }

    async fn complete(
        &self,
        model: ProviderModelConfig,
        system: String,
        messages: Vec<ProviderMessage>,
        tools: Vec<ProviderTool>,
    ) -> Result<ProviderCompletion, BcaipError> {
        let timeout_ms = model.timeout_ms;
        let model = model.to_bcaip_model_config(self.provider.get_name())?;
        let messages = convert_messages(messages)?;
        let tools = convert_tools(tools)?;
        let observer = RequestObserver::start(RequestDescriptor {
            provider: self.provider.get_name(),
            model: &model.model_name,
            operation: RequestOperation::Complete,
            system: &system,
            messages: &messages,
            tools: &tools,
        });
        let provider = Arc::clone(&self.provider);
        let (message, usage) = match run_provider_future(timeout_ms, async move {
            provider.complete(&model, &system, &messages, &tools).await
        })
        .await
        {
            Ok(Ok(completion)) => completion,
            Ok(Err(error)) => return Err(observer.fail(BcaipError::from(error))),
            Err(error) => return Err(observer.fail(error)),
        };
        observer.response_started();

        let completion = ProviderCompletion {
            message_json: serde_json::to_string(&message)?,
            content: message
                .content
                .iter()
                .filter_map(MessageContent::from_bcaip_content)
                .collect(),
            usage: Some(Usage::from_provider_usage(&usage)?),
        };
        observer.succeeded(
            completion.usage.clone(),
            observer
                .captures_payloads()
                .then(|| completion.message_json.clone()),
        );
        Ok(completion)
    }
}

async fn run_provider_future<T>(
    timeout_ms: Option<u64>,
    future: impl Future<Output = T> + Send + 'static,
) -> Result<T, BcaipError>
where
    T: Send + 'static,
{
    run_on_runtime(async move {
        if let Some(timeout_ms) = timeout_ms {
            tokio::time::timeout(Duration::from_millis(timeout_ms), future)
                .await
                .map_err(|_| BcaipError::Timeout {
                    details: format!("request timed out after {timeout_ms}ms"),
                })
        } else {
            Ok(future.await)
        }
    })
    .await?
}

fn convert_messages(messages: Vec<ProviderMessage>) -> Result<Vec<Message>, BcaipError> {
    messages
        .iter()
        .filter_map(|message| message.to_bcaip_message().transpose())
        .collect()
}

fn convert_tools(tools: Vec<ProviderTool>) -> Result<Vec<Tool>, BcaipError> {
    tools.iter().map(ProviderTool::to_bcaip_tool).collect()
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum DecisionQuestion {
    Noul {
        instructions: String,
        criteria: Option<NoulCriteria>,
    },
    Choice {
        instructions: String,
        criteria: HashMap<String, String>,
    },
    Score {
        instructions: String,
        criteria: Vec<String>,
    },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct NoulCriteria {
    pub true_description: String,
    pub false_description: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DecisionRequest {
    pub model: String,
    pub state_json: String,
    pub questions: HashMap<String, DecisionQuestion>,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum DecisionAnswer {
    Noul {
        noul: f64,
    },
    Choice {
        choice: String,
        confidence: f64,
        probabilities: HashMap<String, f64>,
    },
    Score {
        score: f64,
        confidence: f64,
        legend_json: HashMap<String, String>,
        probabilities: HashMap<String, f64>,
    },
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct DecisionResponse {
    pub model: String,
    pub answers: HashMap<String, DecisionAnswer>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cost: Option<f64>,
    pub id: Option<String>,
    pub provider: Option<String>,
}

impl TryFrom<DecisionRequest> for BcaipDecisionRequest {
    type Error = BcaipError;

    fn try_from(value: DecisionRequest) -> Result<Self, Self::Error> {
        Ok(Self {
            model: value.model,
            state: serde_json::from_str(&value.state_json)?,
            questions: value
                .questions
                .into_iter()
                .map(|(name, question)| (name, question.into()))
                .collect(),
        })
    }
}

impl From<DecisionQuestion> for BcaipDecisionQuestion {
    fn from(value: DecisionQuestion) -> Self {
        match value {
            DecisionQuestion::Noul {
                instructions,
                criteria,
            } => Self::Noul {
                instructions,
                criteria: criteria.map(|criteria| BcaipNoulCriteria {
                    true_description: criteria.true_description,
                    false_description: criteria.false_description,
                }),
            },
            DecisionQuestion::Choice {
                instructions,
                criteria,
            } => Self::Choice {
                instructions,
                criteria,
            },
            DecisionQuestion::Score {
                instructions,
                criteria,
            } => Self::Score {
                instructions,
                criteria,
            },
        }
    }
}

impl From<bcaip_providers::decision::DecisionResponse> for DecisionResponse {
    fn from(value: bcaip_providers::decision::DecisionResponse) -> Self {
        Self {
            model: value.model,
            answers: value
                .answers
                .into_iter()
                .map(|(name, answer)| (name, answer.into()))
                .collect(),
            input_tokens: value.usage.input_tokens,
            output_tokens: value.usage.output_tokens,
            cost: value.usage.cost,
            id: value.id,
            provider: value.provider,
        }
    }
}

impl From<BcaipDecisionAnswer> for DecisionAnswer {
    fn from(value: BcaipDecisionAnswer) -> Self {
        match value {
            BcaipDecisionAnswer::Noul { noul } => Self::Noul { noul },
            BcaipDecisionAnswer::Choice {
                choice,
                confidence,
                probabilities,
            } => Self::Choice {
                choice,
                confidence,
                probabilities,
            },
            BcaipDecisionAnswer::Score {
                score,
                confidence,
                legend,
                probabilities,
            } => Self::Score {
                score,
                confidence,
                legend_json: legend
                    .into_iter()
                    .map(|(level, description)| {
                        let description = match description {
                            serde_json::Value::String(text) => text,
                            other => other.to_string(),
                        };
                        (level, description)
                    })
                    .collect(),
                probabilities,
            },
        }
    }
}

#[derive(uniffi::Object)]
pub struct DecisionProvider {
    provider: Arc<dyn BcaipDecisionProvider>,
}

#[uniffi::export]
impl DecisionProvider {
    pub async fn create_decision(
        &self,
        request: DecisionRequest,
    ) -> Result<DecisionResponse, BcaipError> {
        let request = request.try_into()?;
        let provider = Arc::clone(&self.provider);
        let response =
            run_on_runtime(async move { provider.create_decision(&request).await }).await??;
        Ok(response.into())
    }
}

#[derive(uniffi::Object)]
pub struct Provider {
    handle: ProviderHandle,
}

impl Provider {
    fn new(provider: Box<dyn BcaipProvider>) -> Arc<Self> {
        Arc::new(Self {
            handle: ProviderHandle::new(provider),
        })
    }
}

#[uniffi::export]
impl Provider {
    pub fn name(&self) -> String {
        self.handle.name()
    }

    pub fn supported_features(&self) -> Vec<Feature> {
        let name = self.name();
        let mut features = vec![Feature::Streaming, Feature::Tools, Feature::JsonSchema];
        if matches!(
            name.as_str(),
            "openai" | "anthropic" | "databricks" | "databricks_v2" | "groq"
        ) {
            features.push(Feature::Images);
        }
        if matches!(
            name.as_str(),
            "openai" | "anthropic" | "databricks" | "databricks_v2" | "google"
        ) {
            features.push(Feature::Documents);
        }
        if matches!(
            name.as_str(),
            "openai" | "anthropic" | "databricks" | "databricks_v2"
        ) {
            features.push(Feature::Reasoning);
        }
        features
    }

    pub async fn context_limit(&self, model: ProviderModelConfig) -> Result<u64, BcaipError> {
        Ok(self.handle.context_limit(model).await? as u64)
    }

    pub async fn list_models(&self) -> Result<Vec<ProviderModelInfo>, BcaipError> {
        self.handle.list_models().await
    }

    pub async fn stream(
        &self,
        model: ProviderModelConfig,
        system: String,
        messages: Vec<ProviderMessage>,
        tools: Vec<ProviderTool>,
    ) -> Result<Arc<ProviderStream>, BcaipError> {
        self.handle.stream(model, system, messages, tools).await
    }

    pub async fn complete(
        &self,
        model: ProviderModelConfig,
        system: String,
        messages: Vec<ProviderMessage>,
        tools: Vec<ProviderTool>,
    ) -> Result<ProviderCompletion, BcaipError> {
        self.handle.complete(model, system, messages, tools).await
    }

    /// Summarizes a conversation down to a single message so it can continue
    /// past this model's context window.
    pub async fn compact(
        &self,
        model_name: String,
        messages: Vec<CompactionMessage>,
        templates: Option<CompactionTemplates>,
    ) -> Result<CompactionSummary, BcaipError> {
        let messages: Vec<Message> = messages
            .iter()
            .map(CompactionMessage::to_bcaip_message)
            .collect();
        let templates = templates.map(Into::into).unwrap_or_default();
        let model = bcaip_context_management::ProviderModel::new(
            self.handle.provider.clone(),
            ModelConfig::new(&model_name),
        );

        let summary = run_on_runtime(async move {
            bcaip_context_management::summarize(&model, None, &templates, &messages).await
        })
        .await?
        .map_err(BcaipError::generic)?;

        Ok(CompactionSummary {
            text: summary.message.as_concat_text(),
            input_tokens: summary.usage.usage.input_tokens,
            output_tokens: summary.usage.usage.output_tokens,
            total_tokens: summary.usage.usage.total_tokens,
            cache_read_input_tokens: summary.usage.usage.cache_read_input_tokens,
            cache_creation_input_tokens: summary.usage.usage.cache_write_input_tokens,
        })
    }
}

/// A text-only message. Compaction reads conversations as text, so this is the
/// whole input shape callers need across the language boundary.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CompactionMessage {
    pub role: MessageRole,
    pub text: String,
}

impl CompactionMessage {
    fn to_bcaip_message(&self) -> Message {
        let role = match self.role {
            MessageRole::User | MessageRole::Tool => Role::User,
            MessageRole::Assistant => Role::Assistant,
        };
        let mut message = match role {
            Role::User => Message::user(),
            Role::Assistant => Message::assistant(),
        }
        .with_text(&self.text);
        message.role = role;
        message
    }
}

/// Overrides for the summarization and summary-rendering prompts.
#[derive(Debug, Clone, uniffi::Record)]
pub struct CompactionTemplates {
    pub compaction: String,
    pub summary: String,
}

impl From<CompactionTemplates> for bcaip_context_management::Templates {
    fn from(value: CompactionTemplates) -> Self {
        Self {
            compaction: value.compaction,
            summary: value.summary,
        }
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct CompactionSummary {
    pub text: String,
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
    pub cache_read_input_tokens: Option<i32>,
    pub cache_creation_input_tokens: Option<i32>,
}

#[uniffi::export]
pub fn default_compaction_templates() -> CompactionTemplates {
    let templates = bcaip_context_management::Templates::default();
    CompactionTemplates {
        compaction: templates.compaction,
        summary: templates.summary,
    }
}

#[uniffi::export]
pub fn declarative_provider_from_json(json: String) -> Result<Arc<Provider>, BcaipError> {
    let provider = bcaip_providers::declarative::from_json(&json, None, EnvKeyResolver {})?;
    Ok(Provider::new(provider))
}

fn decision_provider(provider: impl BcaipDecisionProvider + 'static) -> Arc<DecisionProvider> {
    Arc::new(DecisionProvider {
        provider: Arc::new(provider),
    })
}

#[uniffi::export]
pub fn openrouter_decision_provider(
    api_key: String,
    base_url: Option<String>,
) -> Result<Arc<DecisionProvider>, BcaipError> {
    let client = ApiClient::new_with_tls(
        base_url.unwrap_or_else(|| "https://openrouter.ai".to_string()),
        AuthMethod::BearerToken(api_key),
        None,
    )?;
    Ok(decision_provider(
        bcaip_providers::openrouter::OpenRouterProvider::new(client, None, None),
    ))
}

#[uniffi::export]
pub fn typesafe_decision_provider(
    api_key: String,
    base_url: Option<String>,
) -> Result<Arc<DecisionProvider>, BcaipError> {
    let client = ApiClient::new_with_tls(
        base_url.unwrap_or_else(|| bcaip_providers::typesafe::TYPESAFE_DEFAULT_HOST.to_string()),
        AuthMethod::BearerToken(api_key),
        None,
    )?;
    Ok(decision_provider(
        bcaip_providers::typesafe::TypeSafeProvider::new(client),
    ))
}

#[uniffi::export]
pub fn openrouter_decision_default_model() -> String {
    bcaip_providers::openrouter::OPENROUTER_DECISION_DEFAULT_MODEL.to_string()
}

#[uniffi::export]
pub fn typesafe_decision_default_model() -> String {
    bcaip_providers::typesafe::TYPESAFE_DEFAULT_MODEL.to_string()
}

#[uniffi::export]
pub fn openai_default_model() -> String {
    bcaip_providers::openai::OPEN_AI_DEFAULT_MODEL.to_string()
}

/// Simple one-argument wrapper for Rust callers who do not need a custom base URL.
///
/// This preserves backward compatibility: existing code calling `openai_provider(api_key)`
/// can continue to compile after the `base_url` parameter was added.
#[uniffi::export]
pub fn openai_provider_simple(api_key: String) -> Result<Arc<Provider>, BcaipError> {
    openai_provider(api_key, None)
}

/// Create an OpenAI provider with an optional custom base URL.
///
/// Pass `None` for the default `https://api.openai.com`, or a custom host
/// (e.g. DeepSeek, Kimi) to enable reasoning-context preservation.
#[uniffi::export(default(base_url = None))]
pub fn openai_provider(
    api_key: String,
    base_url: Option<String>,
) -> Result<Arc<Provider>, BcaipError> {
    let raw = base_url.unwrap_or_else(|| "https://api.openai.com".to_string());

    let (host, query_params, has_v1) = parse_openai_base_url(&raw)?;
    let is_openai = is_direct_openai_host(&raw);

    let auth = if api_key.is_empty() {
        AuthMethod::NoAuth
    } else {
        AuthMethod::BearerToken(api_key)
    };

    let mut api_client = ApiClient::with_timeout_and_tls(
        host,
        auth,
        Duration::from_secs(bcaip_providers::api_client::DEFAULT_PROVIDER_TIMEOUT_SECS),
        None,
    )?;

    if !query_params.is_empty() {
        api_client = api_client.with_query(query_params);
    }

    // For the real OpenAI API, model-based routing is correct — Responses-family
    // models should go to /v1/responses. For custom hosts, always keep chat
    // completions: a server may include `/v1` in its URL without implementing
    // the Responses endpoint, and the versionless path still matches the
    // `is_chat_completions_path()` check below.
    let base_path = if is_openai {
        if has_v1 {
            OPEN_AI_DEFAULT_BASE_PATH.to_string()
        } else {
            OPEN_AI_VERSIONLESS_BASE_PATH.to_string()
        }
    } else {
        OPEN_AI_VERSIONLESS_BASE_PATH.to_string()
    };

    let provider = OpenAiProviderBuilder::new(api_client)
        .base_path(base_path)
        .preserve_thinking_context(!is_openai)
        .build();

    Ok(Provider::new(Box::new(provider)))
}

/// Determine whether a host is the real OpenAI API (not a custom compatible server).
///
/// Extracts the hostname from URLs like `https://api.openai.com` or plain hostnames.
/// Compares exactly to avoid false positives (e.g. `api.openai.com.local:8000`).
fn is_direct_openai_host(raw_url: &str) -> bool {
    // Strip scheme if present to get just the hostname portion.
    let hostname = raw_url
        .split("://")
        .nth(1)
        .unwrap_or(raw_url)
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");

    let h = hostname.to_ascii_lowercase();
    h == "api.openai.com" || h.ends_with(".api.openai.com")
}

#[uniffi::export]
pub fn anthropic_default_model() -> String {
    bcaip_providers::anthropic::ANTHROPIC_DEFAULT_MODEL.to_string()
}

#[uniffi::export]
pub fn anthropic_provider(
    api_key: String,
    base_url: Option<String>,
    beta_headers: Vec<String>,
) -> Result<Arc<Provider>, BcaipError> {
    let mut api_client = ApiClient::new_with_tls(
        base_url.unwrap_or_else(|| "https://api.anthropic.com".to_string()),
        AuthMethod::ApiKey {
            header_name: "x-api-key".to_string(),
            key: api_key,
        },
        None,
    )?
    .with_header(
        "anthropic-version",
        bcaip_providers::anthropic::ANTHROPIC_API_VERSION,
    )?;

    if !beta_headers.is_empty() {
        api_client = api_client.with_header("anthropic-beta", &beta_headers.join(","))?;
    }

    let provider = AnthropicProviderBuilder::new(api_client)
        .format_options(bcaip_provider_types::formats::AnthropicFormatOptions::native())
        .build();
    Ok(Provider::new(Box::new(provider)))
}

#[uniffi::export]
pub fn groq_default_model() -> String {
    groq_config()
        .models
        .first()
        .map(|model| model.name.clone())
        .unwrap_or_else(|| "moonshotai/kimi-k2-instruct-0905".to_string())
}

#[uniffi::export]
pub fn groq_provider(api_key: String) -> Result<Arc<Provider>, BcaipError> {
    let json = bcaip_providers::groq::JSON.replace("${GROQ_API_KEY}", &api_key);
    let provider = bcaip_providers::declarative::from_json(
        &json,
        None,
        StaticKeyResolver {
            key_name: "GROQ_API_KEY".to_string(),
            key: api_key,
        },
    )?;
    Ok(Provider::new(provider))
}

fn groq_config() -> DeclarativeProviderConfig {
    bcaip_providers::declarative::deserialize_provider_config(bcaip_providers::groq::JSON)
        .expect("bundled groq provider config is valid")
}

struct StaticKeyResolver {
    key_name: String,
    key: String,
}

impl bcaip_providers::declarative::KeyResolver for StaticKeyResolver {
    type Error = std::env::VarError;

    fn resolve_key(&self, key: &str) -> std::result::Result<String, Self::Error> {
        if key == self.key_name {
            Ok(self.key.clone())
        } else {
            std::env::var(key)
        }
    }
}

#[uniffi::export]
pub fn databricks_default_model() -> String {
    bcaip_providers::databricks::DATABRICKS_DEFAULT_MODEL.to_string()
}

#[uniffi::export]
pub fn databricks_provider(host: String, token: String) -> Result<Arc<Provider>, BcaipError> {
    let retry_config = BcaipDatabricksProvider::load_retry_config(|key| std::env::var(key).ok());
    let provider = BcaipDatabricksProvider::new(
        host,
        DatabricksAuth::token(token),
        retry_config,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )?;

    Ok(Provider::new(Box::new(provider)))
}

#[uniffi::export]
pub fn databricks_v2_default_model() -> String {
    bcaip_providers::databricks_v2::DATABRICKS_V2_DEFAULT_MODEL.to_string()
}

#[uniffi::export(default(gateway_path = None))]
pub fn databricks_v2_provider(
    host: String,
    token: String,
    gateway_path: Option<String>,
) -> Result<Arc<Provider>, BcaipError> {
    let retry_config = BcaipDatabricksV2Provider::load_retry_config(|key| std::env::var(key).ok());
    let provider = BcaipDatabricksV2Provider::new(
        host,
        DatabricksAuth::token(token),
        retry_config,
        None,
        None,
        None,
        None,
        None,
    )?;
    let provider = match gateway_path {
        Some(path) => provider.with_gateway_path(&path)?,
        None => provider,
    };

    Ok(Provider::new(Box::new(provider)))
}

#[derive(uniffi::Object)]
pub struct ProviderStream {
    state: Arc<tokio::sync::Mutex<ProviderStreamState>>,
    timeout_ms: Option<u64>,
    observer: Arc<RequestObserver>,
}

struct ProviderStreamState {
    stream: MessageStream,
    pending: Vec<StreamChunk>,
    final_usage: Option<Usage>,
    ended: bool,
}

#[uniffi::export]
impl ProviderStream {
    pub async fn next_chunk(&self) -> Result<Option<StreamChunk>, BcaipError> {
        let state = Arc::clone(&self.state);
        let timeout_ms = self.timeout_ms;
        let observer = Arc::clone(&self.observer);
        run_on_runtime(async move {
            let mut state = state.lock().await;
            loop {
                if let Some(chunk) = state.pending.pop() {
                    return Ok(Some(chunk));
                }

                if state.ended {
                    return Ok(None);
                }

                let next = if let Some(timeout_ms) = timeout_ms {
                    match tokio::time::timeout(
                        Duration::from_millis(timeout_ms),
                        state.stream.next(),
                    )
                    .await
                    {
                        Ok(next) => next,
                        Err(_) => {
                            return Err(observer.fail(BcaipError::Timeout {
                                details: format!("request timed out after {timeout_ms}ms"),
                            }));
                        }
                    }
                } else {
                    state.stream.next().await
                };

                match next {
                    Some(Ok((message, usage))) => {
                        if let Some(usage) = usage {
                            match Usage::from_provider_usage(&usage) {
                                Ok(usage) => state.final_usage = Some(usage),
                                Err(error) => {
                                    state.ended = true;
                                    return Err(observer.fail(error));
                                }
                            }
                        }
                        let Some(message) = message else {
                            continue;
                        };
                        let mut chunks = message_to_chunks(message);
                        if chunks.is_empty() {
                            continue;
                        }
                        chunks.reverse();
                        let first = chunks.pop();
                        state.pending = chunks;
                        return Ok(first);
                    }
                    Some(Err(error)) => {
                        state.ended = true;
                        let error = BcaipStreamError::from(&BcaipError::from(error));
                        observer.fail_stream(error.clone());
                        return Ok(Some(StreamChunk::ErrorChunk { error }));
                    }
                    None => {
                        state.ended = true;
                        let usage = state.final_usage.clone();
                        observer.succeeded(usage.clone(), None);
                        return Ok(Some(StreamChunk::EndChunk { usage }));
                    }
                }
            }
        })
        .await?
    }
}

fn message_to_chunks(message: Message) -> Vec<StreamChunk> {
    message
        .content
        .into_iter()
        .filter_map(|content| match content {
            BcaipMessageContent::Text(text) if !text.text.is_empty() => {
                Some(StreamChunk::TextChunk {
                    text: text.text.clone(),
                })
            }
            BcaipMessageContent::ToolRequest(request) => {
                let index = request.provider_index();
                let provider_metadata_json = request
                    .metadata
                    .as_ref()
                    .and_then(|metadata| serde_json::to_string(metadata).ok());
                match request.tool_call {
                    Ok(tool_call) => Some(StreamChunk::ToolChunk {
                        index,
                        id: request.id,
                        name: tool_call.name.to_string(),
                        arguments_json: serde_json::to_string(
                            &tool_call.arguments.unwrap_or_default(),
                        )
                        .unwrap_or_else(|_| "{}".to_string()),
                        provider_metadata_json,
                    }),
                    Err(error) => Some(StreamChunk::ErrorChunk {
                        error: BcaipStreamError {
                            kind: BcaipStreamErrorKind::Generic,
                            message: error.to_string(),
                            retry_after_ms: None,
                        },
                    }),
                }
            }
            BcaipMessageContent::Thinking(thinking) => Some(StreamChunk::ThinkingChunk {
                thinking: thinking.thinking,
                signature: thinking.signature,
            }),
            BcaipMessageContent::RedactedThinking(redacted) => {
                Some(StreamChunk::RedactedThinkingChunk {
                    data: redacted.data,
                })
            }
            _ => None,
        })
        .collect()
}

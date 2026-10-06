use super::base::ProviderDef;
use super::formats::bedrock::{
    bedrock_anthropic_thinking_fields, bedrock_inference_config, from_bedrock_message,
    from_bedrock_usage, sanitize_json_unicode_tags, to_bedrock_message_with_caching,
    to_bedrock_tool_config,
};
use crate::session_context::SESSION_ID_HEADER;
use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use aws_sdk_bedrockruntime::{Client, types as bedrock};
use aws_sdk_bedrockruntime::{
    config::ProvideCredentials,
    operation::{converse::ConverseError, converse_stream::ConverseStreamError},
    types::error::ConverseStreamOutputError,
};
use base64::Engine;
use bcaip_provider_types::ProviderSetupCategory::Model;
use bcaip_provider_types::ProviderSetupGroup::Additional;
use bcaip_provider_types::ProviderSetupMethod::CloudCredentials;
use bcaip_provider_types::base::{
    ConfigKey, MessageStream, ModelInfo, Provider, ProviderDescriptor, ProviderMetadata,
    model_info_for_provider_model,
};
use bcaip_provider_types::context_limit::ContextLimitResolver;
use bcaip_provider_types::conversations::Message;
use bcaip_provider_types::conversations::{ProviderUsage, Usage};
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::formats::{create_responses_request, extract_reasoning_effort};
use bcaip_provider_types::model::ModelConfig;
use bcaip_provider_types::request_log::{LoggerHandleExt, start_log};
use bcaip_provider_types::retry::{ProviderRetry, RetryConfig};
use bcaip_provider_types::{ProviderSetupMetadata, json};
use futures::future::BoxFuture;
use goose_providers::api_client::{DEFAULT_CONNECT_TIMEOUT_SECS, DEFAULT_PROVIDER_TIMEOUT_SECS};
use goose_providers::openai_compatible::{handle_status, stream_responses_compat};
use reqwest::header::{AUTHORIZATION, HeaderName, HeaderValue};
use rmcp::model::{CallToolRequestParams, ErrorCode, ErrorData, Tool, object};
use serde_json::Value;
use smithy_transport_reqwest::ReqwestHttpClient;
use std::collections::HashMap;

pub(crate) const BEDROCK_PROVIDER_NAME: &str = "aws_bedrock";
pub const BEDROCK_DOC_LINK: &str =
    "https://docs.aws.amazon.com/bedrock/latest/userguide/models-supported.html";

pub const BEDROCK_DEFAULT_MODEL: &str = "us.anthropic.claude-sonnet-4-5-20250929-v1:0";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BedrockEndpoint {
    Converse,
    MantleResponses,
}

#[derive(Debug, Clone, Copy)]
struct BedrockModelEntry {
    name: &'static str,
    wire_model_id: &'static str,
    endpoint: BedrockEndpoint,
    context_limit: Option<u32>,
}

const BEDROCK_MODEL_TABLE: &[BedrockModelEntry] = &[
    BedrockModelEntry {
        name: "global.anthropic.claude-sonnet-5",
        wire_model_id: "global.anthropic.claude-sonnet-5",
        endpoint: BedrockEndpoint::Converse,
        context_limit: None,
    },
    BedrockModelEntry {
        name: "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
        wire_model_id: "us.anthropic.claude-sonnet-4-5-20250929-v1:0",
        endpoint: BedrockEndpoint::Converse,
        context_limit: None,
    },
    BedrockModelEntry {
        name: "us.anthropic.claude-sonnet-4-20250514-v1:0",
        wire_model_id: "us.anthropic.claude-sonnet-4-20250514-v1:0",
        endpoint: BedrockEndpoint::Converse,
        context_limit: None,
    },
    BedrockModelEntry {
        name: "us.anthropic.claude-3-7-sonnet-20250219-v1:0",
        wire_model_id: "us.anthropic.claude-3-7-sonnet-20250219-v1:0",
        endpoint: BedrockEndpoint::Converse,
        context_limit: None,
    },
    BedrockModelEntry {
        name: "us.anthropic.claude-opus-4-20250514-v1:0",
        wire_model_id: "us.anthropic.claude-opus-4-20250514-v1:0",
        endpoint: BedrockEndpoint::Converse,
        context_limit: None,
    },
    BedrockModelEntry {
        name: "us.anthropic.claude-opus-4-1-20250805-v1:0",
        wire_model_id: "us.anthropic.claude-opus-4-1-20250805-v1:0",
        endpoint: BedrockEndpoint::Converse,
        context_limit: None,
    },
    BedrockModelEntry {
        name: "openai.gpt-5.5",
        wire_model_id: "openai.gpt-5.5",
        endpoint: BedrockEndpoint::MantleResponses,
        context_limit: None,
    },
    BedrockModelEntry {
        name: "openai.gpt-5.4",
        wire_model_id: "openai.gpt-5.4",
        endpoint: BedrockEndpoint::MantleResponses,
        context_limit: None,
    },
    BedrockModelEntry {
        name: "openai.gpt-5.6-sol",
        wire_model_id: "openai.gpt-5.6-sol",
        endpoint: BedrockEndpoint::MantleResponses,
        context_limit: None,
    },
    BedrockModelEntry {
        name: "openai.gpt-5.6-terra",
        wire_model_id: "openai.gpt-5.6-terra",
        endpoint: BedrockEndpoint::MantleResponses,
        context_limit: None,
    },
    BedrockModelEntry {
        name: "openai.gpt-5.6-luna",
        wire_model_id: "openai.gpt-5.6-luna",
        endpoint: BedrockEndpoint::MantleResponses,
        context_limit: None,
    },
    BedrockModelEntry {
        name: "google.gemma-4-31b",
        wire_model_id: "google.gemma-4-31b",
        endpoint: BedrockEndpoint::MantleResponses,
        context_limit: Some(262144),
    },
    BedrockModelEntry {
        name: "google.gemma-4-26b-a4b",
        wire_model_id: "google.gemma-4-26b-a4b",
        endpoint: BedrockEndpoint::MantleResponses,
        context_limit: Some(262144),
    },
    BedrockModelEntry {
        name: "google.gemma-4-e2b",
        wire_model_id: "google.gemma-4-e2b",
        endpoint: BedrockEndpoint::MantleResponses,
        context_limit: None,
    },
];

pub(crate) fn local_context_limit(model: &str) -> Option<usize> {
    BEDROCK_MODEL_TABLE
        .iter()
        .find(|entry| entry.name.eq_ignore_ascii_case(model))
        .and_then(|entry| entry.context_limit)
        .map(|limit| limit as usize)
}

fn find_model_entry(name: &str) -> Option<&'static BedrockModelEntry> {
    // Direct lookup first (handles exact names like "google.gemma-4-31b")
    if let Some(entry) = BEDROCK_MODEL_TABLE.iter().find(|e| e.name == name) {
        return Some(entry);
    }
    // For openai.* names, strip the prefix, extract effort suffix, then reconstruct
    if let Some(without_prefix) = name.strip_prefix("openai.") {
        let (base, _) = extract_reasoning_effort(without_prefix);
        let candidate = format!("openai.{}", base);
        return BEDROCK_MODEL_TABLE.iter().find(|e| e.name == candidate);
    }
    // For other names, try stripping effort suffix directly
    let (base_name, _) = extract_reasoning_effort(name);
    if let Some(entry) = BEDROCK_MODEL_TABLE.iter().find(|e| e.name == base_name) {
        return Some(entry);
    }
    let candidate = format!("openai.{}", base_name);
    BEDROCK_MODEL_TABLE.iter().find(|e| e.name == candidate)
}

pub const BEDROCK_DEFAULT_MAX_RETRIES: usize = 6;
pub const BEDROCK_DEFAULT_INITIAL_RETRY_INTERVAL_MS: u64 = 2000;
pub const BEDROCK_DEFAULT_BACKOFF_MULTIPLIER: f64 = 2.0;
pub const BEDROCK_DEFAULT_MAX_RETRY_INTERVAL_MS: u64 = 120_000;

#[derive(Debug, serde::Serialize)]
pub struct BedrockProvider {
    #[serde(skip)]
    client: Client,
    #[serde(skip)]
    retry_config: RetryConfig,
    #[serde(skip)]
    name: String,
    #[serde(skip)]
    region: Option<String>,
    #[serde(skip)]
    bearer_token: Option<String>,
    #[serde(skip)]
    http_client: reqwest::Client,
    #[serde(skip)]
    mantle_base_url: Option<String>,
}

/// Request inputs shared by the `Converse` and `ConverseStream` APIs.
struct ConverseRequestParts {
    system_blocks: Vec<bedrock::SystemContentBlock>,
    messages: Vec<bedrock::Message>,
    tool_config: Option<bedrock::ToolConfiguration>,
    thinking_fields: Option<aws_smithy_types::Document>,
    inference_config: bedrock::InferenceConfiguration,
}

impl BedrockProvider {
    pub async fn from_env(
        _tls_config: Option<goose_providers::api_client::TlsConfig>,
    ) -> Result<Self> {
        let config = crate::config::Config::global();

        // Check for bearer token first to determine if region is required
        let bearer_token = match config.get_secret::<String>("AWS_BEARER_TOKEN_BEDROCK") {
            Ok(token) => {
                let token = token.trim().to_string();
                if token.is_empty() { None } else { Some(token) }
            }
            Err(_) => None,
        };

        // Get AWS_REGION from config if explicitly set (optional - SDK can resolve from other sources)
        let region = match config.get_param::<String>("AWS_REGION") {
            Ok(r) if !r.is_empty() => Some(r),
            Ok(_) => None,
            Err(_) => None,
        };

        // Use load_defaults() which supports AWS SSO, profiles, and environment variables
        let mut loader = aws_config::defaults(aws_config::BehaviorVersion::latest())
            .http_client(ReqwestHttpClient::new());

        if let Ok(profile_name) = config.get_param::<String>("AWS_PROFILE") {
            if !profile_name.is_empty() {
                loader = loader.profile_name(&profile_name);
            }
        }

        // Apply region to loader if explicitly configured
        if let Some(ref region) = region {
            loader = loader.region(aws_config::Region::new(region.clone()));
        }

        let sdk_config = loader.load().await;

        // Validate region requirement for bearer token auth after SDK config is loaded
        // This allows region to be resolved from ~/.aws/config, AWS_DEFAULT_REGION, etc.
        if bearer_token.is_some() && sdk_config.region().is_none() {
            return Err(anyhow::anyhow!(
                "AWS region is required when using AWS_BEARER_TOKEN_BEDROCK authentication. \
                Set AWS_REGION, AWS_DEFAULT_REGION, or configure region in your AWS profile."
            ));
        }

        let resolved_region = sdk_config.region().map(|r| r.to_string());

        let client = if let Some(ref token) = bearer_token {
            // Build from sdk_config to inherit all settings (endpoint overrides, timeouts, etc.)
            // then override authentication with bearer token
            let bedrock_config = aws_sdk_bedrockruntime::Config::new(&sdk_config)
                .to_builder()
                .bearer_token(aws_sdk_bedrockruntime::config::Token::new(
                    token.clone(),
                    None,
                ))
                .auth_scheme_preference([
                    aws_smithy_runtime_api::client::auth::http::HTTP_BEARER_AUTH_SCHEME_ID,
                ])
                .build();

            Client::from_conf(bedrock_config)
        } else {
            Self::create_client_with_credentials(&sdk_config).await?
        };

        let retry_config = Self::load_retry_config(config);

        Ok(Self {
            client,
            retry_config,
            name: BEDROCK_PROVIDER_NAME.to_string(),
            region: resolved_region,
            bearer_token,
            http_client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(DEFAULT_CONNECT_TIMEOUT_SECS))
                .read_timeout(std::time::Duration::from_secs(
                    DEFAULT_PROVIDER_TIMEOUT_SECS,
                ))
                .build()?,
            mantle_base_url: None,
        })
    }

    async fn create_client_with_credentials(sdk_config: &aws_config::SdkConfig) -> Result<Client> {
        sdk_config
            .credentials_provider()
            .ok_or_else(|| anyhow::anyhow!("No AWS credentials provider configured"))?
            .provide_credentials()
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to load AWS credentials: {}. Make sure to run 'aws sso login --profile <your-profile>' if using SSO",
                    e
                )
            })?;

        Ok(Client::new(sdk_config))
    }

    fn load_retry_config(config: &crate::config::Config) -> RetryConfig {
        let max_retries = config
            .get_param::<usize>("BEDROCK_MAX_RETRIES")
            .unwrap_or(BEDROCK_DEFAULT_MAX_RETRIES);

        let initial_interval_ms = config
            .get_param::<u64>("BEDROCK_INITIAL_RETRY_INTERVAL_MS")
            .unwrap_or(BEDROCK_DEFAULT_INITIAL_RETRY_INTERVAL_MS);

        let backoff_multiplier = config
            .get_param::<f64>("BEDROCK_BACKOFF_MULTIPLIER")
            .unwrap_or(BEDROCK_DEFAULT_BACKOFF_MULTIPLIER);

        let max_interval_ms = config
            .get_param::<u64>("BEDROCK_MAX_RETRY_INTERVAL_MS")
            .unwrap_or(BEDROCK_DEFAULT_MAX_RETRY_INTERVAL_MS);

        RetryConfig::new(
            max_retries,
            initial_interval_ms,
            backoff_multiplier,
            max_interval_ms,
        )
    }

    fn should_enable_caching(&self, model: &ModelConfig) -> bool {
        let config = crate::config::Config::global();

        let enabled = config
            .get_param::<bool>("BEDROCK_ENABLE_CACHING")
            .unwrap_or(false);
        enabled && model.model_name.contains("anthropic.claude") && !model.prompt_cache_disabled()
    }

    async fn post_mantle_streaming(
        &self,
        session_id: Option<&str>,
        payload: &Value,
    ) -> Result<reqwest::Response, ProviderError> {
        let region = self.region.as_deref().ok_or_else(|| {
            ProviderError::Authentication(
                "AWS region is required for Bedrock mantle endpoint".to_string(),
            )
        })?;
        let token = self.bearer_token.as_deref().ok_or_else(|| {
            ProviderError::Authentication(
                "AWS_BEARER_TOKEN_BEDROCK is required for openai.gpt-* models".to_string(),
            )
        })?;

        let url = self.mantle_base_url.clone().unwrap_or_else(|| {
            format!(
                "https://bedrock-mantle.{}.api.aws/openai/v1/responses",
                region
            )
        });

        let mut req = self
            .http_client
            .post(&url)
            .header(AUTHORIZATION, format!("Bearer {}", token))
            .json(payload);

        if let Some(id) = session_id.filter(|id| !id.is_empty())
            && let (Ok(name), Ok(value)) = (
                HeaderName::from_bytes(SESSION_ID_HEADER.as_bytes()),
                HeaderValue::from_str(id),
            ) {
                req = req.header(name, value);
            }

        let response = goose_providers::http_status::send_bounded(
            req,
            std::time::Duration::from_secs(DEFAULT_PROVIDER_TIMEOUT_SECS),
        )
        .await?;

        handle_status(response).await
    }

    /// Build the request inputs shared by [`Self::converse`] and
    /// [`Self::converse_stream`]: system blocks (with optional cache point),
    /// converted messages (with optional trailing-message cache point), and
    /// the tool configuration.
    fn build_request_parts(
        &self,
        model: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<ConverseRequestParts, ProviderError> {
        let enable_caching = self.should_enable_caching(model);

        let system_blocks = if enable_caching {
            vec![
                bedrock::SystemContentBlock::Text(system.to_string()),
                // Add cache point AFTER the system prompt content
                bedrock::SystemContentBlock::CachePoint(
                    bedrock::CachePointBlock::builder()
                        .r#type(bedrock::CachePointType::Default)
                        .build()
                        .map_err(|e| {
                            ProviderError::ExecutionError(format!(
                                "Failed to build cache point: {}",
                                e
                            ))
                        })?,
                ),
            ]
        } else {
            vec![bedrock::SystemContentBlock::Text(system.to_string())]
        };

        let visible_messages: Vec<&Message> =
            messages.iter().filter(|m| m.is_agent_visible()).collect();

        let mut bedrock_messages: Vec<bedrock::Message> = Vec::new();
        for message in visible_messages {
            let formatted =
                to_bedrock_message_with_caching(message, false, Some(&model.model_name))?;
            if formatted.content().is_empty() {
                continue;
            }
            if let Some(previous) = bedrock_messages.last_mut()
                && previous.role() == formatted.role() {
                    previous.content.extend(formatted.content);
                    continue;
                }
            bedrock_messages.push(formatted);
        }

        if enable_caching
            && let Some(last) = bedrock_messages.last_mut() {
                last.content.push(bedrock::ContentBlock::CachePoint(
                    bedrock::CachePointBlock::builder()
                        .r#type(bedrock::CachePointType::Default)
                        .build()
                        .map_err(|error| ProviderError::ExecutionError(error.to_string()))?,
                ));
            }

        let tool_config = if tools.is_empty() {
            None
        } else {
            Some(to_bedrock_tool_config(tools)?)
        };

        Ok(ConverseRequestParts {
            system_blocks,
            messages: bedrock_messages,
            tool_config,
            thinking_fields: bedrock_anthropic_thinking_fields(model),
            inference_config: bedrock_inference_config(model),
        })
    }

    async fn converse(
        &self,
        model: &ModelConfig,
        session_id: Option<&str>,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<(bedrock::Message, Option<bedrock::TokenUsage>), ProviderError> {
        let parts = self.build_request_parts(model, system, messages, tools)?;

        let mut request = self
            .client
            .converse()
            .set_system(Some(parts.system_blocks))
            .model_id(&model.model_name)
            .set_messages(Some(parts.messages))
            .inference_config(parts.inference_config);

        if let Some(fields) = parts.thinking_fields {
            request = request.additional_model_request_fields(fields);
        }

        if let Some(tool_config) = parts.tool_config {
            request = request.tool_config(tool_config);
        }

        let mut request = request.customize();

        if let Some(session_id) = session_id.filter(|id| !id.is_empty()) {
            let session_id = session_id.to_string();
            request = request.mutate_request(move |req| {
                if let Ok(value) = HeaderValue::from_str(&session_id) {
                    req.headers_mut().insert(SESSION_ID_HEADER, value);
                }
            });
        }

        let response = request
            .send()
            .await
            .map_err(|err| match err.into_service_error() {
                ConverseError::ThrottlingException(throttle_err) => {
                    ProviderError::RateLimitExceeded {
                        details: format!("Bedrock throttling error: {:?}", throttle_err),
                        retry_delay: None,
                    }
                }
                ConverseError::AccessDeniedException(err) => {
                    ProviderError::Authentication(format!("Failed to call Bedrock: {:?}", err))
                }
                ConverseError::ValidationException(err)
                    if {
                        let msg = err.message().unwrap_or_default();
                        msg.contains("Input is too long for requested model.")
                            || msg.contains("prompt is too long")
                    } =>
                {
                    ProviderError::ContextLengthExceeded(format!(
                        "Failed to call Bedrock: {:?}",
                        err
                    ))
                }
                ConverseError::ValidationException(err) => ProviderError::ExecutionError(format!(
                    "Bedrock validation error: {}",
                    err.message().unwrap_or("unknown validation error")
                )),
                ConverseError::ModelErrorException(err) => {
                    ProviderError::ExecutionError(format!("Failed to call Bedrock: {:?}", err))
                }
                err => ProviderError::ServerError(format!("Failed to call Bedrock: {:?}", err)),
            })?;

        match response.output {
            Some(bedrock::ConverseOutput::Message(message)) => Ok((message, response.usage)),
            _ => Err(ProviderError::RequestFailed(
                "No output from Bedrock".to_string(),
            )),
        }
    }

    /// Escape hatch: `BEDROCK_DISABLE_STREAMING=true` restores the previous
    /// blocking `Converse` behaviour in case a model or region misbehaves
    /// with `ConverseStream`.
    fn streaming_disabled(&self) -> bool {
        let config = crate::config::Config::global();
        config
            .get_param::<bool>("BEDROCK_DISABLE_STREAMING")
            .unwrap_or(false)
    }

    /// Streaming variant of [`Self::converse`]. Builds an identical request
    /// but calls the AWS `ConverseStream` API, returning the raw event
    /// receiver so [`Provider::stream`] can forward deltas incrementally.
    async fn converse_stream(
        &self,
        model: &ModelConfig,
        session_id: Option<&str>,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<
        aws_sdk_bedrockruntime::operation::converse_stream::ConverseStreamOutput,
        ProviderError,
    > {
        let parts = self.build_request_parts(model, system, messages, tools)?;

        let mut request = self
            .client
            .converse_stream()
            .set_system(Some(parts.system_blocks))
            .model_id(&model.model_name)
            .set_messages(Some(parts.messages))
            .inference_config(parts.inference_config);

        if let Some(fields) = parts.thinking_fields {
            request = request.additional_model_request_fields(fields);
        }

        if let Some(tool_config) = parts.tool_config {
            request = request.tool_config(tool_config);
        }

        let mut request = request.customize();

        if let Some(session_id) = session_id.filter(|id| !id.is_empty()) {
            let session_id = session_id.to_string();
            request = request.mutate_request(move |req| {
                if let Ok(value) = HeaderValue::from_str(&session_id) {
                    req.headers_mut().insert(SESSION_ID_HEADER, value);
                }
            });
        }

        request
            .send()
            .await
            .map_err(|err| match err.into_service_error() {
                ConverseStreamError::ThrottlingException(throttle_err) => {
                    ProviderError::RateLimitExceeded {
                        details: format!("Bedrock throttling error: {:?}", throttle_err),
                        retry_delay: None,
                    }
                }
                ConverseStreamError::AccessDeniedException(err) => {
                    ProviderError::Authentication(format!("Failed to call Bedrock: {:?}", err))
                }
                ConverseStreamError::ValidationException(err)
                    if {
                        let msg = err.message().unwrap_or_default();
                        msg.contains("Input is too long for requested model.")
                            || msg.contains("prompt is too long")
                    } =>
                {
                    ProviderError::ContextLengthExceeded(format!(
                        "Failed to call Bedrock: {:?}",
                        err
                    ))
                }
                ConverseStreamError::ModelErrorException(err) => {
                    ProviderError::ExecutionError(format!("Failed to call Bedrock: {:?}", err))
                }
                err => ProviderError::ServerError(format!("Failed to call Bedrock: {:?}", err)),
            })
    }

    /// Pre-ConverseStream behaviour: blocking `Converse` call wrapped in a
    /// single-item stream. Kept as the `BEDROCK_DISABLE_STREAMING=true`
    /// escape hatch.
    async fn stream_via_converse(
        &self,
        model: &ModelConfig,
        session_id: Option<&str>,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
        model_name: &str,
    ) -> Result<MessageStream, ProviderError> {
        let (bedrock_message, bedrock_usage) = self
            .with_retry(|| self.converse(model, session_id, system, messages, tools))
            .await?;

        let usage = bedrock_usage
            .as_ref()
            .map(from_bedrock_usage)
            .unwrap_or_default();

        let message = from_bedrock_message(&bedrock_message)?;

        // Add debug trace with input context
        let debug_payload = serde_json::json!({
            "system": system,
            "messages": messages,
            "tools": tools
        });
        let mut log = start_log(model, &debug_payload)?;
        log.write(
            &serde_json::to_value(&message).unwrap_or_default(),
            Some(&usage),
        )?;

        let provider_usage = ProviderUsage::new(model_name.to_string(), usage);
        Ok(bcaip_provider_types::base::stream_from_single_message(
            message,
            provider_usage,
        ))
    }
}

/// Accumulation state for in-flight content blocks while consuming a
/// `ConverseStream` response. Tool inputs and reasoning content arrive as
/// fragments that only become a complete [`Message`] at `ContentBlockStop`.
#[derive(Default)]
struct StreamBlockState {
    /// content_block_index -> (tool_use_id, tool_name, accumulated input JSON)
    tool_blocks: HashMap<i32, (String, String, String)>,
    /// content_block_index -> (accumulated reasoning text, accumulated signature)
    reasoning_blocks: HashMap<i32, (String, String)>,
    /// content_block_index -> accumulated redacted (encrypted) reasoning bytes
    redacted_blocks: HashMap<i32, Vec<u8>>,
    /// StopReason carried by the stream's MessageStop event, if seen
    stop_reason: Option<bedrock::StopReason>,
}

/// Convert a single `ConverseStream` event into zero or more [`Message`]s
/// ready to be yielded, plus token usage when the event carries it.
///
/// Mirrors the delta-yield contract of
/// `formats::anthropic::response_to_streaming_message`: text deltas yield
/// immediately (token-level chunks); tool-use inputs and reasoning blocks
/// accumulate in `state` until their `ContentBlockStop`.
fn process_stream_event(
    event: bedrock::ConverseStreamOutput,
    state: &mut StreamBlockState,
    message_id: &str,
) -> (Vec<Message>, Option<Usage>) {
    let mut messages = Vec::new();
    let mut usage = None;

    match event {
        bedrock::ConverseStreamOutput::ContentBlockStart(ev) => {
            if let Some(bedrock::ContentBlockStart::ToolUse(tu)) = ev.start {
                state.tool_blocks.insert(
                    ev.content_block_index,
                    (tu.tool_use_id, tu.name, String::new()),
                );
            }
        }
        bedrock::ConverseStreamOutput::ContentBlockDelta(ev) => match ev.delta {
            Some(bedrock::ContentBlockDelta::Text(text)) => {
                if !text.is_empty() {
                    messages.push(Message::assistant().with_text(text).with_id(message_id));
                }
            }
            Some(bedrock::ContentBlockDelta::ToolUse(tu)) => {
                if let Some(entry) = state.tool_blocks.get_mut(&ev.content_block_index) {
                    entry.2.push_str(&tu.input);
                }
            }
            Some(bedrock::ContentBlockDelta::ReasoningContent(rc)) => match rc {
                bedrock::ReasoningContentBlockDelta::Text(t) => {
                    state
                        .reasoning_blocks
                        .entry(ev.content_block_index)
                        .or_default()
                        .0
                        .push_str(&t);
                }
                bedrock::ReasoningContentBlockDelta::Signature(s) => {
                    state
                        .reasoning_blocks
                        .entry(ev.content_block_index)
                        .or_default()
                        .1
                        .push_str(&s);
                }
                bedrock::ReasoningContentBlockDelta::RedactedContent(blob) => {
                    state
                        .redacted_blocks
                        .entry(ev.content_block_index)
                        .or_default()
                        .extend_from_slice(blob.as_ref());
                }
                _ => {}
            },
            _ => {}
        },
        bedrock::ConverseStreamOutput::ContentBlockStop(ev) => {
            let idx = ev.content_block_index;
            if let Some((text, signature)) = state.reasoning_blocks.remove(&idx) {
                if !text.is_empty() || !signature.is_empty() {
                    messages.push(
                        Message::assistant()
                            .with_thinking(text, signature)
                            .with_id(message_id),
                    );
                }
            }
            if let Some(bytes) = state.redacted_blocks.remove(&idx) {
                if !bytes.is_empty() {
                    // Same base64 encoding as the non-streaming path
                    // (formats::bedrock::from_bedrock_reasoning_content_block)
                    // so redacted thinking round-trips back to Bedrock intact.
                    let encoded = base64::prelude::BASE64_STANDARD.encode(&bytes);
                    messages.push(
                        Message::assistant()
                            .with_redacted_thinking(encoded)
                            .with_id(message_id),
                    );
                }
            }
            if let Some((id, name, input_json)) = state.tool_blocks.remove(&idx) {
                // Parse the accumulated tool input. On failure, yield an
                // error tool request (not a stream error) so the agent can
                // report it back to the model — same behaviour as the
                // Anthropic provider.
                let tool_call = if input_json.trim().is_empty() {
                    Ok(CallToolRequestParams::new(name)
                        .with_arguments(object(serde_json::json!({}))))
                } else {
                    match serde_json::from_str::<Value>(&input_json) {
                        Ok(parsed) => sanitize_json_unicode_tags(parsed)
                            .map(|arguments| {
                                CallToolRequestParams::new(name).with_arguments(object(arguments))
                            })
                            .map_err(|error| {
                                ErrorData::new(ErrorCode::INVALID_PARAMS, error.to_string(), None)
                            }),
                        Err(_) => Err(ErrorData::new(
                            ErrorCode::INVALID_PARAMS,
                            json::truncation_error_message(&input_json).unwrap_or_else(|| {
                                format!("Could not parse tool arguments: {}", input_json)
                            }),
                            None,
                        )),
                    }
                };
                messages.push(
                    Message::assistant()
                        .with_tool_request(id, tool_call)
                        .with_id(message_id),
                );
            }
        }
        bedrock::ConverseStreamOutput::MessageStop(ev) => {
            state.stop_reason = Some(ev.stop_reason);
        }
        bedrock::ConverseStreamOutput::Metadata(ev) => {
            if let Some(u) = ev.usage {
                usage = Some(from_bedrock_usage(&u));
            }
        }
        // MessageStart / unknown variants carry no content that needs
        // forwarding.
        _ => {}
    }

    (messages, usage)
}

/// Flush tool blocks left open when the stream ended without their
/// ContentBlockStop. Their arguments are incomplete, so each becomes a failed
/// tool request carrying guidance for the model, matching how the Anthropic
/// provider reports truncated tool calls.
fn flush_incomplete_tool_blocks(
    state: &mut StreamBlockState,
    truncated_by_limit: bool,
    message_id: &str,
) -> Vec<Message> {
    let mut messages = Vec::new();
    let mut indices: Vec<i32> = state.tool_blocks.keys().copied().collect();
    indices.sort_unstable();
    for index in indices {
        if let Some((id, _name, input_json)) = state.tool_blocks.remove(&index) {
            let guidance = if truncated_by_limit {
                "The model's response was truncated because it reached the output token limit while generating this tool call. \
                 Try increasing max_tokens for this provider or breaking the task into smaller steps."
            } else {
                "A tool call was not completed before the stream ended. \
                 Try resending your message or breaking the task into smaller steps."
            };
            let snippet_len = input_json.chars().count();
            let tail: String = input_json
                .chars()
                .rev()
                .take(80)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let message_text = format!(
                "{guidance}\nReceived {snippet_len} characters of arguments; cut off at: …{tail}"
            );
            let error = ErrorData::new(ErrorCode::INVALID_PARAMS, message_text, None);
            messages.push(
                Message::assistant()
                    .with_tool_request(id, Err(error))
                    .with_id(message_id),
            );
        }
    }
    messages
}

/// Flag a turn the model cut off at the output token limit so consumers can
/// warn and compact, matching formats::anthropic::response_to_streaming_message.
fn output_token_limit_marker(
    stop_reason: Option<&bedrock::StopReason>,
    message_id: &str,
) -> Option<Message> {
    if stop_reason == Some(&bedrock::StopReason::MaxTokens) {
        let mut message = Message::assistant().with_id(message_id);
        message.metadata.output_token_limit_reached = true;
        Some(message)
    } else {
        None
    }
}

impl ProviderDescriptor for BedrockProvider {
    fn metadata() -> ProviderMetadata {
        let models = BEDROCK_MODEL_TABLE
            .iter()
            .map(|entry| {
                entry.context_limit.map_or_else(
                    || model_info_for_provider_model(BEDROCK_PROVIDER_NAME, entry.name),
                    |limit| ModelInfo::new(entry.name).with_context_limit(limit as usize),
                )
            })
            .collect();
        ProviderMetadata::with_models(
            BEDROCK_PROVIDER_NAME,
            "Amazon Bedrock",
            "Run models through Amazon Bedrock. Supports AWS SSO profiles - run 'aws sso login --profile <profile-name>' before using. Configure with AWS_PROFILE and AWS_REGION, use environment variables/credentials, or use AWS_BEARER_TOKEN_BEDROCK for bearer token authentication. Region is required for bearer token auth (can be set via AWS_REGION, AWS_DEFAULT_REGION, or AWS profile). Prompt caching can be enabled for Anthropic Claude models by setting BEDROCK_ENABLE_CACHING=true. Responses stream via the ConverseStream API; set BEDROCK_DISABLE_STREAMING=true to fall back to blocking Converse calls.",
            BEDROCK_DEFAULT_MODEL,
            models,
            BEDROCK_DOC_LINK,
            vec![
                ConfigKey::new("AWS_PROFILE", false, false, Some("default"), true),
                ConfigKey::new("AWS_REGION", true, false, Some("us-east-1"), true),
                ConfigKey::new("AWS_BEARER_TOKEN_BEDROCK", false, true, None, true),
                ConfigKey::new("BEDROCK_ENABLE_CACHING", false, false, Some("false"), false),
                ConfigKey::new(
                    "BEDROCK_DISABLE_STREAMING",
                    false,
                    false,
                    Some("false"),
                    false,
                ),
            ],
        )
        .with_setup(
            ProviderSetupMetadata::new(
                Model,
                CloudCredentials,
                Additional,
            )
            .with_field("AWS_REGION", "AWS Region", Some("us-west-2"), None),
        )
    }
}

impl ProviderDef for BedrockProvider {
    type Provider = Self;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<goose_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(Self::from_env(tls_config))
    }
}

#[async_trait]
impl Provider for BedrockProvider {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn retry_config(&self) -> RetryConfig {
        self.retry_config.clone()
    }

    async fn get_context_limit(&self, model: &str, override_limit: Option<usize>) -> usize {
        let configured_limits = BEDROCK_MODEL_TABLE.iter().filter_map(|entry| {
            entry
                .context_limit
                .map(|limit| (entry.name.to_string(), limit as usize))
        });
        ContextLimitResolver::new(&self.name)
            .with_configured_limits(configured_limits)
            .resolve(model, override_limit, || async { Ok(None) })
            .await
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        Ok(BEDROCK_MODEL_TABLE
            .iter()
            .map(|e| e.name.to_string())
            .collect())
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let session_id = crate::session_context::current_session_id().unwrap_or_default();
        let session_id_opt = if session_id.is_empty() {
            None
        } else {
            Some(session_id.as_str())
        };

        if let Some(entry) = find_model_entry(&model_config.model_name) {
            if entry.endpoint == BedrockEndpoint::MantleResponses {
                let capability_model_name =
                    entry.name.strip_prefix("openai.").unwrap_or(entry.name);
                let mut normalized_config = model_config.clone();
                normalized_config.model_name = capability_model_name.to_string();
                let mut payload =
                    create_responses_request(&normalized_config, system, messages, tools)?;
                payload["model"] = Value::String(entry.wire_model_id.to_string());
                payload["stream"] = Value::Bool(true);
                if entry.name.starts_with("google.gemma-4-") {
                    payload["parallel_tool_calls"] = Value::Bool(false);
                }
                let mut log = start_log(model_config, &payload).map_err(anyhow::Error::from)?;

                let response = self
                    .with_retry(|| self.post_mantle_streaming(session_id_opt, &payload))
                    .await
                    .inspect_err(|e| {
                        let _ = log.error(e);
                    })?;

                return stream_responses_compat(response, log);
            }
        }

        let model_name = model_config.model_name.clone();

        // Escape hatch: restore the previous blocking-Converse behaviour.
        if self.streaming_disabled() {
            return self
                .stream_via_converse(
                    model_config,
                    session_id_opt,
                    system,
                    messages,
                    tools,
                    &model_name,
                )
                .await;
        }

        // Open the AWS ConverseStream event stream. Retry wraps the request
        // setup only — mid-stream errors are surfaced, not retried (matching
        // the Anthropic provider's behaviour).
        let response = self
            .with_retry(|| {
                self.converse_stream(model_config, session_id_opt, system, messages, tools)
            })
            .await?;

        // Debug trace with input context; the streamed text is written once
        // the stream completes.
        let debug_payload = serde_json::json!({
            "system": system,
            "messages": messages,
            "tools": tools
        });
        let mut log = start_log(model_config, &debug_payload)?;

        let mut event_stream = response.stream;

        Ok(Box::pin(try_stream! {
            let mut state = StreamBlockState::default();
            // One id for the whole assistant turn so consumers
            // (Conversation::push) can coalesce consecutive deltas into a
            // single message — mirrors the Anthropic provider, which stamps
            // the API-provided message id on every chunk. Bedrock's
            // MessageStart event carries no id, so generate one.
            let message_id = format!("msg_{}", uuid::Uuid::new_v4());
            let mut full_text = String::new();
            let mut final_usage: Option<ProviderUsage> = None;

            loop {
                let event = event_stream.recv().await.map_err(|err| {
                    // Map Bedrock mid-stream exceptions to specific ProviderError
                    // variants so the agent's retry / context-length / server-error
                    // handling kicks in, mirroring the non-streaming Converse error
                    // mapping. Without this, a mid-stream throttling or
                    // context-length failure would be flattened to a generic
                    // RequestFailed and lose its retryable / context semantics.
                    match err.as_service_error() {
                        Some(ConverseStreamOutputError::ThrottlingException(e)) => {
                            ProviderError::RateLimitExceeded {
                                details: format!("Bedrock streaming throttling error: {:?}", e),
                                retry_delay: None,
                            }
                        }
                        Some(ConverseStreamOutputError::ValidationException(e))
                            if {
                                let msg = e.message().unwrap_or_default();
                                msg.contains("Input is too long for requested model.")
                                    || msg.contains("prompt is too long")
                            } =>
                        {
                            ProviderError::ContextLengthExceeded(format!(
                                "Bedrock streaming validation error: {:?}",
                                e
                            ))
                        }
                        Some(ConverseStreamOutputError::ServiceUnavailableException(_))
                        | Some(ConverseStreamOutputError::InternalServerException(_)) => {
                            ProviderError::ServerError(format!(
                                "Bedrock streaming server error: {:?}",
                                err
                            ))
                        }
                        Some(ConverseStreamOutputError::ModelStreamErrorException(e)) => {
                            ProviderError::ExecutionError(format!(
                                "Bedrock model stream error: {:?}",
                                e
                            ))
                        }
                        _ => ProviderError::RequestFailed(format!(
                            "Bedrock stream receive error: {:?}",
                            err
                        )),
                    }
                })?;
                let Some(event) = event else { break };

                let (messages, usage) = process_stream_event(event, &mut state, &message_id);
                if let Some(usage) = usage {
                    final_usage = Some(ProviderUsage::new(model_name.clone(), usage));
                }
                for message in messages {
                    if let Some(text) = message.content.first().and_then(|c| c.as_text()) {
                        full_text.push_str(text);
                    }
                    yield (Some(message), None);
                }
            }

            // The stream ended with tool blocks that never saw their
            // ContentBlockStop, so Bedrock cut the response off mid arguments.
            // Surface them as failed tool requests so the agent can react, and
            // flag the token limit so the CLI/ACP warning and compaction kick
            // in, mirroring the Anthropic provider.
            let stop_reason = state.stop_reason.take();
            let truncated_by_limit = stop_reason.as_ref() == Some(&bedrock::StopReason::MaxTokens);
            for message in flush_incomplete_tool_blocks(&mut state, truncated_by_limit, &message_id)
            {
                yield (Some(message), None);
            }
            if let Some(message) = output_token_limit_marker(stop_reason.as_ref(), &message_id) {
                yield (Some(message), None);
            }

            let mut usage = final_usage.unwrap_or_else(|| {
                ProviderUsage::new(model_name.clone(), Usage::default())
            });
            if let Some(reason) = stop_reason.as_ref().map(|reason| reason.as_str().to_string()) {
                usage.finish_reasons = Some(vec![reason]);
            }
            let _ = log.write(
                &serde_json::json!({ "streamed_text": full_text }),
                Some(&usage.usage),
            );
            yield (None, Some(usage));
        }))
    }
}

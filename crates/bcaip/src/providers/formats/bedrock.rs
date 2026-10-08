use crate::mcp_utils::ToolResult;
use crate::providers::bedrock::BEDROCK_PROVIDER_NAME;
use crate::utils::{sanitize_unicode_tags, strip_unicode_tags};
use anyhow::{Result, anyhow, bail};
use aws_sdk_bedrockruntime::types as bedrock;
use aws_smithy_types::{Document, Number};
use base64::Engine;
use bcaip_provider_types::conversations::Usage;
use bcaip_provider_types::conversations::{Message, MessageContent};
use bcaip_provider_types::document_format::{
    UNSUPPORTED_PROVIDER_REASON, unsupported_document_text,
};
use bcaip_provider_types::formats::ThinkingType;
use bcaip_provider_types::formats::{
    ANTHROPIC_PROVIDER_NAME, MIN_ANSWER_TOKENS, adaptive_output_effort, model_supports_temperature,
    requires_explicit_thinking_disable, thinking_block_is_stale, thinking_budget_tokens,
    thinking_type_for_provider,
};
use bcaip_provider_types::model::ModelConfig;
use bcaip_provider_types::model_mapping::maybe_get_canonical_model;
use chrono::Utc;
use once_cell::sync::Lazy;
use regex::Regex;
use rmcp::model::{
    CallToolRequestParams, ContentBlock, ErrorCode, ErrorData, ResourceContents, Role, Tool, object,
};
use serde_json::Value;
use std::{borrow::Cow, collections::HashMap, path::Path};
static BEDROCK_VERSION_SUFFIX_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"-v\d+(:\d+)?$").unwrap());

pub fn bedrock_anthropic_thinking_fields(model_config: &ModelConfig) -> Option<Document> {
    let anthropic_config = bedrock_anthropic_model_config(model_config)?;
    let thinking_type = thinking_type_for_provider(ANTHROPIC_PROVIDER_NAME, &anthropic_config);
    let thinking = match thinking_type {
        ThinkingType::Adaptive => Document::Object(HashMap::from([(
            "type".to_string(),
            Document::String("adaptive".to_string()),
        )])),
        ThinkingType::Enabled => {
            // Thinking tokens count against `maxTokens`, which `bedrock_inference_config`
            // now sends when explicitly configured. Mirror the Anthropic formatter: clamp
            // the budget to leave room for an answer, and drop thinking entirely when even
            // a minimal budget wouldn't fit under the cap. When max_tokens is unset, Bedrock
            // applies its per-model default so there is nothing to clamp against.
            let mut budget_tokens = thinking_budget_tokens(model_config);
            if let Some(max_tokens) = model_config.max_tokens {
                budget_tokens = budget_tokens.min(max_tokens.saturating_sub(MIN_ANSWER_TOKENS));
                if budget_tokens < MIN_ANSWER_TOKENS {
                    return None;
                }
            }
            Document::Object(HashMap::from([
                ("type".to_string(), Document::String("enabled".to_string())),
                (
                    "budget_tokens".to_string(),
                    Document::Number(Number::PosInt(budget_tokens as u64)),
                ),
            ]))
        }
        ThinkingType::Disabled => {
            if !requires_explicit_thinking_disable(
                ANTHROPIC_PROVIDER_NAME,
                &anthropic_config.model_name,
            ) {
                return None;
            }
            Document::Object(HashMap::from([(
                "type".to_string(),
                Document::String("disabled".to_string()),
            )]))
        }
    };

    let mut fields = HashMap::from([("thinking".to_string(), thinking)]);

    if thinking_type == ThinkingType::Adaptive {
        fields.insert(
            "output_config".to_string(),
            Document::Object(HashMap::from([(
                "effort".to_string(),
                Document::String(adaptive_output_effort(model_config).to_string()),
            )])),
        );
    }

    Some(Document::Object(fields))
}

fn bedrock_anthropic_model_config(model_config: &ModelConfig) -> Option<ModelConfig> {
    let (_, anthropic_model) = model_config.model_name.rsplit_once("anthropic.")?;

    Some(ModelConfig {
        model_name: strip_bedrock_version_suffix(anthropic_model),
        ..model_config.clone()
    })
}

/// Bedrock model ids carry a `-v1:0` style suffix (e.g.
/// `claude-opus-4-1-20250805-v1:0`) that the canonical Anthropic registry does
/// not recognise. Dropping it lets the date stamp become the terminal segment
/// the registry already knows how to normalise.
fn strip_bedrock_version_suffix(model_name: &str) -> String {
    BEDROCK_VERSION_SUFFIX_RE
        .replace(model_name, "")
        .into_owned()
}

/// Build the Bedrock `InferenceConfiguration` (`maxTokens`, `temperature`) for
/// a request from the active [`ModelConfig`].
///
/// Without this the `Converse`/`ConverseStream` APIs fall back to per-model
/// server defaults, so a configured `max_tokens`/`temperature` is silently
/// dropped. Each field is sent only when the user has configured it, so that
/// unset values continue to use Bedrock's per-model server defaults rather than
/// being pinned to a generic fallback:
/// - `max_tokens` is sent only when explicitly set (`model_config.max_tokens`).
///   Using [`ModelConfig::max_output_tokens`] here would forward its `4096`
///   fallback for every model whose id is not in the canonical catalog (e.g.
///   cross-region ids like `us.anthropic.claude-...`), capping models whose
///   real output limit is far higher.
/// - `temperature` is sent only when set and the model supports it. Support is
///   resolved against the Anthropic canonical registry for `anthropic.*` model
///   ids (the same mapping used for thinking) and the Bedrock canonical registry
///   for other known Bedrock ids, so models that reject a custom temperature keep
///   the server default.
pub fn bedrock_inference_config(model_config: &ModelConfig) -> bedrock::InferenceConfiguration {
    let mut builder = bedrock::InferenceConfiguration::builder();

    if let Some(max_tokens) = model_config.max_tokens {
        builder = builder.max_tokens(max_tokens);
    }

    if let Some(temperature) = model_config.temperature {
        if bedrock_model_supports_temperature(model_config) {
            builder = builder.temperature(temperature);
        }
    }

    builder.build()
}

/// Whether `temperature` may be sent for this Bedrock model. For `anthropic.*`
/// ids we resolve against the Anthropic canonical registry; for other known
/// Bedrock ids we consult the Bedrock canonical registry and otherwise keep the
/// permissive fallback used by [`model_supports_temperature`].
fn bedrock_model_supports_temperature(model_config: &ModelConfig) -> bool {
    if let Some(anthropic_config) = bedrock_anthropic_model_config(model_config) {
        model_supports_temperature(ANTHROPIC_PROVIDER_NAME, &anthropic_config)
    } else {
        maybe_get_canonical_model(BEDROCK_PROVIDER_NAME, &model_config.model_name)
            .and_then(|model| model.temperature)
            .unwrap_or(true)
    }
}

pub fn to_bedrock_message_with_caching(
    message: &Message,
    enable_caching: bool,
    current_model: Option<&str>,
) -> Result<bedrock::Message> {
    let thinking_is_stale = thinking_block_is_stale(message, current_model);
    let mut content_blocks: Vec<bedrock::ContentBlock> = message
        .content
        .iter()
        .filter(|content| {
            if !thinking_is_stale {
                return true;
            }
            match content {
                MessageContent::Thinking(thinking) => thinking.signature.is_empty(),
                MessageContent::RedactedThinking(_) => false,
                _ => true,
            }
        })
        .map(to_bedrock_message_content)
        .collect::<Result<_>>()?;

    if enable_caching && !content_blocks.is_empty() {
        content_blocks.push(bedrock::ContentBlock::CachePoint(
            bedrock::CachePointBlock::builder()
                .r#type(bedrock::CachePointType::Default)
                .build()
                .map_err(|e| anyhow!("Failed to build cache point for message: {}", e))?,
        ));
    }

    bedrock::Message::builder()
        .role(to_bedrock_role(&message.role))
        .set_content(Some(content_blocks))
        .build()
        .map_err(|err| anyhow!("Failed to construct Bedrock message: {}", err))
}

pub fn to_bedrock_message_content(content: &MessageContent) -> Result<bedrock::ContentBlock> {
    Ok(match content {
        MessageContent::Text(text) => bedrock::ContentBlock::Text(text.text.to_string()),
        MessageContent::ToolConfirmationRequest(_tool_confirmation_request) => {
            bedrock::ContentBlock::Text("".to_string())
        }
        MessageContent::ActionRequired(_action_required) => {
            bedrock::ContentBlock::Text("".to_string())
        }
        MessageContent::Image(image) => {
            bedrock::ContentBlock::Image(to_bedrock_image(&image.data, &image.mime_type)?)
        }
        MessageContent::Document(document) => bedrock::ContentBlock::Text(
            unsupported_document_text(document, UNSUPPORTED_PROVIDER_REASON),
        ),
        MessageContent::Thinking(thinking) => {
            let mut builder = bedrock::ReasoningTextBlock::builder().text(&thinking.thinking);
            if !thinking.signature.is_empty() {
                builder = builder.signature(&thinking.signature);
            }
            bedrock::ContentBlock::ReasoningContent(bedrock::ReasoningContentBlock::ReasoningText(
                builder.build()?,
            ))
        }
        MessageContent::RedactedThinking(redacted) => {
            match base64::prelude::BASE64_STANDARD.decode(&redacted.data) {
                Ok(bytes) => bedrock::ContentBlock::ReasoningContent(
                    bedrock::ReasoningContentBlock::RedactedContent(aws_smithy_types::Blob::new(
                        bytes,
                    )),
                ),
                Err(_) => bedrock::ContentBlock::Text("".to_string()),
            }
        }
        MessageContent::SystemNotification(_) => {
            bail!("SystemNotification should not get passed to the provider")
        }
        MessageContent::Error(_) => {
            bail!("Error content should not get passed to the provider")
        }
        MessageContent::ToolRequest(tool_req) => {
            let tool_use_id = tool_req.id.to_string();
            let tool_use = if let Ok(call) = tool_req.tool_call.as_ref() {
                bedrock::ToolUseBlock::builder()
                    .tool_use_id(tool_use_id)
                    .name(call.name.to_string())
                    .input(to_bedrock_json(&args_to_value(call.arguments.clone())))
                    .build()
            } else {
                // Unparseable tool call: emit a placeholder tool_use so the paired
                // tool_result isn't orphaned — Bedrock rejects a tool_use with no name
                // and a tool_result with no matching tool_use. Mirrors the
                // OpenAI/Databricks/Anthropic formatters.
                bedrock::ToolUseBlock::builder()
                    .tool_use_id(tool_use_id)
                    .name("unparseable_tool_call")
                    .input(to_bedrock_json(&args_to_value(None)))
                    .build()
            }?;
            bedrock::ContentBlock::ToolUse(tool_use)
        }
        MessageContent::ToolResponse(tool_res) => {
            let content = match &tool_res.tool_result {
                Ok(result) => Some(
                    result
                        .content
                        .iter()
                        .map(|c| to_bedrock_tool_result_content_block(&tool_res.id, c.clone()))
                        .collect::<Result<_>>()?,
                ),
                Err(error) => {
                    let message = format!("The tool call returned the following error:\n{}", error);
                    Some(vec![bedrock::ToolResultContentBlock::Text(
                        crate::utils::sanitize_unicode_tags(&message),
                    )])
                }
            };
            bedrock::ContentBlock::ToolResult(
                bedrock::ToolResultBlock::builder()
                    .tool_use_id(tool_res.id.to_string())
                    .status(if tool_res.tool_result.is_ok() {
                        bedrock::ToolResultStatus::Success
                    } else {
                        bedrock::ToolResultStatus::Error
                    })
                    .set_content(content)
                    .build()?,
            )
        }
    })
}

/// Convert MCP Content to Bedrock ToolResultContentBlock
///
/// Supports text, images, and document resources. Images are supported
/// by Bedrock for Anthropic Claude 3 models.
pub fn to_bedrock_tool_result_content_block(
    tool_use_id: &str,
    content: ContentBlock,
) -> Result<bedrock::ToolResultContentBlock> {
    Ok(match content {
        ContentBlock::Text(text) => bedrock::ToolResultContentBlock::Text(text.text),
        ContentBlock::Image(image) => {
            bedrock::ToolResultContentBlock::Image(to_bedrock_image(&image.data, &image.mime_type)?)
        }
        ContentBlock::ResourceLink(_link) => {
            bedrock::ToolResultContentBlock::Text("[Resource link]".to_string())
        }
        ContentBlock::Resource(resource) => match &resource.resource {
            ResourceContents::TextResourceContents { text, .. } => {
                match to_bedrock_document(tool_use_id, &resource.resource)? {
                    Some(doc) => bedrock::ToolResultContentBlock::Document(doc),
                    None => {
                        bedrock::ToolResultContentBlock::Text(sanitize_unicode_tags(text.as_str()))
                    }
                }
            }
            ResourceContents::BlobResourceContents { .. } => {
                bail!("Blob resource content is not supported by Bedrock provider yet")
            }
            _ => bail!("Unsupported resource content"),
        },
        ContentBlock::Audio(..) => bail!("Audio is not supported by Bedrock provider"),
        _ => bail!("Unsupported content"),
    })
}

pub fn to_bedrock_role(role: &Role) -> bedrock::ConversationRole {
    match role {
        Role::User => bedrock::ConversationRole::User,
        Role::Assistant => bedrock::ConversationRole::Assistant,
    }
}

pub fn to_bedrock_image(data: &str, mime_type: &str) -> Result<bedrock::ImageBlock> {
    // Extract format from MIME type
    let format = match mime_type {
        "image/png" => bedrock::ImageFormat::Png,
        "image/jpeg" | "image/jpg" => bedrock::ImageFormat::Jpeg,
        "image/gif" => bedrock::ImageFormat::Gif,
        "image/webp" => bedrock::ImageFormat::Webp,
        _ => bail!(
            "Unsupported image format: {}. Bedrock supports png, jpeg, gif, webp",
            mime_type
        ),
    };

    // Create image source with base64 data
    let source = bedrock::ImageSource::Bytes(aws_smithy_types::Blob::new(
        base64::prelude::BASE64_STANDARD
            .decode(data)
            .map_err(|e| anyhow!("Failed to decode base64 image data: {}", e))?,
    ));

    // Build the image block
    Ok(bedrock::ImageBlock::builder()
        .format(format)
        .source(source)
        .build()?)
}

pub fn to_bedrock_tool_config(tools: &[Tool]) -> Result<bedrock::ToolConfiguration> {
    Ok(bedrock::ToolConfiguration::builder()
        .set_tools(Some(
            tools.iter().map(to_bedrock_tool).collect::<Result<_>>()?,
        ))
        .build()?)
}

pub fn to_bedrock_tool(tool: &Tool) -> Result<bedrock::Tool> {
    let mut input_schema = tool.input_schema.as_ref().clone();

    // If the schema doesn't have a "type" field, add it
    // This is required by Bedrock
    if !input_schema.contains_key("type") {
        input_schema.insert("type".to_string(), Value::String("object".to_string()));
    }
    let input_schema = sanitize_json_unicode_tags(Value::Object(input_schema))?;

    Ok(bedrock::Tool::ToolSpec(
        bedrock::ToolSpecification::builder()
            .name(tool.name.to_string())
            .description(
                tool.description
                    .as_ref()
                    .map(|d| strip_unicode_tags(d))
                    .unwrap_or_default(),
            )
            .input_schema(bedrock::ToolInputSchema::Json(to_bedrock_json(
                &input_schema,
            )))
            .build()?,
    ))
}

pub(crate) fn sanitize_json_unicode_tags(value: Value) -> Result<Value> {
    Ok(match value {
        Value::String(text) => Value::String(strip_unicode_tags(&text)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(sanitize_json_unicode_tags)
                .collect::<Result<_>>()?,
        ),
        Value::Object(values) => {
            let mut sanitized = serde_json::Map::new();
            for (key, value) in values {
                let key = strip_unicode_tags(&key);
                if sanitized.contains_key(&key) {
                    bail!("JSON contains a duplicate key after Unicode tag sanitization");
                }
                sanitized.insert(key, sanitize_json_unicode_tags(value)?);
            }
            Value::Object(sanitized)
        }
        value => value,
    })
}

fn args_to_value(args: Option<serde_json::Map<String, Value>>) -> Value {
    match args {
        Some(map) => Value::Object(map),
        None => Value::Object(serde_json::Map::new()),
    }
}

pub fn to_bedrock_json(value: &Value) -> Document {
    match value {
        Value::Null => Document::Null,
        Value::Bool(bool) => Document::Bool(*bool),
        Value::Number(num) => {
            if let Some(n) = num.as_u64() {
                Document::Number(Number::PosInt(n))
            } else if let Some(n) = num.as_i64() {
                Document::Number(Number::NegInt(n))
            } else if let Some(n) = num.as_f64() {
                Document::Number(Number::Float(n))
            } else {
                unreachable!()
            }
        }
        Value::String(str) => Document::String(str.to_string()),
        Value::Array(arr) => Document::Array(arr.iter().map(to_bedrock_json).collect()),
        Value::Object(obj) => Document::Object(HashMap::from_iter(
            obj.into_iter()
                .map(|(key, val)| (key.to_string(), to_bedrock_json(val))),
        )),
    }
}

fn to_bedrock_document(
    tool_use_id: &str,
    content: &ResourceContents,
) -> Result<Option<bedrock::DocumentBlock>> {
    let (uri, text) = match content {
        ResourceContents::TextResourceContents { uri, text, .. } => {
            (uri, sanitize_unicode_tags(text))
        }
        ResourceContents::BlobResourceContents { .. } => {
            bail!("Blob resource content is not supported by Bedrock provider yet")
        }
        _ => bail!("Unsupported resource content"),
    };

    let filename = Path::new(uri)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or(uri);

    // Return None if the file type is not supported
    let (name, format) = match filename.split_once('.') {
        Some((name, "txt")) => (name, bedrock::DocumentFormat::Txt),
        Some((name, "csv")) => (name, bedrock::DocumentFormat::Csv),
        Some((name, "md")) => (name, bedrock::DocumentFormat::Md),
        _ => return Ok(None), // Not a supported document type
    };

    // Since we can't use the full path (due to character limit and also Bedrock does not accept `/` etc.),
    // and Bedrock wants document names to be unique, we're adding `tool_use_id` as a prefix to make
    // document names unique
    let name = format!("{tool_use_id}-{name}");

    Ok(Some(
        bedrock::DocumentBlock::builder()
            .format(format)
            .name(name)
            .source(bedrock::DocumentSource::Bytes(text.as_bytes().into()))
            .build()
            .map_err(|err| anyhow!("Failed to construct Bedrock document: {}", err))?,
    ))
}

pub fn from_bedrock_message(message: &bedrock::Message) -> Result<Message> {
    let role = from_bedrock_role(message.role())?;
    let content = message
        .content()
        .iter()
        .filter(|block| !matches!(block, bedrock::ContentBlock::CachePoint(_)))
        .map(from_bedrock_content_block)
        .collect::<Result<Vec<_>>>()?;
    let created = Utc::now().timestamp();

    Ok(Message::new(role, created, content))
}

pub fn from_bedrock_content_block(block: &bedrock::ContentBlock) -> Result<MessageContent> {
    Ok(match block {
        bedrock::ContentBlock::Text(text) => MessageContent::text(text),
        bedrock::ContentBlock::ToolUse(tool_use) => {
            let arguments = from_bedrock_json(&tool_use.input.clone())
                .and_then(sanitize_json_unicode_tags)
                .map(|arguments| {
                    CallToolRequestParams::new(tool_use.name.clone())
                        .with_arguments(object(arguments))
                })
                .map_err(|error| {
                    ErrorData::new(ErrorCode::INVALID_PARAMS, error.to_string(), None)
                });
            MessageContent::tool_request(tool_use.tool_use_id.to_string(), arguments)
        }
        bedrock::ContentBlock::ToolResult(tool_res) => MessageContent::tool_response(
            tool_res.tool_use_id.to_string(),
            if tool_res.content.is_empty() {
                Err(ErrorData {
                    code: ErrorCode::INTERNAL_ERROR,
                    message: Cow::from("Empty content for tool use from Bedrock".to_string()),
                    data: None,
                })
            } else {
                tool_res
                    .content
                    .iter()
                    .map(from_bedrock_tool_result_content_block)
                    .collect::<ToolResult<Vec<_>>>()
                    .map(rmcp::model::CallToolResult::success)
            },
        ),
        bedrock::ContentBlock::ReasoningContent(reasoning) => {
            from_bedrock_reasoning_content_block(reasoning)?
        }
        bedrock::ContentBlock::CachePoint(_) => {
            bail!("CachePoint blocks should have been filtered out during message processing")
        }
        _ => bail!(
            "Unsupported Bedrock content block type: {}",
            bedrock_content_block_kind(block)
        ),
    })
}

fn from_bedrock_reasoning_content_block(
    reasoning: &bedrock::ReasoningContentBlock,
) -> Result<MessageContent> {
    Ok(match reasoning {
        bedrock::ReasoningContentBlock::ReasoningText(text_block) => {
            let signature = text_block.signature.clone().unwrap_or_default();
            MessageContent::thinking(text_block.text.clone(), signature)
        }
        bedrock::ReasoningContentBlock::RedactedContent(blob) => {
            let encoded = base64::prelude::BASE64_STANDARD.encode(blob.as_ref());
            MessageContent::redacted_thinking(encoded)
        }
        _ => bail!(
            "Unsupported Bedrock reasoning content variant: {}",
            bedrock_reasoning_content_block_kind(reasoning)
        ),
    })
}

fn bedrock_reasoning_content_block_kind(block: &bedrock::ReasoningContentBlock) -> &'static str {
    match block {
        bedrock::ReasoningContentBlock::ReasoningText(_) => "ReasoningText",
        bedrock::ReasoningContentBlock::RedactedContent(_) => "RedactedContent",
        _ => "Unknown",
    }
}

fn bedrock_content_block_kind(block: &bedrock::ContentBlock) -> &'static str {
    match block {
        bedrock::ContentBlock::Audio(_) => "Audio",
        bedrock::ContentBlock::CachePoint(_) => "CachePoint",
        bedrock::ContentBlock::CitationsContent(_) => "CitationsContent",
        bedrock::ContentBlock::Document(_) => "Document",
        bedrock::ContentBlock::GuardContent(_) => "GuardContent",
        bedrock::ContentBlock::Image(_) => "Image",
        bedrock::ContentBlock::ReasoningContent(_) => "ReasoningContent",
        bedrock::ContentBlock::SearchResult(_) => "SearchResult",
        bedrock::ContentBlock::Text(_) => "Text",
        bedrock::ContentBlock::ToolResult(_) => "ToolResult",
        bedrock::ContentBlock::ToolUse(_) => "ToolUse",
        bedrock::ContentBlock::Video(_) => "Video",
        _ => "Unknown",
    }
}

pub fn from_bedrock_tool_result_content_block(
    content: &bedrock::ToolResultContentBlock,
) -> ToolResult<ContentBlock> {
    Ok(match content {
        bedrock::ToolResultContentBlock::Text(text) => ContentBlock::text(text.to_string()),
        _ => {
            return Err(ErrorData {
                code: ErrorCode::INTERNAL_ERROR,
                message: Cow::from("Unsupported tool result from Bedrock".to_string()),
                data: None,
            });
        }
    })
}

pub fn from_bedrock_role(role: &bedrock::ConversationRole) -> Result<Role> {
    Ok(match role {
        bedrock::ConversationRole::User => Role::User,
        bedrock::ConversationRole::Assistant => Role::Assistant,
        _ => bail!("Unknown role from Bedrock"),
    })
}

pub fn from_bedrock_usage(usage: &bedrock::TokenUsage) -> Usage {
    Usage::from_cache_exclusive_input(
        Some(usage.input_tokens),
        Some(usage.output_tokens),
        Some(usage.total_tokens),
        usage.cache_read_input_tokens,
        usage.cache_write_input_tokens,
    )
}

pub fn from_bedrock_json(document: &Document) -> Result<Value> {
    Ok(match document {
        Document::Null => Value::Null,
        Document::Bool(bool) => Value::Bool(*bool),
        Document::Number(num) => match num {
            Number::PosInt(i) => Value::Number((*i).into()),
            Number::NegInt(i) => Value::Number((*i).into()),
            Number::Float(f) => Value::Number(
                serde_json::Number::from_f64(*f).ok_or(anyhow!("Expected a valid float"))?,
            ),
        },
        Document::String(str) => Value::String(str.clone()),
        Document::Array(arr) => {
            Value::Array(arr.iter().map(from_bedrock_json).collect::<Result<_>>()?)
        }
        Document::Object(obj) => Value::Object(
            obj.iter()
                .map(|(key, val)| Ok((key.clone(), from_bedrock_json(val)?)))
                .collect::<Result<_>>()?,
        ),
    })
}

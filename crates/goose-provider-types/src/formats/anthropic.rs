use crate::canonical::ThinkingMode;
use crate::conversations::{Message, MessageContentBlock, CostSource, ProviderUsage, Usage};
use crate::document_format::{
    ASSISTANT_ROLE_REASON, DocumentFormat, UNSUPPORTED_MEDIA_TYPE_REASON, convert_document,
    document_media_type_is_supported, unsupported_document_text,
};
use crate::errors::ProviderError;
use crate::images::{ImageFormat, convert_image};
use crate::json;
use crate::maybe_send::MaybeSend;
use crate::mcp_utils::extract_text_from_resource;
use crate::model::ModelConfig;
use crate::model_mapping::maybe_get_canonical_model;
use crate::thinking::ThinkingEffort;
use anyhow::{Result, anyhow};
use rmcp::model::{
    CallToolRequestParams, ContentBlock, ErrorCode, ErrorData, JsonObject, ResourceContents, Role,
    Tool, object,
};
use rmcp::object as json_object;
use serde_json::{Map, Value, json};
use std::collections::HashSet;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;

pub const ANTHROPIC_PROVIDER_NAME: &str = "anthropic";

macro_rules! string_enum {
    ($name:ident { $($variant:ident => $str:literal),+ $(,)? }) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum $name { $($variant),+ }

        impl FromStr for $name {
            type Err = String;
            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s.to_lowercase().as_str() {
                    $($str => Ok(Self::$variant),)+
                    other => Err(format!("unknown {}: '{other}'", stringify!($name))),
                }
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self { $(Self::$variant => write!(f, $str),)+ }
            }
        }
    }
}

string_enum!(ThinkingType { Adaptive => "adaptive", Enabled => "enabled", Disabled => "disabled" });
string_enum!(CacheTtl { FiveMinutes => "5m", OneHour => "1h" });

string_enum!(PrefixMismatchBehavior { DropBlock => "drop_block", Error => "error" });

pub const THINKING_BINDING_CONTROLS_BETA: &str = "thinking-binding-controls-2026-08-01";
pub const INPUT_TRANSFORMATIONS_FIELD: &str = "input_transformations";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AnthropicFormatOptions {
    pub preserve_unsigned_thinking: bool,
    pub preserve_thinking_context: bool,
    pub thinking_disabled: bool,
    pub emit_clear_thinking: bool,
    pub current_model: Option<String>,
    pub prompt_cache_disabled: bool,
    pub cache_ttl: Option<CacheTtl>,
    pub prefix_mismatch_behavior: Option<PrefixMismatchBehavior>,
    pub strip_thinking_history: bool,
}

impl AnthropicFormatOptions {
    /// Anthropic-compatible providers keep `Default`, which does not request block binding.
    pub fn native() -> Self {
        Self {
            prefix_mismatch_behavior: Some(PrefixMismatchBehavior::DropBlock),
            ..Self::default()
        }
    }

    fn for_model(self, provider_name: &str, model_config: &ModelConfig) -> Self {
        let preserve_thinking_context = model_config
            .request_param::<bool>("preserve_thinking_context")
            .unwrap_or(self.preserve_thinking_context);
        let preserve_unsigned_thinking = model_config
            .request_param::<bool>("preserve_unsigned_thinking")
            .unwrap_or(self.preserve_unsigned_thinking)
            || preserve_thinking_context;
        let always_on = canonical_thinking_mode(provider_name, &model_config.model_name)
            == Some(ThinkingMode::AlwaysOnAdaptive);
        let thinking_disabled = !always_on
            && (model_config.reasoning == Some(false)
                || model_config.thinking_effort() == Some(ThinkingEffort::Off));
        let emit_clear_thinking = model_config
            .request_param::<bool>("emit_clear_thinking")
            .unwrap_or(self.emit_clear_thinking);
        let cache_ttl = model_config
            .cache_ttl()
            .and_then(|ttl| ttl.parse::<CacheTtl>().ok())
            .or(self.cache_ttl);
        let prefix_mismatch_behavior = match model_config
            .request_param::<String>("prefix_mismatch_behavior")
            .as_deref()
        {
            None => self.prefix_mismatch_behavior,
            Some("off") => None,
            Some(value) => value.parse().ok().or(self.prefix_mismatch_behavior),
        };

        Self {
            preserve_unsigned_thinking,
            preserve_thinking_context,
            thinking_disabled,
            emit_clear_thinking,
            current_model: self
                .current_model
                .or_else(|| Some(model_config.model_name.clone())),
            prompt_cache_disabled: model_config.prompt_cache_disabled(),
            cache_ttl,
            prefix_mismatch_behavior,
            strip_thinking_history: self.strip_thinking_history,
        }
    }

    /// `{"type":"ephemeral"}` selects Anthropic's default 5m TTL; the `ttl`
    /// field is only sent for an explicit 1h opt-in, since a 1h write is
    /// billed at 2x input instead of 1.25x.
    fn cache_control(&self) -> Value {
        match self.cache_ttl {
            Some(CacheTtl::OneHour) => {
                json!({ TYPE_FIELD: "ephemeral", "ttl": "1h" })
            }
            _ => json!({ TYPE_FIELD: "ephemeral" }),
        }
    }
}

pub fn thinking_block_is_stale(message: &Message, current_model: Option<&str>) -> bool {
    let Some(current_model) = current_model else {
        return false;
    };
    let Some(inference) = message.metadata.inference.as_ref() else {
        return false;
    };
    let requested = inference.requested_model.as_str();
    let resolved = inference.resolved_model.as_deref().unwrap_or("");
    if requested.is_empty() && resolved.is_empty() {
        return false;
    }
    current_model != requested && current_model != resolved
}

fn canonical_thinking_mode(provider_name: &str, model_name: &str) -> Option<ThinkingMode> {
    maybe_get_canonical_model(provider_name, model_name)
        .and_then(|model| model.thinking_mode)
        .or_else(|| provider_thinking_mode(provider_name, model_name))
}

/// Models that always reason when the canonical entry has no thinking mode.
/// Muse Spark rejects `thinking: disabled` and ignores `budget_tokens`.
fn provider_thinking_mode(provider_name: &str, model_name: &str) -> Option<ThinkingMode> {
    if provider_name == "muse_code" && model_name.starts_with("muse-spark") {
        return Some(ThinkingMode::AlwaysOnAdaptive);
    }
    None
}

/// Adaptive models run adaptive thinking when `thinking` is omitted, so turning
/// it off takes an explicit disable. Always-on models reject that disable.
pub fn requires_explicit_thinking_disable(provider_name: &str, model_name: &str) -> bool {
    canonical_thinking_mode(provider_name, model_name) == Some(ThinkingMode::Adaptive)
}

fn canonical_reasoning(provider_name: &str, model_config: &ModelConfig) -> Option<bool> {
    maybe_get_canonical_model(provider_name, &model_config.model_name)
        .and_then(|model| model.reasoning)
}

pub fn model_supports_temperature(provider_name: &str, model_config: &ModelConfig) -> bool {
    maybe_get_canonical_model(provider_name, &model_config.model_name)
        .and_then(|model| model.temperature)
        .unwrap_or(true)
}

pub fn thinking_type(model_config: &ModelConfig) -> ThinkingType {
    thinking_type_for_provider(ANTHROPIC_PROVIDER_NAME, model_config)
}

pub fn thinking_type_for_provider(provider_name: &str, model_config: &ModelConfig) -> ThinkingType {
    let mode = canonical_thinking_mode(provider_name, &model_config.model_name);
    let reasoning = model_config
        .reasoning
        .or_else(|| canonical_reasoning(provider_name, model_config));

    if reasoning != Some(true) {
        return ThinkingType::Disabled;
    }

    if mode == Some(ThinkingMode::AlwaysOnAdaptive) {
        return ThinkingType::Adaptive;
    }

    let effort = model_config.thinking_effort();

    if effort.is_none() && model_config.request_param::<i32>("budget_tokens").is_some() {
        return match mode {
            Some(ThinkingMode::Adaptive) => ThinkingType::Adaptive,
            _ => ThinkingType::Enabled,
        };
    }

    match effort.unwrap_or(ThinkingEffort::Off) {
        ThinkingEffort::Off => ThinkingType::Disabled,
        _ if mode == Some(ThinkingMode::Adaptive) => ThinkingType::Adaptive,
        _ => ThinkingType::Enabled,
    }
}

// Constants for frequently used strings in Anthropic API format
const TYPE_FIELD: &str = "type";
const CONTENT_FIELD: &str = "content";
const TEXT_TYPE: &str = "text";
const ROLE_FIELD: &str = "role";
const USER_ROLE: &str = "user";
const ASSISTANT_ROLE: &str = "assistant";
const TOOL_USE_TYPE: &str = "tool_use";
const TOOL_RESULT_TYPE: &str = "tool_result";
const THINKING_TYPE: &str = "thinking";
const REDACTED_THINKING_TYPE: &str = "redacted_thinking";
const CACHE_CONTROL_FIELD: &str = "cache_control";
const ID_FIELD: &str = "id";
const NAME_FIELD: &str = "name";
const INPUT_FIELD: &str = "input";
const TOOL_USE_ID_FIELD: &str = "tool_use_id";
const IS_ERROR_FIELD: &str = "is_error";
const SIGNATURE_FIELD: &str = "signature";
const DATA_FIELD: &str = "data";
const IMAGE_TYPE: &str = "image";
const DOCUMENT_TYPE: &str = "document";
const SOURCE_FIELD: &str = "source";
const BASE64_TYPE: &str = "base64";
const MEDIA_TYPE_FIELD: &str = "media_type";
// Claude vision only accepts these image media types; other image/* blobs fall
// through to the text/binary-marker path so an unsupported type (e.g.
// image/svg+xml) doesn't turn the next request into a provider rejection.
const ANTHROPIC_IMAGE_MEDIA_TYPES: [&str; 4] =
    ["image/jpeg", "image/png", "image/gif", "image/webp"];
const EVENT_MESSAGE_START: &str = "message_start";
const EVENT_MESSAGE_DELTA: &str = "message_delta";
const EVENT_MESSAGE_STOP: &str = "message_stop";
const EVENT_CONTENT_BLOCK_START: &str = "content_block_start";
const EVENT_CONTENT_BLOCK_DELTA: &str = "content_block_delta";
const EVENT_CONTENT_BLOCK_STOP: &str = "content_block_stop";
const STOP_REASON_REFUSAL: &str = "refusal";
const REFUSAL_FALLBACK_DETAILS: &str = "No additional details were provided.";

/// Coerce a tool call's optional arguments into the JSON value Anthropic
/// expects for the `input` field of a `tool_use` content block.
///
/// Anthropic's Messages API requires `input` to be an object. When the
/// internal `CallToolRequestParams::arguments` is `None` (which happens for
/// parameterless tools, tool calls round-tripped from disk, or calls created
/// via `CallToolRequestParams::new` without `.with_arguments(...)`) the
/// `json!` macro would otherwise serialize it as JSON `null` and the API
/// rejects the next replay of the tool_use block with a 400 error:
/// `messages.<N>.content.<M>.tool_use.input: Input should be an object.`
/// See issue #9287.
fn args_to_input_value(arguments: Option<JsonObject>) -> Value {
    Value::Object(arguments.unwrap_or_default())
}

/// Convert internal Message format to Anthropic's API message specification
pub fn format_messages_anthropic(messages: &[Message]) -> Vec<Value> {
    format_messages_with_options(messages, &AnthropicFormatOptions::default())
}

fn format_messages_with_options(
    messages: &[Message],
    options: &AnthropicFormatOptions,
) -> Vec<Value> {
    let mut anthropic_messages = Vec::new();

    for message in messages {
        let role = match message.role {
            Role::User => USER_ROLE,
            Role::Assistant => ASSISTANT_ROLE,
        };

        let thinking_is_stale = thinking_block_is_stale(message, options.current_model.as_deref());
        let replay_thinking =
            !options.thinking_disabled && !options.strip_thinking_history && !thinking_is_stale;

        let mut content = Vec::new();
        for msg_content in &message.content {
            match msg_content {
                MessageContentBlock::Text(text) => {
                    if !text.text.trim().is_empty() {
                        content.push(json!({
                            TYPE_FIELD: TEXT_TYPE,
                            TEXT_TYPE: text.text
                        }));
                    }
                }
                MessageContentBlock::ToolRequest(tool_request) => {
                    match &tool_request.tool_call {
                        Ok(tool_call) => {
                            content.push(json!({
                                TYPE_FIELD: TOOL_USE_TYPE,
                                ID_FIELD: tool_request.id,
                                NAME_FIELD: tool_call.name,
                                INPUT_FIELD: args_to_input_value(tool_call.arguments.clone())
                            }));
                        }
                        Err(_tool_error) => {
                            // The paired tool response carries the parse error and
                            // serializes to a tool_result below; Anthropic rejects a
                            // tool_result without a preceding tool_use, so emit a
                            // placeholder tool_use with the same id to keep history valid.
                            content.push(json!({
                                TYPE_FIELD: TOOL_USE_TYPE,
                                ID_FIELD: tool_request.id,
                                NAME_FIELD: "unparseable_tool_call",
                                INPUT_FIELD: json!({})
                            }));
                        }
                    }
                }
                MessageContentBlock::ToolResponse(tool_response) => {
                    match &tool_response.tool_result {
                        Ok(result) => {
                            let mut blocks: Vec<Value> = Vec::new();
                            let mut text_parts: Vec<String> = Vec::new();
                            let mut has_media = false;

                            for c in result.content.iter() {
                                if let Some(t) = c.as_text() {
                                    text_parts.push(t.text.clone());
                                    if !t.text.is_empty() {
                                        blocks.push(json!({
                                            TYPE_FIELD: TEXT_TYPE,
                                            TEXT_TYPE: t.text.clone()
                                        }));
                                    }
                                    continue;
                                }
                                if let Some(r) = c.as_resource() {
                                    // Claude only accepts a fixed set of media types, so
                                    // unsupported blobs fall back to text below rather than
                                    // being rejected by the provider.
                                    if let ResourceContents::BlobResourceContents {
                                        blob,
                                        mime_type,
                                        ..
                                    } = &r.resource
                                    {
                                        let mime = mime_type.as_deref().unwrap_or("");
                                        if ANTHROPIC_IMAGE_MEDIA_TYPES.contains(&mime) {
                                            has_media = true;
                                            blocks.push(json!({
                                                TYPE_FIELD: IMAGE_TYPE,
                                                SOURCE_FIELD: {
                                                    TYPE_FIELD: BASE64_TYPE,
                                                    MEDIA_TYPE_FIELD: mime,
                                                    DATA_FIELD: blob,
                                                }
                                            }));
                                            continue;
                                        }
                                        if mime == "application/pdf" {
                                            has_media = true;
                                            blocks.push(json!({
                                                TYPE_FIELD: DOCUMENT_TYPE,
                                                SOURCE_FIELD: {
                                                    TYPE_FIELD: BASE64_TYPE,
                                                    MEDIA_TYPE_FIELD: mime,
                                                    DATA_FIELD: blob,
                                                }
                                            }));
                                            continue;
                                        }
                                    }
                                    let text = extract_text_from_resource(&r.resource);
                                    if !text.is_empty() {
                                        text_parts.push(text.clone());
                                        blocks.push(json!({
                                            TYPE_FIELD: TEXT_TYPE,
                                            TEXT_TYPE: text
                                        }));
                                    }
                                    continue;
                                }
                                if let ContentBlock::Image(image) = c {
                                    if ANTHROPIC_IMAGE_MEDIA_TYPES
                                        .contains(&image.mime_type.as_str())
                                    {
                                        has_media = true;
                                        blocks.push(convert_image(
                                            &image.clone(),
                                            &ImageFormat::Anthropic,
                                        ));
                                    } else {
                                        let marker = format!("[Image: {}]", image.mime_type);
                                        text_parts.push(marker.clone());
                                        blocks.push(json!({
                                            TYPE_FIELD: TEXT_TYPE,
                                            TEXT_TYPE: marker
                                        }));
                                    }
                                }
                            }

                            let content_value = if has_media {
                                Value::Array(blocks)
                            } else {
                                Value::String(text_parts.join("\n"))
                            };

                            content.push(json!({
                                TYPE_FIELD: TOOL_RESULT_TYPE,
                                TOOL_USE_ID_FIELD: tool_response.id,
                                CONTENT_FIELD: content_value
                            }));
                        }
                        Err(tool_error) => {
                            content.push(json!({
                                TYPE_FIELD: TOOL_RESULT_TYPE,
                                TOOL_USE_ID_FIELD: tool_response.id,
                                CONTENT_FIELD: format!("Error: {}", tool_error),
                                IS_ERROR_FIELD: true
                            }));
                        }
                    }
                }
                MessageContentBlock::ToolConfirmationRequest(_tool_confirmation_request) => {
                    // Skip tool confirmation requests
                }
                MessageContentBlock::ActionRequired(_action_required) => {
                    // Skip action required messages - they're for UI only
                }
                MessageContentBlock::SystemNotification(_) | MessageContentBlock::Error(_) => {
                    // Skip
                }
                MessageContentBlock::Thinking(thinking) => {
                    // Anthropic rejects thinking blocks sent without a matching thinking config.
                    if replay_thinking {
                        if !thinking.signature.is_empty() {
                            content.push(json!({
                                TYPE_FIELD: THINKING_TYPE,
                                THINKING_TYPE: thinking.thinking,
                                SIGNATURE_FIELD: thinking.signature
                            }));
                        } else if options.preserve_unsigned_thinking
                            && !thinking.thinking.is_empty()
                        {
                            content.push(json!({
                                TYPE_FIELD: THINKING_TYPE,
                                THINKING_TYPE: thinking.thinking
                            }));
                        }
                    }
                }
                MessageContentBlock::RedactedThinking(redacted) => {
                    if replay_thinking {
                        content.push(json!({
                            TYPE_FIELD: REDACTED_THINKING_TYPE,
                            DATA_FIELD: redacted.data
                        }));
                    }
                }
                MessageContentBlock::Image(image) => {
                    content.push(convert_image(image, &ImageFormat::Anthropic));
                }
                MessageContentBlock::Document(document) => {
                    if message.role != Role::User {
                        content.push(json!({
                            TYPE_FIELD: TEXT_TYPE,
                            TEXT_TYPE: unsupported_document_text(document, ASSISTANT_ROLE_REASON)
                        }));
                    } else if document_media_type_is_supported(&document.mime_type) {
                        content.push(convert_document(document, &DocumentFormat::Anthropic));
                    } else {
                        content.push(json!({
                            TYPE_FIELD: TEXT_TYPE,
                            TEXT_TYPE: unsupported_document_text(
                                document,
                                UNSUPPORTED_MEDIA_TYPE_REASON,
                            )
                        }));
                    }
                }
            }
        }

        // Skip messages with empty content
        if !content.is_empty() {
            anthropic_messages.push(json!({
                ROLE_FIELD: role,
                CONTENT_FIELD: content
            }));
        }
    }

    if anthropic_messages.is_empty() {
        anthropic_messages.push(json!({
            ROLE_FIELD: USER_ROLE,
            CONTENT_FIELD: [{
                TYPE_FIELD: TEXT_TYPE,
                TEXT_TYPE: "Ignore"
            }]
        }));
    }

    if options.prompt_cache_disabled {
        return anthropic_messages;
    }

    // The last two user messages extend the cached prefix each turn.
    let mut user_count = 0;
    for message in anthropic_messages.iter_mut().rev() {
        if message.get(ROLE_FIELD) != Some(&json!(USER_ROLE)) {
            continue;
        }
        if let Some(block) = message
            .get_mut(CONTENT_FIELD)
            .and_then(|content| content.as_array_mut())
            .and_then(|content_array| content_array.last_mut())
            .and_then(|b| b.as_object_mut())
        {
            block.insert(CACHE_CONTROL_FIELD.to_string(), options.cache_control());
            user_count += 1;
            if user_count >= 2 {
                break;
            }
        }
    }

    anthropic_messages
}

fn anthropic_flavored_input_schema(input_schema: Arc<JsonObject>) -> Arc<JsonObject> {
    if input_schema.is_empty() {
        return Arc::new(json_object!({
            "type": "object",
        }));
    }
    input_schema
}

/// Convert internal Tool format to Anthropic's API tool specification
pub fn format_tools(tools: &[Tool], options: &AnthropicFormatOptions) -> Vec<Value> {
    let mut unique_tools = HashSet::new();
    let mut tool_specs = Vec::new();

    for tool in tools {
        if unique_tools.insert(tool.name.clone()) {
            tool_specs.push(json!({
                NAME_FIELD: tool.name,
                "description": tool.description,
                "input_schema": anthropic_flavored_input_schema(tool.input_schema.clone())
            }));
        }
    }

    if options.prompt_cache_disabled {
        return tool_specs;
    }

    // Add "cache_control" to the last tool spec, if any. This means that all tool definitions,
    // will be cached as a single prefix.
    if let Some(last_tool) = tool_specs.last_mut() {
        last_tool
            .as_object_mut()
            .unwrap()
            .insert(CACHE_CONTROL_FIELD.to_string(), options.cache_control());
    }

    tool_specs
}

/// Convert system message to Anthropic's API system specification
pub fn format_system(system: &str, options: &AnthropicFormatOptions) -> Value {
    if options.prompt_cache_disabled {
        return json!([{
            TYPE_FIELD: TEXT_TYPE,
            TEXT_TYPE: system
        }]);
    }
    json!([{
        TYPE_FIELD: TEXT_TYPE,
        TEXT_TYPE: system,
        CACHE_CONTROL_FIELD: options.cache_control()
    }])
}

/// Convert Anthropic's API response to internal Message format
pub fn response_to_message_anthropic(response: &Value) -> Result<Message> {
    let content_blocks = response
        .get(CONTENT_FIELD)
        .and_then(|c| c.as_array())
        .ok_or_else(|| anyhow!("Invalid response format: missing content array"))?;

    let mut message = Message::assistant();

    for block in content_blocks {
        match block.get(TYPE_FIELD).and_then(|t| t.as_str()) {
            Some(TEXT_TYPE) => {
                if let Some(text) = block.get(TEXT_TYPE).and_then(|t| t.as_str()) {
                    message = message.with_text(text.to_string());
                }
            }
            Some(TOOL_USE_TYPE) => {
                let id = block
                    .get(ID_FIELD)
                    .and_then(|i| i.as_str())
                    .ok_or_else(|| anyhow!("Missing tool_use id"))?;
                let name = block
                    .get(NAME_FIELD)
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| anyhow!("Missing tool_use name"))?
                    .to_string();
                let input = block
                    .get(INPUT_FIELD)
                    .ok_or_else(|| anyhow!("Missing tool_use input"))?;

                let tool_call =
                    CallToolRequestParams::new(name).with_arguments(object(input.clone()));
                message = message.with_tool_request(id, Ok(tool_call));
            }
            Some(THINKING_TYPE) => {
                let thinking = block
                    .get(THINKING_TYPE)
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| anyhow!("Missing thinking content"))?
                    .to_string();
                let signature = block
                    .get(SIGNATURE_FIELD)
                    .and_then(|s| s.as_str())
                    .unwrap_or_default();
                message = message.with_thinking(thinking, signature);
            }
            Some(REDACTED_THINKING_TYPE) => {
                let data = block
                    .get(DATA_FIELD)
                    .and_then(|d| d.as_str())
                    .ok_or_else(|| anyhow!("Missing redacted_thinking data"))?;
                message = message.with_redacted_thinking(data);
            }
            _ => continue,
        }
    }

    Ok(message)
}

fn usage_from_anthropic_fields(usage: &Value) -> Usage {
    let field = |key: &str| {
        usage
            .get(key)
            .and_then(|v| v.as_u64())
            .map(|v| v.min(i32::MAX as u64) as i32)
    };

    Usage::from_cache_exclusive_input(
        Some(field("input_tokens").unwrap_or(0)),
        Some(field("output_tokens").unwrap_or(0)),
        None,
        field("cache_read_input_tokens"),
        field("cache_creation_input_tokens"),
    )
}

/// Merge a `message_delta` usage into the usage captured at `message_start`.
/// Delta usage is cumulative (input grows during server tool use), so fields
/// present in the raw delta payload win over the start values.
fn merge_delta_usage(existing: &Usage, delta: &Usage, delta_data: &Value) -> Usage {
    let reports = |key: &str| delta_data.get(key).is_some();

    let output = if reports("output_tokens") {
        delta.output_tokens
    } else {
        existing.output_tokens
    };

    if !reports("input_tokens") {
        Usage::new(existing.input_tokens, output, None).with_cache_tokens(
            existing.cache_read_input_tokens,
            existing.cache_write_input_tokens,
        )
    } else if reports("cache_read_input_tokens") || reports("cache_creation_input_tokens") {
        Usage::new(delta.input_tokens, output, None).with_cache_tokens(
            delta.cache_read_input_tokens,
            delta.cache_write_input_tokens,
        )
    } else {
        Usage::from_cache_exclusive_input(
            delta.input_tokens,
            output,
            None,
            existing.cache_read_input_tokens,
            existing.cache_write_input_tokens,
        )
    }
}

pub fn get_usage_anthropic(data: &Value) -> Result<Usage> {
    if let Some(usage) = data.get("usage") {
        Ok(usage_from_anthropic_fields(usage))
    } else if data.as_object().is_some() {
        // Check if the data itself is the usage object (for message_delta events that might have usage at top level)
        let usage = usage_from_anthropic_fields(data);
        if usage.total_tokens.unwrap_or(0) > 0 {
            Ok(usage)
        } else {
            tracing::debug!("🔍 Anthropic no token data found in object");
            Ok(Usage::new(None, None, None))
        }
    } else {
        tracing::debug!(
            "Failed to get usage data: {}",
            ProviderError::UsageError("No usage data found in response".to_string())
        );
        // If no usage data, return None for all values
        Ok(Usage::new(None, None, None))
    }
}

/// Anthropic response fields that have no canonical `ProviderUsage` equivalent.
const ADDITIONAL_USAGE_FIELDS: [&str; 1] = ["service_tier"];

pub fn input_transformations(message_data: &Value) -> Option<Value> {
    let transformations = message_data.get(INPUT_TRANSFORMATIONS_FIELD)?.as_array()?;
    let dropped: Vec<(&str, &str)> = transformations
        .iter()
        .filter(|t| t.get("type").and_then(|v| v.as_str()) == Some("thinking_dropped"))
        .map(|t| {
            (
                t.get("path").and_then(Value::as_str).unwrap_or(""),
                t.get("reason").and_then(Value::as_str).unwrap_or(""),
            )
        })
        .collect();
    if !dropped.is_empty() {
        tracing::warn!(?dropped, "API dropped thinking blocks from the request");
    }
    Some(Value::Array(transformations.clone()))
}

pub fn get_additional_data(data: &Value) -> Option<Map<String, Value>> {
    let usage = data.get("usage")?.as_object()?;
    let additional: Map<String, Value> = ADDITIONAL_USAGE_FIELDS
        .iter()
        .filter_map(|field| Some(((*field).to_string(), usage.get(*field)?.clone())))
        .collect();
    (!additional.is_empty()).then_some(additional)
}

fn provider_usage_with_cost(
    model: String,
    usage: Usage,
    data: &Value,
    fallback_cost: Option<f64>,
) -> ProviderUsage {
    let provider_usage = ProviderUsage::new(model, usage);
    match super::openai::get_cost(data).or(fallback_cost) {
        Some(cost) => provider_usage.with_cost(cost, CostSource::ProviderReported),
        None => provider_usage,
    }
}

pub fn thinking_effort(model_config: &ModelConfig) -> ThinkingEffort {
    model_config
        .thinking_effort()
        .unwrap_or(ThinkingEffort::High)
}

fn adaptive_effort_wire(provider_name: &str, model_config: &ModelConfig) -> String {
    let effort = adaptive_output_effort(model_config);
    // Meta Messages accepts low, medium, high, and xhigh. goose's max maps to xhigh.
    if provider_name == "muse_code" && effort == ThinkingEffort::Max {
        return "xhigh".to_string();
    }
    effort.to_string()
}

pub fn adaptive_output_effort(model_config: &ModelConfig) -> ThinkingEffort {
    match thinking_effort(model_config) {
        ThinkingEffort::Off => ThinkingEffort::High,
        effort => effort,
    }
}

pub fn thinking_budget_tokens(model_config: &ModelConfig) -> i32 {
    if let Some(request_param) = model_config
        .request_params
        .as_ref()
        .and_then(|params| params.get("budget_tokens"))
        .and_then(|v| serde_json::from_value::<i32>(v.clone()).ok())
    {
        return request_param.max(1024);
    }

    let effort = model_config
        .thinking_effort()
        .unwrap_or(ThinkingEffort::High);
    match effort {
        ThinkingEffort::Off => 1024,
        ThinkingEffort::Low => 4000,
        ThinkingEffort::Medium => 10000,
        ThinkingEffort::High => 16000,
        ThinkingEffort::Max => 32000,
    }
}

// Anthropic counts thinking tokens against max_tokens, so the budget must leave
// room for a response. Clamp it to preserve at least this many answer tokens, and
// drop thinking only when even a minimal budget wouldn't fit under the cap.
// Shared with the Bedrock formatter, which applies the same clamp.
pub const MIN_ANSWER_TOKENS: i32 = 1024;

fn apply_thinking_config(
    payload: &mut Value,
    provider_name: &str,
    model_config: &ModelConfig,
    max_tokens: i32,
    options: AnthropicFormatOptions,
) {
    let obj = payload.as_object_mut().unwrap();
    match thinking_type_for_provider(provider_name, model_config) {
        ThinkingType::Adaptive => {
            obj.insert("thinking".to_string(), json!({"type": "adaptive"}));
            let effort = adaptive_effort_wire(provider_name, model_config);
            obj.insert("output_config".to_string(), json!({"effort": effort}));
        }
        ThinkingType::Enabled => {
            let budget_tokens = thinking_budget_tokens(model_config)
                .min(max_tokens.saturating_sub(MIN_ANSWER_TOKENS));
            if budget_tokens >= MIN_ANSWER_TOKENS {
                obj.insert(
                    "thinking".to_string(),
                    json!({
                        "type": "enabled",
                        "budget_tokens": budget_tokens
                    }),
                );
            }
        }
        ThinkingType::Disabled => {}
    }

    if options.preserve_thinking_context && !options.thinking_disabled {
        if !obj.contains_key("thinking") {
            let budget_tokens = thinking_budget_tokens(model_config)
                .min(max_tokens.saturating_sub(MIN_ANSWER_TOKENS));
            if budget_tokens >= MIN_ANSWER_TOKENS {
                obj.insert(
                    "thinking".to_string(),
                    json!({
                        "type": "enabled",
                        "budget_tokens": budget_tokens
                    }),
                );
            }
        }

        // Z.AI requires this to preserve reasoning; Anthropic rejects it.
        if options.emit_clear_thinking
            && let Some(thinking) = obj.get_mut("thinking").and_then(|t| t.as_object_mut())
        {
            thinking.insert("clear_thinking".to_string(), json!(false));
        }
    }

    if !obj.contains_key("thinking")
        && requires_explicit_thinking_disable(provider_name, &model_config.model_name)
    {
        obj.insert("thinking".to_string(), json!({"type": "disabled"}));
    }

    // `block_binding` is only accepted alongside adaptive or enabled thinking.
    if let Some(behavior) = options.prefix_mismatch_behavior
        && let Some(thinking) = obj.get_mut("thinking").and_then(|t| t.as_object_mut())
        && thinking.get("type").and_then(|t| t.as_str()) != Some("disabled")
    {
        thinking.insert(
            "block_binding".to_string(),
            json!({"prefix_mismatch_behavior": behavior.to_string()}),
        );
    }
}

pub fn block_binding_behavior(payload: &Value) -> Option<PrefixMismatchBehavior> {
    payload
        .pointer("/thinking/block_binding/prefix_mismatch_behavior")
        .and_then(Value::as_str)
        .and_then(|behavior| behavior.parse().ok())
}

pub fn is_thinking_signature_error(message: &str) -> bool {
    let lower = message.to_lowercase();
    lower.contains("thinking")
        && (lower.contains("signature")
            || lower.contains("cannot be modified")
            || lower.contains("block_binding"))
}

pub fn create_request_anthropic(
    provider_name: &str,
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    options: AnthropicFormatOptions,
) -> Result<Value> {
    create_request_for_model_anthropic(
        provider_name,
        model_config,
        &model_config.model_name,
        system,
        messages,
        tools,
        options,
    )
}

pub fn create_request_for_model_anthropic(
    provider_name: &str,
    model_config: &ModelConfig,
    wire_model_name: &str,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    options: AnthropicFormatOptions,
) -> Result<Value> {
    let options = options.for_model(provider_name, model_config);
    let anthropic_messages = format_messages_with_options(messages, &options);
    let tool_specs = format_tools(tools, &options);
    let system_spec = format_system(system, &options);

    if anthropic_messages.is_empty() {
        return Err(anyhow!("No valid messages to send to Anthropic API"));
    }

    let max_tokens = model_config.max_output_tokens();
    let mut payload = json!({
        "model": wire_model_name,
        "messages": anthropic_messages,
        "max_tokens": max_tokens,
    });

    if !system.is_empty() {
        payload
            .as_object_mut()
            .unwrap()
            .insert("system".to_string(), json!(system_spec));
    }

    if !tool_specs.is_empty() {
        payload
            .as_object_mut()
            .unwrap()
            .insert("tools".to_string(), json!(tool_specs));
    }

    if model_supports_temperature(provider_name, model_config)
        && let Some(temp) = model_config.temperature
    {
        payload
            .as_object_mut()
            .unwrap()
            .insert("temperature".to_string(), json!(temp));
    }

    apply_thinking_config(
        &mut payload,
        provider_name,
        model_config,
        max_tokens,
        options,
    );

    Ok(payload)
}

/// Process streaming response from Anthropic's API
pub fn response_to_streaming_message_anthropic<S>(
    mut stream: S,
) -> impl futures::Stream<Item = anyhow::Result<(Option<Message>, Option<ProviderUsage>)>> + 'static
where
    S: futures::Stream<Item = anyhow::Result<String>> + Unpin + MaybeSend + 'static,
{
    use async_stream::try_stream;
    use futures::StreamExt;
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize, Debug)]
    struct StreamingEvent {
        #[serde(rename = "type")]
        event_type: String,
        #[serde(flatten)]
        data: Value,
    }

    #[derive(Deserialize, Debug)]
    #[serde(tag = "type", rename_all = "snake_case")]
    #[allow(clippy::enum_variant_names)]
    enum ContentBlockDelta {
        TextDelta { text: String },
        InputJsonDelta { partial_json: String },
        ThinkingDelta { thinking: String },
        SignatureDelta { signature: String },
    }

    struct ThinkingState {
        text: String,
        signature: String,
    }

    fn block_index(event_data: &Value) -> Option<i32> {
        event_data
            .get("index")
            .and_then(|v| v.as_i64())
            .map(|index| index as i32)
    }

    try_stream! {
        struct StreamingToolCall {
            id: String,
            name: String,
            arguments: String,
        }

        let mut accumulated_tool_calls: std::collections::HashMap<i32, StreamingToolCall> = std::collections::HashMap::new();
        let mut final_usage: Option<ProviderUsage> = None;
        let mut message_id: Option<String> = None;
        let mut thinking: Option<ThinkingState> = None;
        let mut stop_reason: Option<String> = None;
        let mut additional_data: Option<Map<String, Value>> = None;

        while let Some(line_result) = stream.next().await {
            let line = line_result?;

            // Skip empty lines and non-data lines
            // Note: SSE spec allows both "data: value" and "data:value" (space is optional)
            if line.trim().is_empty() || !line.starts_with("data:") {
                continue;
            }

            let data_part = line.strip_prefix("data: ").or_else(|| line.strip_prefix("data:")).unwrap_or(&line);

            // Handle end of stream
            if data_part.trim() == "[DONE]" {
                break;
            }

            // Parse the JSON event
            let event: StreamingEvent = match serde_json::from_str(data_part) {
                Ok(event) => event,
                Err(e) => {
                    tracing::debug!("Failed to parse streaming event: {} - Line: {}", e, data_part);
                    continue;
                }
            };

            match event.event_type.as_str() {
                EVENT_MESSAGE_START => {
                    if let Some(message_data) = event.data.get("message") {
                        additional_data = get_additional_data(message_data);
                        if let Some(transformations) = input_transformations(message_data) {
                            additional_data
                                .get_or_insert_with(Map::new)
                                .insert(INPUT_TRANSFORMATIONS_FIELD.to_string(), transformations);
                        }
                        if let Some(id) = message_data.get("id").and_then(|v| v.as_str()) {
                            message_id = Some(id.to_string());
                        }

                        if let Some(usage_data) = message_data.get("usage") {
                            let usage = get_usage_anthropic(usage_data).unwrap_or_default();
                            let model = message_data.get("model")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            final_usage = Some(provider_usage_with_cost(model, usage, usage_data, None));
                        }
                    }
                    continue;
                }
                EVENT_CONTENT_BLOCK_START => {
                    if let Some(content_block) = event.data.get("content_block") {
                        match content_block.get(TYPE_FIELD).and_then(|v| v.as_str()) {
                            Some(TOOL_USE_TYPE) => {
                                if let (Some(index), Some(id), Some(name)) = (
                                    block_index(&event.data),
                                    content_block.get("id").and_then(|v| v.as_str()),
                                    content_block.get("name").and_then(|v| v.as_str()),
                                ) {
                                    accumulated_tool_calls.insert(index, StreamingToolCall {
                                        id: id.to_string(),
                                        name: name.to_string(),
                                        arguments: String::new(),
                                    });
                                }
                            }
                            Some(THINKING_TYPE) => {
                                thinking = Some(ThinkingState {
                                    text: content_block
                                        .get(THINKING_TYPE)
                                        .and_then(|t| t.as_str())
                                        .unwrap_or_default()
                                        .to_string(),
                                    signature: content_block
                                        .get(SIGNATURE_FIELD)
                                        .and_then(|s| s.as_str())
                                        .unwrap_or_default()
                                        .to_string(),
                                });
                            }
                            Some(REDACTED_THINKING_TYPE) => {
                                if let Some(data) = content_block.get(DATA_FIELD).and_then(|d| d.as_str()) {
                                    let mut message = Message::assistant()
                                        .with_redacted_thinking(data);
                                    message.id = message_id.clone();
                                    yield (Some(message), None);
                                } else {
                                    tracing::warn!("redacted_thinking block missing '{}' field", DATA_FIELD);
                                }
                            }
                            _ => {}
                        }
                    }
                    continue;
                }
                EVENT_CONTENT_BLOCK_DELTA => {
                    if let Some(delta) = event.data.get("delta") {
                        match serde_json::from_value::<ContentBlockDelta>(delta.clone()) {
                            Ok(ContentBlockDelta::TextDelta { text }) => {
                                let mut message = Message::assistant().with_text(&text);
                                message.id = message_id.clone();
                                yield (Some(message), None);
                            }
                            Ok(ContentBlockDelta::InputJsonDelta { partial_json }) => {
                                if let Some(call) = block_index(&event.data)
                                    .and_then(|index| accumulated_tool_calls.get_mut(&index))
                                {
                                    call.arguments.push_str(&partial_json);
                                }
                            }
                            Ok(ContentBlockDelta::ThinkingDelta { thinking: t }) => {
                                if let Some(ref mut state) = thinking {
                                    state.text.push_str(&t);
                                }
                            }
                            Ok(ContentBlockDelta::SignatureDelta { signature: s }) => {
                                if let Some(ref mut state) = thinking {
                                    state.signature.push_str(&s);
                                }
                            }
                            Err(e) => {
                                tracing::debug!("Unknown content_block_delta type: {}", e);
                            }
                        }
                    }
                    continue;
                }
                EVENT_CONTENT_BLOCK_STOP => {
                    if let Some(state) = thinking.take() {
                        // Omitted thinking arrives as an empty string with a signature and must still be replayed.
                        if !state.text.is_empty() || !state.signature.is_empty() {
                            let mut message = Message::assistant()
                                .with_thinking(state.text, state.signature);
                            message.id = message_id.clone();
                            yield (Some(message), None);
                        }
                    }
                    if let Some(index) = block_index(&event.data)
                        && let Some(call) = accumulated_tool_calls.remove(&index) {
                            let StreamingToolCall { id, name, arguments } = call;
                            let parsed_args = if arguments.is_empty() {
                                json!({})
                            } else {
                                match json::parse_tool_arguments(&arguments) {
                                    Some(parsed) => parsed,
                                    None => {
                                        let message_text = json::truncation_error_message(&arguments)
                                            .unwrap_or_else(|| {
                                                format!("Could not parse tool arguments: {arguments}")
                                            });
                                        let error = ErrorData::new(
                                            ErrorCode::INVALID_PARAMS,
                                            message_text,
                                            None,
                                        );
                                        let mut message = Message::new(
                                            Role::Assistant,
                                            chrono::Utc::now().timestamp(),
                                            vec![MessageContentBlock::tool_request_with_provider_index(id, Err(error), None, index)],
                                        );
                                        message.id = message_id.clone();
                                        yield (Some(message), None);
                                        continue;
                                    }
                                }
                            };

                            let tool_call = CallToolRequestParams::new(name).with_arguments(object(parsed_args));

                            let mut message = Message::new(
                                rmcp::model::Role::Assistant,
                                chrono::Utc::now().timestamp(),
                                vec![MessageContentBlock::tool_request_with_provider_index(id, Ok(tool_call), None, index)],
                            );
                            message.id = message_id.clone();
                            yield (Some(message), None);
                        }
                    continue;
                }
                EVENT_MESSAGE_DELTA => {
                    if let Some(usage_data) = event.data.get("usage") {
                        let delta_usage = get_usage_anthropic(usage_data).unwrap_or_default();

                        if let Some(existing_usage) = &final_usage {
                            let merged_usage = merge_delta_usage(&existing_usage.usage, &delta_usage, usage_data);
                            final_usage = Some(provider_usage_with_cost(
                                existing_usage.model.clone(),
                                merged_usage,
                                usage_data,
                                existing_usage.cost,
                            ));
                        } else {
                            let model = event.data.get("model")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            final_usage = Some(provider_usage_with_cost(model, delta_usage, usage_data, None));
                        }
                    }
                    if let Some(delta) = event.data.get("delta") {
                        let stop_details = delta.get("stop_details").filter(|d| !d.is_null());
                        if stop_reason.is_none()
                            && let Some(sr) = delta.get("stop_reason").and_then(|v| v.as_str()) {
                                stop_reason = Some(sr.to_string());
                            }
                        if delta.get("stop_reason").and_then(|v| v.as_str()) == Some(STOP_REASON_REFUSAL) {
                            let str_field = |key: &str| stop_details
                                .and_then(|d| d.get(key))
                                .and_then(|v| v.as_str())
                                .map(str::to_string);
                            let details = str_field("explanation")
                                .or_else(|| stop_details.map(|d| d.to_string()))
                                .unwrap_or_else(|| REFUSAL_FALLBACK_DETAILS.to_string());
                            let category = str_field("category");
                            // The refusal delta carries the request's usage;
                            // flush it so refused turns are still accounted.
                            if let Some(mut usage) = final_usage.take() {
                                usage.finish_reasons = Some(vec![STOP_REASON_REFUSAL.to_string()]);
                                usage.response_id = message_id.clone();
                                usage.additional_data = additional_data.clone();
                                yield (None, Some(usage));
                            }
                            Err(ProviderError::Refusal { details, category })?;
                        } else if let Some(details) = stop_details {
                            // No specific handling for these stop details yet —
                            // forward them rather than silently dropping the turn.
                            let mut message = Message::assistant().with_text(format!(
                                "The provider ended the response with: {details}"
                            ));
                            message.id = message_id.clone();
                            yield (Some(message), None);
                        }
                    }
                    continue;
                }
                EVENT_MESSAGE_STOP => {
                    if let Some(usage_data) = event.data.get("usage") {
                        let usage = get_usage_anthropic(usage_data).unwrap_or_default();
                        let model = event.data.get("model")
                            .and_then(|v| v.as_str())
                            .unwrap_or("unknown")
                            .to_string();
                        let fallback_cost = final_usage.as_ref().and_then(|u| u.cost);
                        final_usage = Some(provider_usage_with_cost(model, usage, usage_data, fallback_cost));
                    }
                    break;
                }
                _ => {
                    // Unknown event type, log and continue
                    tracing::debug!("Unknown streaming event type: {}", event.event_type);
                    continue;
                }
            }
        }

        // A tool_use block left open at stream end never received its
        // content_block_stop, so its args are truncated rather than complete.
        if !accumulated_tool_calls.is_empty() {
            let truncated_by_limit = stop_reason.as_deref() == Some("max_tokens");
            let mut indices: Vec<i32> = accumulated_tool_calls.keys().copied().collect();
            indices.sort();
            for index in indices {
                if let Some(StreamingToolCall { id, arguments: args, .. }) = accumulated_tool_calls.remove(&index) {
                    let guidance = if truncated_by_limit {
                        "The model's response was truncated — it hit the output token limit while generating this tool call. \
                         Try increasing max_tokens for this provider or breaking the task into smaller steps."
                    } else {
                        "A tool call was not completed before the stream ended. \
                         Try resending your message or breaking the task into smaller steps."
                    };
                    let snippet_len = args.chars().count();
                    let tail: String = args.chars().rev().take(80).collect::<Vec<_>>().into_iter().rev().collect();
                    let message_text = format!(
                        "{guidance}\nReceived {snippet_len} characters of arguments; cut off at: …{tail}"
                    );
                    let error = ErrorData::new(ErrorCode::INVALID_PARAMS, message_text, None);
                    let mut message = Message::new(
                        Role::Assistant,
                        chrono::Utc::now().timestamp(),
                        vec![MessageContentBlock::tool_request_with_provider_index(id, Err(error), None, index)],
                    );
                    message.id = message_id.clone();
                    yield (Some(message), None);
                }
            }
        }

        if stop_reason.as_deref() == Some("max_tokens") {
            let mut message = Message::assistant();
            message.id = message_id.clone();
            message.metadata.output_token_limit_reached = true;
            yield (Some(message), None);
        }

        if let Some(mut usage) = final_usage {
            if let Some(reason) = stop_reason {
                usage.finish_reasons = Some(vec![reason]);
            }
            if let Some(id) = message_id {
                usage.response_id = Some(id);
            }
            usage.additional_data = additional_data;
            yield (None, Some(usage));
        }
    }
}

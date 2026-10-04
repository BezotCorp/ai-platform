use crate::base::ThinkingPreservationFormat;
use crate::conversation::message::{Message, MessageContentBlock, ProviderMetadata};
use crate::conversation::token_usage::{CostSource, ProviderUsage, Usage};
use crate::documents::{
    ASSISTANT_ROLE_REASON, DocumentFormat, UNSUPPORTED_MEDIA_TYPE_REASON, convert_document,
    document_media_type_is_supported, unsupported_document_text,
};
use crate::errors::ProviderError;
use crate::images::{ImageFormat, convert_image, detect_image_path, load_image_file};
use crate::json::{parse_tool_arguments, truncation_error_message};
use crate::maybe_send::MaybeSend;
use crate::mcp_utils::extract_text_from_resource;
use crate::model::{ModelConfig, is_goose_internal_request_param};
use crate::thinking::{
    GEMINI_THOUGHT_SIGNATURE_KEY, ThinkFilter, ThinkingEffort, split_think_blocks,
};
use anyhow::{Error, anyhow};
use async_stream::try_stream;
use chrono;
use futures::Stream;
use regex::Regex;
use rmcp::model::{CallToolRequestParams, ContentBlock, ErrorCode, ErrorData, Role, Tool, object};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::OnceLock;

type ToolCallData = HashMap<
    i32,
    (
        String,
        String,
        String,
        Option<serde_json::Map<String, Value>>,
    ),
>;

fn deserialize_null_default_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    Ok(Option::<String>::deserialize(deserializer)?.unwrap_or_default())
}

fn describe_json_value(value: &Value) -> &'static str {
    match value {
        Value::Array(_) => "an array",
        Value::String(_) => "a string",
        Value::Number(_) => "a number",
        Value::Bool(_) => "a boolean",
        Value::Null => "null",
        Value::Object(_) => "an object",
    }
}

fn output_token_limit_tool_error(function_name: &str, id: &str) -> ErrorData {
    ErrorData {
        code: ErrorCode::INVALID_PARAMS,
        message: Cow::from(format!(
            "Tool arguments for {function_name} (id {id}) were truncated because the model reached its output token limit"
        )),
        data: None,
    }
}

pub fn is_reserved_request_param_key(key: &str) -> bool {
    matches!(key, "messages" | "model" | "stream" | "stream_options")
}

#[derive(Debug, Clone, Copy, Default)]
pub struct OpenAiFormatOptions {
    pub preserve_thinking_context: bool,
    pub supports_vision: bool,
    pub thinking_preservation_format: Option<ThinkingPreservationFormat>,
}

fn merge_reasoning_text(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() {
        return suffix.to_string();
    }
    if suffix.is_empty() {
        return prefix.to_string();
    }
    if suffix.starts_with(prefix) {
        return suffix.to_string();
    }
    if prefix.ends_with(suffix) {
        return prefix.to_string();
    }

    format!("{prefix}{suffix}")
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct DeltaToolCallFunction {
    name: Option<String>,
    #[serde(default, deserialize_with = "deserialize_null_default_string")]
    arguments: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct DeltaToolCall {
    id: Option<String>,
    function: DeltaToolCallFunction,
    index: Option<i32>,
    r#type: Option<String>,
    #[serde(flatten)]
    extra: Option<serde_json::Map<String, Value>>,
}

#[derive(Serialize, Deserialize, Debug)]
#[serde(untagged)]
enum DeltaContentBlock {
    String(String),
    Array(Vec<ContentBlockPart>),
}

#[derive(Serialize, Deserialize, Debug)]
struct ContentBlockPart {
    r#type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(rename = "thoughtSignature")]
    thought_signature: Option<String>,
}

#[derive(Serialize, Deserialize, Debug, Default)]
struct Delta {
    #[serde(default)]
    content: Option<DeltaContentBlock>,
    role: Option<String>,
    tool_calls: Option<Vec<DeltaToolCall>>,
    reasoning_details: Option<Vec<Value>>,
    reasoning: Option<String>,
    reasoning_content: Option<String>,
}

impl Delta {
    /// Prefer `reasoning_content` (DeepSeek/OpenRouter) over `reasoning`
    /// (vLLM); some servers (gpt-oss via vLLM) emit both. Skip empty values.
    fn reasoning_text(&self) -> Option<&str> {
        self.reasoning_content
            .as_deref()
            .filter(|s| !s.is_empty())
            .or_else(|| self.reasoning.as_deref().filter(|s| !s.is_empty()))
    }
}

#[derive(Serialize, Deserialize, Debug)]
struct StreamingChoice {
    #[serde(default)]
    delta: Delta,
    index: Option<i32>,
    #[serde(default, deserialize_with = "empty_finish_reason_as_none")]
    finish_reason: Option<String>,
}

fn empty_finish_reason_as_none<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(value.filter(|reason| !reason.is_empty()))
}

#[derive(Serialize, Deserialize, Debug)]
struct StreamingChunk {
    choices: Vec<StreamingChoice>,
    created: Option<i64>,
    id: Option<String>,
    usage: Option<Value>,
    model: Option<String>,
}

fn extract_content_and_signature(
    delta_content: Option<&DeltaContentBlock>,
) -> (Option<String>, Option<String>) {
    match delta_content {
        Some(DeltaContentBlock::String(s)) => (Some(s.clone()), None),
        Some(DeltaContentBlock::Array(parts)) => {
            let text_parts: Vec<_> = parts.iter().filter(|p| p.r#type == "text").collect();

            let text = text_parts
                .iter()
                .filter_map(|p| p.text.as_deref())
                .collect::<String>();

            let signature = text_parts
                .iter()
                .find_map(|p| p.thought_signature.as_ref())
                .cloned();

            let text = if text.is_empty() { None } else { Some(text) };

            (text, signature)
        }
        None => (None, None),
    }
}

pub fn format_messages(messages: &[Message], image_format: &ImageFormat) -> Vec<Value> {
    format_messages_with_options(
        messages,
        image_format,
        OpenAiFormatOptions {
            preserve_thinking_context: true,
            ..Default::default()
        },
    )
}

pub fn format_messages_with_options(
    messages: &[Message],
    image_format: &ImageFormat,
    options: OpenAiFormatOptions,
) -> Vec<Value> {
    let mut messages_spec = Vec::new();
    let mut pending_assistant_reasoning = String::new();
    // Reasoning to propagate across consecutive tool-call messages in the same turn.
    // DeepSeek/Kimi require reasoning_content on every assistant tool-call message.
    let mut tool_call_turn_reasoning = String::new();
    let mut saw_tool_response = false;

    for message in messages {
        if options.preserve_thinking_context && message.role != Role::Assistant {
            pending_assistant_reasoning.clear();
        }

        if options.preserve_thinking_context && message.role == Role::User {
            if message
                .content
                .iter()
                .any(|c| matches!(c, MessageContentBlock::ToolResponse(_)))
            {
                saw_tool_response = true;
            } else {
                tool_call_turn_reasoning.clear();
                saw_tool_response = false;
            }
        }

        // A new assistant message after tool results creates a new turn.
        // Prevents reasoning from the previous turn leaking into the new one.
        if options.preserve_thinking_context && message.role == Role::Assistant && saw_tool_response
        {
            tool_call_turn_reasoning.clear();
            saw_tool_response = false;
        }

        let mut converted = json!({
            "role": message.role
        });

        let mut output = Vec::new();
        // Deferred to the end of the message so every tool result in a batch stays
        // consecutive; a strict OpenAI-compatible API rejects a request where a
        // synthetic user image message splits one assistant tool_calls batch.
        let mut pending_image_messages = Vec::new();
        let mut content_array = Vec::new();
        let mut has_non_text_content = false;
        let mut reasoning_text = String::new();

        for content in &message.content {
            match content {
                MessageContentBlock::Text(text) => {
                    if !text.text.is_empty() {
                        if message.role == Role::User {
                            if options.supports_vision {
                                if let Some(image_path) = detect_image_path(&text.text) {
                                    if let Ok(image) = load_image_file(image_path.as_ref()) {
                                        has_non_text_content = true;
                                        content_array
                                            .push(json!({"type": "text", "text": text.text}));
                                        content_array.push(convert_image(&image, image_format));
                                    } else {
                                        content_array
                                            .push(json!({"type": "text", "text": text.text}));
                                    }
                                } else {
                                    content_array.push(json!({"type": "text", "text": text.text}));
                                }
                            } else {
                                content_array.push(json!({"type": "text", "text": text.text}));
                            }
                        } else {
                            content_array.push(json!({"type": "text", "text": text.text}));
                        }
                    }
                }
                MessageContentBlock::Thinking(t) => {
                    reasoning_text.push_str(&t.thinking);
                }
                MessageContentBlock::RedactedThinking(_) => {
                    continue;
                }
                MessageContentBlock::SystemNotification(_) | MessageContentBlock::Error(_) => {
                    continue;
                }
                MessageContentBlock::ToolRequest(request) => match &request.tool_call {
                    Ok(tool_call) => {
                        let sanitized_name = sanitize_function_name(&tool_call.name);
                        let arguments_str = match &tool_call.arguments {
                            Some(args) => {
                                serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string())
                            }
                            None => "{}".to_string(),
                        };

                        let tool_calls = converted
                            .as_object_mut()
                            .unwrap()
                            .entry("tool_calls")
                            .or_insert(json!([]));

                        let mut tool_call_json = json!({
                            "id": request.id,
                            "type": "function",
                            "function": {
                                "name": sanitized_name,
                                "arguments": arguments_str,
                            }
                        });

                        if let Some(metadata) = &request.metadata {
                            for (key, value) in metadata {
                                tool_call_json[key] = value.clone();
                            }
                        }

                        tool_calls.as_array_mut().unwrap().push(tool_call_json);
                    }
                    Err(_e) => {
                        // An unparseable tool call still needs a valid assistant
                        // `tool_calls` entry. Emitting the error as a bare `role:"tool"`
                        // message (the old behavior) leaves the paired tool response —
                        // which carries the parse error — as an orphan `role:"tool"` with
                        // no preceding assistant `tool_calls`, which strict
                        // OpenAI-compatible APIs reject. Emit a placeholder call with the
                        // same id so the history stays well-formed; the error rides on the
                        // following tool response.
                        let tool_calls = converted
                            .as_object_mut()
                            .unwrap()
                            .entry("tool_calls")
                            .or_insert(json!([]));
                        tool_calls.as_array_mut().unwrap().push(json!({
                            "id": request.id,
                            "type": "function",
                            "function": {
                                "name": "unparseable_tool_call",
                                "arguments": "{}",
                            }
                        }));
                    }
                },
                MessageContentBlock::ToolResponse(response) => {
                    match &response.tool_result {
                        Ok(result) => {
                            // Process all content, replacing images with placeholder text
                            let mut tool_content = Vec::new();

                            for content in result.content.iter() {
                                match content {
                                    ContentBlock::Image(image) => {
                                        if options.supports_vision {
                                            // Add placeholder text in the tool response
                                            tool_content.push(ContentBlock::text("This tool result included an image that is uploaded in the next message."));

                                            // Create a separate image message
                                            pending_image_messages.push(json!({
                                                "role": "user",
                                                "content": [convert_image(&image.clone(), image_format)]
                                            }));
                                        } else {
                                            // Add placeholder text in the tool response
                                            tool_content.push(ContentBlock::text("This tool result included an image that was omitted as the model does not support vision."));
                                        }
                                    }
                                    ContentBlock::Resource(resource) => {
                                        let text = extract_text_from_resource(&resource.resource);
                                        tool_content.push(ContentBlock::text(text));
                                    }
                                    _ => {
                                        tool_content.push(content.clone());
                                    }
                                }
                            }
                            let tool_response_content: Value = json!(
                                tool_content
                                    .iter()
                                    .map(|content| match content {
                                        ContentBlock::Text(text) => text.text.clone(),
                                        _ => String::new(),
                                    })
                                    .collect::<Vec<String>>()
                                    .join(" ")
                            );

                            output.push(json!({
                                "role": "tool",
                                "content": tool_response_content,
                                "tool_call_id": response.id
                            }));
                        }
                        Err(e) => {
                            // A tool result error is shown as output so the model can interpret the error message
                            output.push(json!({
                                "role": "tool",
                                "content": format!("The tool call returned the following error:\n{}", e),
                                "tool_call_id": response.id
                            }));
                        }
                    }
                }
                MessageContentBlock::ToolConfirmationRequest(_) => {}
                MessageContentBlock::ActionRequired(_) => {}
                MessageContentBlock::Image(image) => {
                    if message.role == Role::User {
                        if options.supports_vision {
                            has_non_text_content = true;
                            content_array.push(convert_image(image, image_format));
                        } else {
                            content_array.push(json!({
                                "type": "text",
                                "text": "[image omitted: model does not support vision]"
                            }));
                        }
                    } else {
                        content_array.push(json!({
                            "type": "text",
                            "text": "[Image content removed - not supported in assistant messages]"
                        }));
                    }
                }
                MessageContentBlock::Document(document) => {
                    if message.role != Role::User {
                        content_array.push(json!({
                            "type": "text",
                            "text": unsupported_document_text(document, ASSISTANT_ROLE_REASON)
                        }));
                    } else if document_media_type_is_supported(&document.mime_type) {
                        has_non_text_content = true;
                        content_array.push(convert_document(document, &DocumentFormat::OpenAi));
                    } else {
                        content_array.push(json!({
                            "type": "text",
                            "text": unsupported_document_text(document, UNSUPPORTED_MEDIA_TYPE_REASON)
                        }));
                    }
                }
            }
        }

        output.append(&mut pending_image_messages);

        if !content_array.is_empty() {
            if has_non_text_content {
                converted["content"] = json!(content_array);
            } else {
                let texts: Vec<String> = content_array
                    .iter()
                    .filter_map(|v| v["text"].as_str().map(|s| s.to_string()))
                    .collect();
                converted["content"] = json!(texts.join("\n"));
            }
        }

        // Some strict OpenAI-compatible providers require "content" to be present
        // (even as null) when tool_calls are provided. See #6717.
        if message.role == Role::Assistant
            && converted.get("tool_calls").is_some()
            && converted.get("content").is_none()
        {
            converted["content"] = json!(null);
        }

        let has_message_payload =
            converted.get("content").is_some() || converted.get("tool_calls").is_some();

        if options.preserve_thinking_context && message.role == Role::Assistant {
            if !has_message_payload && output.is_empty() && !reasoning_text.is_empty() {
                pending_assistant_reasoning.push_str(&reasoning_text);
                continue;
            }

            if !pending_assistant_reasoning.is_empty() {
                reasoning_text =
                    merge_reasoning_text(&pending_assistant_reasoning, &reasoning_text);
                pending_assistant_reasoning.clear();
            }

            let has_tool_calls = converted
                .get("tool_calls")
                .and_then(|tc| tc.as_array())
                .is_some_and(|a| !a.is_empty());

            if has_tool_calls {
                if reasoning_text.is_empty() {
                    reasoning_text = tool_call_turn_reasoning.clone();
                } else {
                    tool_call_turn_reasoning = reasoning_text.clone();
                }
            } else {
                // Carry reasoning forward even through non-tool assistant messages
                // (e.g., a visible text chunk that's is sent before a tool-call chunk
                // in the same streaming turn). An empty reasoning_text is equivalent
                // to clear.
                tool_call_turn_reasoning = reasoning_text.clone();
            }
        }

        // Include reasoning_content only when non-empty. Kimi rejects empty
        // reasoning_content (""), so we must omit it entirely.
        if options.preserve_thinking_context && !reasoning_text.is_empty() {
            converted["reasoning_content"] = json!(reasoning_text);
        }

        if has_message_payload {
            output.insert(0, converted);
        }

        messages_spec.extend(output);
    }

    merge_split_tool_call_messages(&mut messages_spec);

    if let Some(format) = options.thinking_preservation_format {
        inline_reasoning_content(&mut messages_spec, format);
    }

    messages_spec
}

/// Rewrites `reasoning_content` into the message `content` for models that reject a
/// separate reasoning field on replay.
///
/// Must run after `merge_split_tool_call_messages`, which relies on `reasoning_content`
/// to identify messages split from the same assistant turn.
fn inline_reasoning_content(messages: &mut [Value], format: ThinkingPreservationFormat) {
    let wrap: fn(&str) -> String = match format {
        ThinkingPreservationFormat::ReasoningContent => return,
        ThinkingPreservationFormat::ContentPrepend => |text| format!("{text}\n\n"),
        ThinkingPreservationFormat::ContentXml => |text| format!("<think>\n{text}\n</think>\n\n"),
    };

    for message in messages {
        let Some(object) = message.as_object_mut() else {
            continue;
        };
        let Some(Value::String(reasoning)) = object.remove("reasoning_content") else {
            continue;
        };
        let prefix = wrap(&reasoning);

        match object.entry("content").or_insert(Value::Null) {
            Value::String(content) => content.insert_str(0, &prefix),
            Value::Array(blocks) => blocks.insert(0, json!({"type": "text", "text": prefix})),
            content => *content = json!(prefix.trim_end()),
        }
    }
}

/// The agent splits a single assistant response with N tool_calls into N
/// interleaved `asst(TC)/tool` pairs, cloning `reasoning_content` onto each.
/// This function merges them back into one assistant message with all tool_calls,
/// followed by the tool results — the standard OpenAI format.
///
/// Only merges when `reasoning_content` is present and matches, since that is
/// the only signal that messages were split from the same turn.
fn merge_split_tool_call_messages(messages: &mut Vec<Value>) {
    let mut i = 0;
    while i < messages.len() {
        let is_assistant_tool_call = messages[i].get("role") == Some(&json!("assistant"))
            && messages[i]
                .get("tool_calls")
                .and_then(|tc| tc.as_array())
                .is_some_and(|a| !a.is_empty());
        let base_reasoning = messages[i].get("reasoning_content");

        if !is_assistant_tool_call || base_reasoning.is_none() {
            i += 1;
            continue;
        }
        let base_reasoning = base_reasoning.unwrap().clone();

        let mut extra_tool_calls: Vec<Value> = Vec::new();
        let mut collected: Vec<Value> = Vec::new();
        let mut scan = i + 1;

        loop {
            if scan >= messages.len() || messages[scan].get("role") != Some(&json!("tool")) {
                break;
            }

            // Skip past tool result and any image-only user messages that
            // format_messages inserts after tool results containing images.
            let mut peek = scan + 1;
            while peek < messages.len() && is_image_only_user_message(&messages[peek]) {
                peek += 1;
            }

            if peek >= messages.len() {
                break;
            }
            let next = &messages[peek];
            let has_no_content = next.get("content").is_none_or(|c| {
                c.is_null()
                    || c.as_str().is_some_and(|s| s.is_empty())
                    || c.as_array().is_some_and(|a| a.is_empty())
            });
            let is_split = next.get("role") == Some(&json!("assistant"))
                && next
                    .get("tool_calls")
                    .and_then(|tc| tc.as_array())
                    .is_some_and(|a| !a.is_empty())
                && has_no_content
                && next.get("reasoning_content") == Some(&base_reasoning);

            if !is_split {
                break;
            }

            collected.extend(messages[scan..peek].iter().cloned());
            if let Some(tc) = messages[peek]
                .get("tool_calls")
                .and_then(|tc| tc.as_array())
            {
                extra_tool_calls.extend(tc.iter().cloned());
            }
            scan = peek + 1;
        }

        if extra_tool_calls.is_empty() {
            i += 1;
            continue;
        }

        if let Some(base_tc) = messages[i]
            .get_mut("tool_calls")
            .and_then(|tc| tc.as_array_mut())
        {
            base_tc.extend(extra_tool_calls);
        }

        let insert_at = i + 1;
        messages.drain(insert_at..scan);
        let num_collected = collected.len();
        for (j, msg) in collected.into_iter().enumerate() {
            messages.insert(insert_at + j, msg);
        }

        i = insert_at + num_collected;
    }
}

/// True if `msg` is a synthetic image-only user message (content is exclusively image_url items).
fn is_image_only_user_message(msg: &Value) -> bool {
    msg.get("role") == Some(&json!("user"))
        && msg
            .get("content")
            .and_then(|c| c.as_array())
            .is_some_and(|arr| {
                !arr.is_empty()
                    && arr
                        .iter()
                        .all(|item| item.get("type") == Some(&json!("image_url")))
            })
}

pub fn format_tools(tools: &[Tool]) -> anyhow::Result<Vec<Value>> {
    let mut tool_names = std::collections::HashSet::new();
    let mut result = Vec::new();

    for tool in tools {
        if !tool_names.insert(&tool.name) {
            return Err(anyhow!("Duplicate tool name: {}", tool.name));
        }

        result.push(json!({
            "type": "function",
            "function": {
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.input_schema,
                "strict": false,
            }
        }));
    }

    Ok(result)
}

pub fn record_response_metadata(usage: &mut ProviderUsage, response: &Value) {
    usage.response_id = response
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_string);

    let finish_reasons = response
        .get("choices")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|choice| choice.get("finish_reason").and_then(Value::as_str))
        .map(str::to_string)
        .collect::<Vec<_>>();
    if !finish_reasons.is_empty() {
        usage.finish_reasons = Some(finish_reasons);
    }
}

/// Convert OpenAI's API response to internal Message format
pub fn response_to_message(response: &Value) -> anyhow::Result<Message> {
    let output_token_limit_reached = response
        .pointer("/choices/0/finish_reason")
        .and_then(Value::as_str)
        == Some("length");

    let Some(original) = response
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|m| m.get("message"))
    else {
        if let Some(error) = response.get("error") {
            let error_message = error
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("Unknown error");
            return Err(anyhow::anyhow!("API error: {}", error_message));
        }
        return Err(anyhow::anyhow!(
            "No message in API response. This may indicate a quota limit or other restriction."
        ));
    };

    let mut content = Vec::new();

    // Capture reasoning content if present (DeepSeek uses "reasoning_content", vLLM uses "reasoning")
    let reasoning_value = original
        .get("reasoning_content")
        .or_else(|| original.get("reasoning"));
    let mut has_structured_thinking = false;
    if let Some(reasoning_content) = reasoning_value
        && let Some(reasoning_str) = reasoning_content.as_str()
        && !reasoning_str.is_empty()
    {
        has_structured_thinking = true;
        content.push(MessageContentBlock::thinking(reasoning_str, ""));
    }

    if let Some(text) = original.get("content")
        && let Some(text_str) = text.as_str()
    {
        let (cleaned, inline_thinking) = split_think_blocks(text_str);

        if !has_structured_thinking && !inline_thinking.is_empty() {
            content.push(MessageContentBlock::thinking(inline_thinking, ""));
        }

        if !cleaned.is_empty() {
            content.push(MessageContentBlock::text(cleaned));
        }
    }

    if let Some(tool_calls) = original.get("tool_calls")
        && let Some(tool_calls_array) = tool_calls.as_array()
    {
        for tool_call in tool_calls_array {
            let id = tool_call["id"].as_str().unwrap_or_default().to_string();
            let function_name = tool_call["function"]["name"]
                .as_str()
                .unwrap_or_default()
                .to_string();

            // Get the raw arguments string from the LLM.
            let arguments_str = tool_call["function"]["arguments"]
                .as_str()
                .unwrap_or_default()
                .to_string();

            // If arguments_str is empty, default to an empty JSON object string.
            let arguments_str = if arguments_str.is_empty() {
                "{}".to_string()
            } else {
                arguments_str
            };

            let standard_fields = ["id", "function", "type", "index"];
            let metadata: Option<serde_json::Map<String, Value>> = tool_call
                .as_object()
                .map(|obj| {
                    obj.iter()
                        .filter(|(k, _)| !standard_fields.contains(&k.as_str()))
                        .map(|(k, v)| (k.clone(), v.clone()))
                        .collect()
                })
                .filter(|m: &serde_json::Map<String, Value>| !m.is_empty());

            if output_token_limit_reached {
                let error = output_token_limit_tool_error(&function_name, &id);
                content.push(MessageContentBlock::tool_request_with_metadata(
                    id,
                    Err(error),
                    metadata.as_ref(),
                ));
                continue;
            }

            if function_name.is_empty() {
                let error = ErrorData {
                    code: ErrorCode::INVALID_REQUEST,
                    message: Cow::from(
                        "The provided function name was empty; a tool call must name a tool"
                            .to_string(),
                    ),
                    data: None,
                };
                content.push(MessageContentBlock::tool_request_with_metadata(
                    id,
                    Err(error),
                    metadata.as_ref(),
                ));
                continue;
            }
            match parse_tool_arguments(&arguments_str) {
                Some(params) if params.is_object() => {
                    content.push(MessageContentBlock::tool_request_with_metadata(
                        id,
                        Ok(CallToolRequestParams::new(function_name)
                            .with_arguments(object(params))),
                        metadata.as_ref(),
                    ));
                }
                Some(other) => {
                    let error = ErrorData {
                        code: ErrorCode::INVALID_PARAMS,
                        message: Cow::from(format!(
                            "Tool arguments for {} (id {}) must be a JSON object, got {}. Raw arguments: '{}'",
                            function_name,
                            id,
                            describe_json_value(&other),
                            arguments_str
                        )),
                        data: None,
                    };
                    content.push(MessageContentBlock::tool_request_with_metadata(
                        id,
                        Err(error),
                        metadata.as_ref(),
                    ));
                }
                None => {
                    let message_text =
                        truncation_error_message(&arguments_str).unwrap_or_else(|| {
                            format!("Could not interpret tool use parameters for id {id}")
                        });
                    let error = ErrorData {
                        code: ErrorCode::INVALID_PARAMS,
                        message: Cow::from(message_text),
                        data: None,
                    };
                    content.push(MessageContentBlock::tool_request_with_metadata(
                        id,
                        Err(error),
                        metadata.as_ref(),
                    ));
                }
            }
        }
    }

    let mut message = Message::new(Role::Assistant, chrono::Utc::now().timestamp(), content);
    message.metadata.output_token_limit_reached = output_token_limit_reached;
    Ok(message)
}

pub fn get_usage(usage: &Value) -> Usage {
    let usage = usage
        .get("usage")
        .filter(|nested| nested.is_object())
        .unwrap_or(usage);

    // Try standard OpenAI fields first, then fall back to Ollama-native fields
    // (prompt_eval_count / eval_count) for compatibility with older Ollama builds
    // that don't translate to OpenAI field names.
    // Parse the value before falling back so that present-but-null keys
    // (e.g. "completion_tokens": null) don't block the fallback.
    let input_tokens = usage
        .get("prompt_tokens")
        .and_then(|v| v.as_i64())
        .or_else(|| usage.get("prompt_eval_count").and_then(|v| v.as_i64()))
        .map(|v| v as i32);

    let output_tokens = usage
        .get("completion_tokens")
        .and_then(|v| v.as_i64())
        .or_else(|| usage.get("eval_count").and_then(|v| v.as_i64()))
        .map(|v| v as i32);

    let cache_read_input_tokens = usage
        .get("cache_read_input_tokens")
        .and_then(|v| v.as_i64())
        .or_else(|| {
            usage
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cached_tokens"))
                .and_then(|v| v.as_i64())
        })
        .map(|v| v as i32);

    let cache_write_input_tokens = usage
        .get("cache_creation_input_tokens")
        .and_then(|v| v.as_i64())
        .or_else(|| {
            usage
                .get("prompt_tokens_details")
                .and_then(|d| d.get("cache_write_tokens"))
                .and_then(|v| v.as_i64())
        })
        .map(|v| v as i32);

    let total_tokens = usage
        .get("total_tokens")
        .and_then(|v| v.as_i64())
        .map(|v| v as i32)
        .or_else(|| match (input_tokens, output_tokens) {
            (Some(input), Some(output)) => Some(input.saturating_add(output)),
            _ => None,
        });

    Usage::new(input_tokens, output_tokens, total_tokens)
        .with_cache_tokens(cache_read_input_tokens, cache_write_input_tokens)
}

pub fn get_cost(usage: &Value) -> Option<f64> {
    usage
        .get("cost")
        .and_then(|v| v.as_f64())
        .filter(|c| c.is_finite() && *c >= 0.0)
}

fn extract_usage_with_output_tokens(
    chunk: &StreamingChunk,
    fallback_model: Option<&str>,
) -> Option<ProviderUsage> {
    chunk
        .usage
        .as_ref()
        .and_then(|u| {
            chunk.model.as_deref().or(fallback_model).map(|model| {
                let usage = ProviderUsage::new(model.to_string(), get_usage(u));
                match get_cost(u) {
                    Some(cost) => usage.with_cost(cost, CostSource::ProviderReported),
                    None => usage,
                }
            })
        })
        .filter(|u| u.usage.output_tokens.is_some())
}

/// Validates and fixes tool schemas to ensure they have proper parameter structure.
/// If parameters exist, ensures they have properties and required fields, or removes parameters entirely.
pub fn validate_tool_schemas(tools: &mut [Value]) {
    for tool in tools.iter_mut() {
        if let Some(function) = tool.get_mut("function")
            && let Some(parameters) = function.get_mut("parameters")
            && parameters.is_object()
        {
            ensure_valid_json_schema(parameters);
        }
    }
}

/// Ensures that the given JSON value follows the expected JSON Schema structure.
fn ensure_valid_json_schema(schema: &mut Value) {
    if let Some(params_obj) = schema.as_object_mut()
        && !params_obj.contains_key("type")
    {
        params_obj.insert("type".to_string(), json!("object"));
    }
    sanitize_schema_node(schema);
}

fn sanitize_schema_node(node: &mut Value) {
    if let Some(obj) = node.as_object_mut() {
        // Moonshot's walle validator rejects `oneOf` behind a `$ref` as
        // "infinite recursion" because its termination check only traverses
        // `anyOf`. The two are interchangeable for tool-argument schemas, so
        // emit the more widely supported form.
        if !obj.contains_key("anyOf")
            && let Some(one_of) = obj.remove("oneOf")
        {
            obj.insert("anyOf".to_string(), one_of);
        }
    }

    normalize_nullable(node);

    let Some(obj) = node.as_object_mut() else {
        return;
    };

    if obj.get("type").and_then(|t| t.as_str()) == Some("object") {
        obj.entry("properties").or_insert_with(|| json!({}));
        obj.entry("required").or_insert_with(|| json!([]));
    }

    for key in ["properties", "$defs", "definitions"] {
        if let Some(children) = obj.get_mut(key).and_then(Value::as_object_mut) {
            for child in children.values_mut() {
                sanitize_schema_node(child);
            }
        }
    }
    for key in ["anyOf", "allOf", "prefixItems"] {
        if let Some(children) = obj.get_mut(key).and_then(Value::as_array_mut) {
            for child in children.iter_mut() {
                sanitize_schema_node(child);
            }
        }
    }
    for key in ["items", "additionalProperties"] {
        if let Some(child) = obj.get_mut(key)
            && child.is_object()
        {
            sanitize_schema_node(child);
        }
    }
}

/// Normalizes nullable type representations that some providers (e.g. Vertex Gemini via Bifrost)
/// don't support:
/// - `"type": ["integer", "null"]` → `"type": "integer"` (drops the null variant)
/// - `"anyOf": [T, {"type": "null"}]` → T (unwraps to the non-null schema)
///
/// Optional-ness is already conveyed by the field being absent from `required`.
fn normalize_nullable(schema: &mut Value) {
    let Some(obj) = schema.as_object_mut() else {
        return;
    };

    // Handle type: ["T", "null"] array form (schemars 1.x style for nullable primitives)
    if let Some(type_val) = obj.get("type").cloned()
        && let Some(types) = type_val.as_array()
    {
        let non_null: Vec<&Value> = types
            .iter()
            .filter(|t| t.as_str() != Some("null"))
            .collect();
        if non_null.len() == 1 {
            let scalar = non_null[0].clone();
            obj.insert("type".to_string(), scalar);
            return;
        }
    }

    // Handle anyOf: [T, {type: "null"}] form — merge the non-null variant's fields
    // into the current object (preserving sibling keys like "description" or "default")
    // rather than replacing the whole schema.
    if let Some(any_of) = obj.remove("anyOf") {
        if let Some(variants) = any_of.as_array()
            && variants.len() == 2
        {
            let is_null = |v: &Value| v.get("type").and_then(|t| t.as_str()) == Some("null");
            let non_null = if is_null(&variants[0]) {
                Some(&variants[1])
            } else if is_null(&variants[1]) {
                Some(&variants[0])
            } else {
                None
            };
            if let Some(replacement) = non_null
                && let Some(replacement_obj) = replacement.as_object()
            {
                for (k, v) in replacement_obj {
                    obj.entry(k.clone()).or_insert(v.clone());
                }
                return;
            }
        }
        // Put it back if we couldn't simplify
        obj.insert("anyOf".to_string(), any_of);
    }
}

fn strip_data_prefix(line: &str) -> Option<&str> {
    // SSE spec allows both "data: value" and "data:value" (space after colon is optional)
    line.strip_prefix("data: ")
        .or_else(|| line.strip_prefix("data:"))
        .map(|s| s.trim())
}

/// Longest error text pulled out of a stream frame, so a pathological payload cannot be
/// pasted wholesale into a user-facing message.
const MAX_STREAM_ERROR_LEN: usize = 500;

/// Best-effort human-readable text for an error payload that may not be a plain string.
///
/// FastAPI reports `HTTPException` as `{"detail": "..."}` but `RequestValidationError` as
/// `{"detail": [{"loc": [...], "msg": "field required", ...}]}`, so a string-only read would
/// drop the commoner validation shape entirely.
fn stream_error_text(value: &Value) -> Option<String> {
    fn one(value: &Value) -> Option<String> {
        match value {
            Value::String(s) => Some(s.clone()),
            Value::Object(_) => value
                .get("msg")
                .or_else(|| value.get("message"))
                .and_then(|m| m.as_str().map(String::from))
                .or_else(|| Some(value.to_string())),
            Value::Null => None,
            other => Some(other.to_string()),
        }
    }

    let text = match value {
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(one).collect();
            if parts.is_empty() {
                return None;
            }
            parts.join("; ")
        }
        other => one(other)?,
    };
    if text.is_empty() {
        return None;
    }
    if text.chars().count() > MAX_STREAM_ERROR_LEN {
        let truncated: String = text.chars().take(MAX_STREAM_ERROR_LEN).collect();
        return Some(format!("{truncated}…"));
    }
    Some(text)
}

/// Decide whether a choice-less SSE frame reports an in-stream failure.
///
/// Returns `Some(err)` when it does, `None` when it is gateway metadata that can be skipped.
///
/// Requires an actual error *signal* — a `status`/`statusCode`/`code` of 400 or above, a
/// `type` of `"error"`, or a `detail` field, which has no benign meaning in this position.
/// Mere prose is not enough: gateways also emit informational frames, and treating
/// `{"message": "processing"}` as a failure would kill a healthy stream, which is the very
/// bug this skip exists to avoid. The converse matters just as much — a gateway that
/// rate-limits with a bare `{"statusCode": 429, "message": …}` on an HTTP 200 must not be
/// silently skipped, or a failed turn is reported as an empty successful one.
fn classify_choiceless_frame(value: &Value) -> Option<ProviderError> {
    let status = ["status", "statusCode", "code"].iter().find_map(|key| {
        let raw = value.get(*key)?;
        raw.as_i64()
            .or_else(|| raw.as_str().and_then(|s| s.parse::<i64>().ok()))
    });

    let has_error_signal = status.is_some_and(|s| s >= 400)
        || value.get("type").and_then(|t| t.as_str()) == Some("error")
        || value.get("detail").is_some_and(|d| !d.is_null());
    if !has_error_signal {
        return None;
    }

    let details = value
        .get("message")
        .and_then(stream_error_text)
        .or_else(|| value.get("detail").and_then(stream_error_text))
        .or_else(|| value.get("error").and_then(stream_error_text))
        // A status with no recoverable text must still be loud rather than vanish.
        .unwrap_or_else(|| match status {
            Some(s) => format!("Gateway returned status {s} mid-stream"),
            None => "Unknown server error".to_string(),
        });
    Some(ProviderError::ServerError(details))
}

/// Parse one SSE `data:` payload.
///
/// Returns `Ok(None)` for a metadata-only frame — a JSON object with no `choices` key at
/// all. Gateways interleave these with the real chunks: Portkey/Azure APIM and friends
/// emit trace/guardrail objects such as `{"hook_results": {...}}` before the first token.
/// They carry nothing this parser consumes, so they are skipped rather than failed on;
/// treating them as decode errors kills the whole turn on an otherwise healthy stream.
///
/// A frame with `"choices": []` is NOT metadata — that is the standard usage-only chunk,
/// so it still deserializes and flows through the empty-choices paths below.
///
/// A choice-less frame that reports an in-stream failure is NOT metadata either — see
/// `classify_choiceless_frame`.
fn parse_streaming_chunk(line: &str) -> Result<Option<StreamingChunk>, ProviderError> {
    let value: Value = serde_json::from_str(line).map_err(|e| {
        ProviderError::stream_decode_error(format!(
            "Failed to parse streaming chunk: {e}: {line:?}"
        ))
    })?;

    if let Some(error) = value.get("error") {
        let message = error
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown server error");
        return Err(ProviderError::ServerError(message.to_string()));
    }

    if value.get("object").and_then(|o| o.as_str()) == Some("error") {
        let message = value
            .get("message")
            .and_then(|m| m.as_str())
            .unwrap_or("Unknown server error");
        return Err(ProviderError::ServerError(message.to_string()));
    }

    if value
        .as_object()
        .is_some_and(|o| !o.contains_key("choices"))
    {
        if let Some(err) = classify_choiceless_frame(&value) {
            return Err(err);
        }
        return Ok(None);
    }

    serde_json::from_value(value).map(Some).map_err(|e| {
        ProviderError::stream_decode_error(format!(
            "Failed to parse streaming chunk: {e}: {line:?}"
        ))
    })
}

fn output_token_limit_marker(id: Option<String>) -> Message {
    let mut message = Message::assistant();
    if let Some(id) = id {
        message = message.with_id(id);
    }
    message.metadata.output_token_limit_reached = true;
    message
}

pub fn response_to_streaming_message<S>(
    mut stream: S,
) -> impl Stream<Item = anyhow::Result<(Option<Message>, Option<ProviderUsage>)>> + 'static
where
    S: Stream<Item = anyhow::Result<String>> + Unpin + MaybeSend + 'static,
{
    try_stream! {
        use futures::StreamExt;

        let mut accumulated_reasoning: Vec<Value> = Vec::new();
        let mut accumulated_reasoning_content = String::new();
        let mut think_filter = ThinkFilter::new();
        let mut saw_structured_reasoning = false;
        let mut yielded_reasoning_content_len = 0usize;
        let mut last_signature: Option<String> = None;
        // Buffer inline <think>...</think> content until we know whether structured
        // reasoning will arrive. Emitting it immediately and then receiving
        // reasoning_content in a later chunk would produce duplicated reasoning.
        let mut pending_inline_thinking = String::new();
        let mut last_seen_model: Option<String> = None;
        let mut last_response_id: Option<String> = None;
        let mut last_finish_reason: Option<String> = None;
        let mut output_token_limit_reached = false;
        let mut output_token_limit_metadata_emitted = false;
        let mut usage_emitted = false;

        'outer: while let Some(response) = stream.next().await {
            let response_str = response?;
            let line = strip_data_prefix(&response_str);

            if line.is_some_and(|l| l == "[DONE]") {
                break 'outer;
            }

            if line.is_none() || line.is_some_and(|l| l.is_empty()) {
                continue
            }

            let Some(chunk) = parse_streaming_chunk(
                line.ok_or_else(|| anyhow!("unexpected stream format"))?
            )? else {
                continue  // metadata-only frame
            };
            if let Some(model) = &chunk.model {
                last_seen_model = Some(model.clone());
            }
            if let Some(id) = &chunk.id {
                last_response_id = Some(id.clone());
            }

            if !chunk.choices.is_empty() {
                if let Some(details) = &chunk.choices[0].delta.reasoning_details {
                    accumulated_reasoning.extend(details.iter().cloned());
                }
                if let Some(rc) = chunk.choices[0].delta.reasoning_text() {
                    accumulated_reasoning_content.push_str(rc);
                    if !rc.is_empty() {
                        saw_structured_reasoning = true;
                        pending_inline_thinking.clear();
                    }
                }
            }

            if let Some(reason) = chunk.choices.first().and_then(|c| c.finish_reason.clone()) {
                last_finish_reason = Some(reason);
            }
            let mut usage = extract_usage_with_output_tokens(&chunk, last_seen_model.as_deref());
            if let Some(u) = usage.as_mut() {
                if let Some(reason) = &last_finish_reason {
                    u.finish_reasons = Some(vec![reason.clone()]);
                }
                if let Some(id) = &last_response_id {
                    u.response_id = Some(id.clone());
                }
            }
            output_token_limit_reached |= last_finish_reason.as_deref() == Some("length");

            if chunk.choices.is_empty() {
                usage_emitted |= usage.is_some();
                yield (None, usage)
            } else if chunk.choices[0].delta.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty()) {
                let mut tool_call_data: ToolCallData = HashMap::new();

                if let Some(tool_calls) = &chunk.choices[0].delta.tool_calls {
                    for (position, tool_call) in tool_calls.iter().enumerate() {
                        if let (Some(id), Some(name)) = (&tool_call.id, &tool_call.function.name) {
                            let index = tool_call.index.unwrap_or(position as i32);
                            tool_call_data.insert(index, (id.clone(), name.clone(), tool_call.function.arguments.clone(), tool_call.extra.clone()));
                        }
                    }
                }

                let is_complete = matches!(
                    chunk.choices[0].finish_reason.as_deref(),
                    Some("tool_calls" | "length")
                );

                if !is_complete {
                    let mut done = false;
                    while !done {
                        if let Some(response_chunk) = stream.next().await {
                            let response_str = response_chunk?;
                            if let Some(line) = strip_data_prefix(&response_str) {
                                if line == "[DONE]" {
                                    break 'outer;
                                }

                                // A metadata frame here must NOT fall through to the
                                // empty-choices branch below, which ends accumulation and
                                // would truncate this tool call's arguments.
                                let Some(tool_chunk) = parse_streaming_chunk(line)? else {
                                    continue
                                };
                                if let Some(model) = &tool_chunk.model {
                                    last_seen_model = Some(model.clone());
                                }
                                if let Some(id) = &tool_chunk.id {
                                    last_response_id = Some(id.clone());
                                }
                                if let Some(reason) = tool_chunk.choices.first().and_then(|c| c.finish_reason.clone()) {
                                    last_finish_reason = Some(reason);
                                }

                                if let Some(mut chunk_usage) = extract_usage_with_output_tokens(&tool_chunk, last_seen_model.as_deref()) {
                                    if let Some(reason) = &last_finish_reason {
                                        chunk_usage.finish_reasons = Some(vec![reason.clone()]);
                                    }
                                    if let Some(id) = &last_response_id {
                                        chunk_usage.response_id = Some(id.clone());
                                    }
                                    usage = Some(chunk_usage);
                                }

                                if !tool_chunk.choices.is_empty() {
                                    output_token_limit_reached |=
                                        tool_chunk.choices[0].finish_reason.as_deref()
                                            == Some("length");

                                    if let Some(details) = &tool_chunk.choices[0].delta.reasoning_details {
                                        accumulated_reasoning.extend(details.iter().cloned());
                                    }
                                    if let Some(rc) = tool_chunk.choices[0].delta.reasoning_text() {
                                        accumulated_reasoning_content.push_str(rc);
                                        if !rc.is_empty() {
                                            saw_structured_reasoning = true;
                                            pending_inline_thinking.clear();
                                        }
                                    }
                                    if let Some(delta_tool_calls) = &tool_chunk.choices[0].delta.tool_calls {
                                        for delta_call in delta_tool_calls {
                                            if let Some(index) = delta_call.index {
                                                if let Some((_, stored_name, args, extra)) = tool_call_data.get_mut(&index) {
                                                    if let Some(new_name) = &delta_call.function.name
                                                        && !new_name.is_empty() {
                                                            *stored_name = new_name.clone();
                                                        }
                                                    args.push_str(&delta_call.function.arguments);
                                                    if extra.is_none() && delta_call.extra.is_some() {
                                                        *extra = delta_call.extra.clone();
                                                    } else if let (Some(existing), Some(new_extra)) = (extra.as_mut(), &delta_call.extra) {
                                                        for (key, value) in new_extra {
                                                            existing.entry(key.clone()).or_insert(value.clone());
                                                        }
                                                    }
                                                } else if let (Some(id), Some(name)) = (&delta_call.id, &delta_call.function.name) {
                                                    tool_call_data.insert(index, (id.clone(), name.clone(), delta_call.function.arguments.clone(), delta_call.extra.clone()));
                                                }
                                            }
                                        }
                                    }
                                    if tool_chunk.choices[0].finish_reason.is_some() {
                                        done = true;
                                    }
                                } else {
                                    done = true;
                                }
                            }
                        } else {
                            break;
                        }
                    }
                }

                let _metadata: Option<ProviderMetadata> = if !accumulated_reasoning.is_empty() {
                    let mut map = ProviderMetadata::new();
                    map.insert("reasoning_details".to_string(), json!(accumulated_reasoning));
                    Some(map)
                } else {
                    None
                };

                let filtered = think_filter.push("");
                let mut flush_thinking = String::new();
                if !saw_structured_reasoning {
                    flush_thinking.push_str(&pending_inline_thinking);
                    flush_thinking.push_str(&filtered.thinking);
                }
                pending_inline_thinking.clear();
                if !filtered.content.is_empty() || !flush_thinking.is_empty() {
                    let mut filtered_contents = Vec::new();
                    if !filtered.content.is_empty() {
                        filtered_contents.push(MessageContentBlock::text(filtered.content));
                    }
                    if !flush_thinking.is_empty() {
                        filtered_contents.push(MessageContentBlock::thinking(flush_thinking, ""));
                    }

                    if !filtered_contents.is_empty() {
                        let mut msg = Message::new(
                            Role::Assistant,
                            chrono::Utc::now().timestamp(),
                            filtered_contents,
                        );

                        if let Some(id) = chunk.id.clone() {
                            msg = msg.with_id(id);
                        }

                        yield (Some(msg), None);
                    }
                }

                let mut contents = Vec::new();
                if yielded_reasoning_content_len < accumulated_reasoning_content.len()
                    && let Some(unyielded_reasoning) =
                        accumulated_reasoning_content.get(yielded_reasoning_content_len..)
                        && !unyielded_reasoning.is_empty() {
                            contents.push(MessageContentBlock::thinking(unyielded_reasoning, ""));
                        }
                accumulated_reasoning_content.clear();
                yielded_reasoning_content_len = 0;
                let mut sorted_indices: Vec<_> = tool_call_data.keys().cloned().collect();
                sorted_indices.sort();

                for index in sorted_indices {
                    if let Some((id, function_name, arguments, extra_fields)) = tool_call_data.get(&index) {
                        let metadata = if let Some(sig) = &last_signature {
                            let mut combined = extra_fields.clone().unwrap_or_default();
                            combined.insert(
                                GEMINI_THOUGHT_SIGNATURE_KEY.to_string(),
                                json!(sig)
                            );
                            Some(combined)
                        } else {
                            extra_fields.as_ref().filter(|m| !m.is_empty()).cloned()
                        };

                        let content = if output_token_limit_reached {
                            MessageContentBlock::tool_request_with_provider_index(
                                id.clone(),
                                Err(output_token_limit_tool_error(function_name, id)),
                                metadata.as_ref(),
                                index,
                            )
                        } else if arguments.is_empty() {
                            MessageContentBlock::tool_request_with_provider_index(
                                id.clone(),
                                Ok(CallToolRequestParams::new(function_name.clone()).with_arguments(object(json!({})))),
                                metadata.as_ref(),
                                index,
                            )
                        } else {
                            match parse_tool_arguments(arguments) {
                                Some(params) if params.is_object() => MessageContentBlock::tool_request_with_provider_index(
                                    id.clone(),
                                    Ok(CallToolRequestParams::new(function_name.clone()).with_arguments(object(params))),
                                    metadata.as_ref(),
                                    index,
                                ),
                                // Valid JSON but NOT an object (a bare array/string/number).
                                // Surface a tool error so the model retries instead of
                                // crashing the run (rmcp's `object()` debug-asserts on
                                // non-objects). Mirrors the non-streaming decoder.
                                Some(other) => {
                                    let error = ErrorData {
                                        code: ErrorCode::INVALID_PARAMS,
                                        message: Cow::from(format!(
                                            "Tool arguments for {} (id {}) must be a JSON object, got {}. Raw arguments: '{}'",
                                            function_name, id, describe_json_value(&other), arguments
                                        )),
                                        data: None,
                                    };
                                    MessageContentBlock::tool_request_with_provider_index(id.clone(), Err(error), metadata.as_ref(), index)
                                }
                                None => {
                                    let message_text = truncation_error_message(arguments)
                                        .unwrap_or_else(|| {
                                            format!("Could not interpret tool use parameters for id {id}")
                                        });
                                    let error = ErrorData {
                                        code: ErrorCode::INVALID_PARAMS,
                                        message: Cow::from(message_text),
                                        data: None,
                                    };
                                    MessageContentBlock::tool_request_with_provider_index(id.clone(), Err(error), metadata.as_ref(), index)
                                }
                            }
                        };

                        contents.push(content);
                    }
                }

                let mut msg = Message::new(
                    Role::Assistant,
                    chrono::Utc::now().timestamp(),
                    contents,
                );

                // Add ID if present
                if let Some(id) = chunk.id {
                    msg = msg.with_id(id);
                }
                msg.metadata.output_token_limit_reached = output_token_limit_reached;
                output_token_limit_metadata_emitted |= output_token_limit_reached;

                usage_emitted |= usage.is_some();
                yield (
                    Some(msg),
                    usage,
                )
            } else if chunk.choices[0].delta.content.is_some() || chunk.choices[0].delta.reasoning_text().is_some() {
                let mut content = Vec::new();

                if let Some(reasoning) = chunk.choices[0].delta.reasoning_text() {
                    let signature = last_signature.as_deref().unwrap_or("");
                    content.push(MessageContentBlock::thinking(reasoning, signature));
                    yielded_reasoning_content_len = accumulated_reasoning_content.len();
                }

                let (text_content, thought_signature) = extract_content_and_signature(chunk.choices[0].delta.content.as_ref());

                if let Some(sig) = thought_signature {
                    last_signature = Some(sig);
                }

                if let Some(text) = text_content {
                    let filtered = think_filter.push(&text);

                    if !saw_structured_reasoning && !filtered.thinking.is_empty() {
                        pending_inline_thinking.push_str(&filtered.thinking);
                    }

                    if !filtered.content.is_empty() {
                        content.push(MessageContentBlock::text(filtered.content));
                    }
                }

                if !content.is_empty() {
                    let mut msg = Message::new(
                        Role::Assistant,
                        chrono::Utc::now().timestamp(),
                        content,
                    );

                    if let Some(id) = chunk.id {
                        msg = msg.with_id(id);
                    }

                    let final_usage = if chunk.choices[0].finish_reason.is_some() {
                        usage
                    } else {
                        None
                    };
                    usage_emitted |= final_usage.is_some();
                    yield (Some(msg), final_usage)
                } else if usage.is_some() {
                    usage_emitted = true;
                    yield (None, usage)
                }
            } else if usage.is_some() {
                usage_emitted = true;
                yield (None, usage)
            }
        }

        let filtered = think_filter.finish();
        let mut trailing_thinking = String::new();
        if !saw_structured_reasoning {
            trailing_thinking.push_str(&pending_inline_thinking);
            trailing_thinking.push_str(&filtered.thinking);
        }
        pending_inline_thinking.clear();

        if !filtered.content.is_empty() || !trailing_thinking.is_empty() {
            let mut content = Vec::new();

            if !filtered.content.is_empty() {
                content.push(MessageContentBlock::text(filtered.content));
            }

            if !trailing_thinking.is_empty() {
                content.push(MessageContentBlock::thinking(trailing_thinking, ""));
            }

            let mut message = Message::new(
                Role::Assistant,
                chrono::Utc::now().timestamp(),
                content,
            );
            if let Some(id) = last_response_id.clone() {
                message = message.with_id(id);
            }
            message.metadata.output_token_limit_reached =
                output_token_limit_reached && !output_token_limit_metadata_emitted;
            output_token_limit_metadata_emitted |= message.metadata.output_token_limit_reached;

            yield (Some(message), None)
        }

        if output_token_limit_reached && !output_token_limit_metadata_emitted {
            yield (Some(output_token_limit_marker(last_response_id.clone())), None)
        }

        if !usage_emitted && (last_response_id.is_some() || last_finish_reason.is_some()) {
            let mut usage = ProviderUsage::new(
                last_seen_model.unwrap_or_else(|| "unknown".to_string()),
                Usage::default(),
            );
            usage.response_id = last_response_id;
            usage.finish_reasons = last_finish_reason.map(|reason| vec![reason]);
            yield (None, Some(usage))
        }
    }
}

pub fn create_request(
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    image_format: &ImageFormat,
    for_streaming: bool,
) -> anyhow::Result<Value, Error> {
    create_request_with_options(
        model_config,
        system,
        messages,
        tools,
        image_format,
        for_streaming,
        OpenAiFormatOptions {
            preserve_thinking_context: true,
            supports_vision: model_config.supports_vision.unwrap_or_default(),
            ..Default::default()
        },
    )
}

pub fn create_request_with_options(
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    image_format: &ImageFormat,
    for_streaming: bool,
    format_options: OpenAiFormatOptions,
) -> anyhow::Result<Value, Error> {
    let (wire_model_name, _) = extract_reasoning_effort(&model_config.model_name);
    create_request_for_model_with_options(
        model_config,
        &wire_model_name,
        &model_config.model_name,
        system,
        messages,
        tools,
        image_format,
        for_streaming,
        format_options,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn create_request_for_model_with_options(
    model_config: &ModelConfig,
    wire_model_name: &str,
    capability_model_name: &str,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    image_format: &ImageFormat,
    for_streaming: bool,
    format_options: OpenAiFormatOptions,
) -> anyhow::Result<Value, Error> {
    if model_config.model_name.starts_with("o1-mini") {
        return Err(anyhow!(
            "o1-mini model is not currently supported since goose uses tool calling and o1-mini does not support it. Please use o1 or o3 models instead."
        ));
    }

    let (model_name, legacy_reasoning_effort) = extract_reasoning_effort(capability_model_name);
    let is_reasoning_model = is_openai_responses_model(&model_name);
    let supports_xai_effort = supports_xai_reasoning_effort(&model_name);
    let reasoning_effort = if is_reasoning_model {
        model_config
            .thinking_effort()
            .map_or(legacy_reasoning_effort, |effort| {
                openai_reasoning_effort_for_thinking(&model_name, effort)
            })
    } else if supports_xai_effort {
        model_config
            .thinking_effort()
            .and_then(|effort| xai_reasoning_effort_for_thinking(&model_name, effort))
    } else {
        None
    };

    let system_message = json!({
        "role": if is_reasoning_model { "developer" } else { "system" },
        "content": system
    });

    let messages_spec = format_messages_with_options(messages, image_format, format_options);
    let mut tools_spec = format_tools(tools)?;

    validate_tool_schemas(&mut tools_spec);

    let mut messages_array = vec![system_message];
    messages_array.extend(messages_spec);

    let mut payload = json!({
        "model": wire_model_name,
        "messages": messages_array
    });

    if let Some(effort) = reasoning_effort {
        payload["reasoning_effort"] = json!(effort);
    }

    if !tools_spec.is_empty() {
        payload["tools"] = json!(tools_spec);
    }

    if !is_reasoning_model
        && !supports_xai_effort
        && let Some(temp) = model_config.temperature
    {
        payload["temperature"] = json!(temp);
    }

    // Only emit max_tokens / max_completion_tokens when the user (via
    // GOOSE_MAX_TOKENS) or a canonical model record has supplied a value.
    // For unknown models on OpenAI-compatible endpoints (e.g. llama_swap,
    // lmstudio) sending the historic 4096 default truncates non-trivial
    // responses; omitting the field lets the server use its own max.
    if let Some(max_tokens) = model_config.max_tokens {
        let key = if is_reasoning_model {
            "max_completion_tokens"
        } else {
            "max_tokens"
        };
        payload
            .as_object_mut()
            .unwrap()
            .insert(key.to_string(), json!(max_tokens));
    }

    if for_streaming {
        payload["stream"] = json!(true);
        payload["stream_options"] = json!({"include_usage": true});
    }

    if let Some(params) = &model_config.request_params
        && let Some(obj) = payload.as_object_mut()
    {
        for (key, value) in params {
            if !is_goose_internal_request_param(key) && !is_reserved_request_param_key(key) {
                obj.insert(key.clone(), value.clone());
            }
        }
    }

    Ok(payload)
}

/// Extract an explicit reasoning-effort suffix from a model name.
///
/// Returns `(base_model_name, Some(effort))` when the user appended a
/// recognised suffix like `-high` or `-xhigh`, e.g. `gpt-5.4-high` →
/// `("gpt-5.4", Some("high"))`.
///
/// When no suffix is present the effort is `None` — callers should omit
/// the `reasoning` field entirely so the API applies its own per-model
/// default. This avoids hard-coding a default that may be invalid for
/// certain models (e.g. `gpt-5-pro` only accepts `high`; older o-series
/// models reject `none` and `xhigh`).
pub fn extract_reasoning_effort(model_name: &str) -> (String, Option<String>) {
    if !is_openai_responses_model(model_name) {
        return (model_name.to_string(), None);
    }

    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)^(?P<base>.+)-(?P<effort>none|low|medium|high|xhigh)$").unwrap()
    });

    if let Some(captures) = re.captures(model_name) {
        let base = captures["base"].to_string();
        let effort = captures["effort"].to_ascii_lowercase();
        return (base, Some(effort));
    }

    (model_name.to_string(), None)
}

/// True when the model should use the OpenAI Responses API.
///
/// The Responses API is backwards-compatible with all OpenAI reasoning
/// models, so every `o`-series (`o1`, `o3`, `o4`, …), `gpt-5`, and `gpt-6` variant
/// routes here. The matcher intentionally scans the full model identifier so
/// hosted aliases like `databricks-gpt-5.4`, `goose-o3-mini`, or
/// `headless-goose-o3-mini` work without provider-specific normalization.
pub fn is_openai_responses_model(model_name: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| {
        Regex::new(r"(?i)(?:^|[-/])(?:o\d+(?:$|-)|gpt-(?:5|6)(?:$|[-.]))").unwrap()
    });
    re.is_match(model_name)
}

/// Returns whether an xAI Chat Completions model accepts `reasoning_effort`.
pub fn supports_xai_reasoning_effort(model_name: &str) -> bool {
    let model_name = model_name.to_ascii_lowercase();

    model_name.starts_with("grok-4.5")
        || model_name.starts_with("grok-4.3")
        || model_name.starts_with("grok-3-mini")
}

/// Returns whether an xAI model performs server-side reasoning.
pub fn is_xai_reasoning_model(model_name: &str) -> bool {
    let model_name = model_name.to_ascii_lowercase();

    if model_name.contains("non-reasoning") || model_name.contains("non_reasoning") {
        return false;
    }

    supports_xai_reasoning_effort(&model_name)
        || model_name.starts_with("grok-4.20")
        || model_name.starts_with("grok-4-0709")
        || model_name.starts_with("grok-4-fast-reasoning")
        || model_name.starts_with("grok-4-1-fast-reasoning")
}

/// Maps Goose's effort levels to values accepted by xAI Chat Completions.
pub fn xai_reasoning_effort_for_thinking(
    model_name: &str,
    effort: ThinkingEffort,
) -> Option<String> {
    let model_name = model_name.to_ascii_lowercase();
    let supports_none = model_name.starts_with("grok-4.3");
    let supports_medium = !model_name.starts_with("grok-3-mini");

    match effort {
        ThinkingEffort::Off if supports_none => Some("none".to_string()),
        ThinkingEffort::Off => Some("low".to_string()),
        ThinkingEffort::Low => Some("low".to_string()),
        ThinkingEffort::Medium if supports_medium => Some("medium".to_string()),
        ThinkingEffort::Medium | ThinkingEffort::High | ThinkingEffort::Max => {
            Some("high".to_string())
        }
    }
}

pub fn openai_reasoning_effort_for_thinking(
    model_name: &str,
    effort: ThinkingEffort,
) -> Option<String> {
    let supported = openai_reasoning_efforts_for_model(model_name);

    let preferred: &[&str] = match effort {
        ThinkingEffort::Off => &["none", "low"],
        ThinkingEffort::Low => &["low", "medium", "high", "xhigh"],
        ThinkingEffort::Medium => &["medium", "high", "low", "xhigh"],
        ThinkingEffort::High => &["high", "medium", "xhigh", "low"],
        ThinkingEffort::Max => &["max", "xhigh", "high", "medium", "low"],
    };

    preferred
        .iter()
        .find(|level| supported.contains(level))
        .map(|level| (*level).to_string())
}

pub(crate) fn openai_reasoning_efforts_for_model(model_name: &str) -> &'static [&'static str] {
    let normalized = model_name.to_ascii_lowercase();

    if normalized.contains("gpt-5") || normalized.contains("gpt-6") {
        if normalized.contains("-pro") || normalized.contains("/pro") {
            &["high"]
        } else if normalized.contains("gpt-6") {
            // GPT-6 Astra and GPT-6.1 Sol require reasoning; GPT-6 Sol and Luna may disable it.
            let is_gpt_6_1_sol = normalized
                .match_indices("gpt-6.1-sol")
                .any(|(index, name)| {
                    let (prefix, rest) = normalized.split_at(index);
                    let (_, suffix) = rest.split_at(name.len());
                    (prefix.is_empty() || prefix.ends_with(['/', '.', '-']))
                        && (suffix.is_empty() || suffix.starts_with(['-', '@']))
                });
            if normalized.contains("astra") || is_gpt_6_1_sol {
                &["low", "medium", "high", "xhigh", "max"]
            } else {
                &["none", "low", "medium", "high", "xhigh", "max"]
            }
        } else if normalized.contains("gpt-5.4")
            || normalized.contains("gpt-5-4")
            || normalized.contains("gpt-5.5")
            || normalized.contains("gpt-5-5")
            || normalized.contains("gpt-5.6")
            || normalized.contains("gpt-5-6")
        {
            &["none", "low", "medium", "high", "xhigh"]
        } else {
            &["low", "medium", "high"]
        }
    } else {
        &["low", "medium", "high"]
    }
}

const MAX_FUNCTION_NAME_LENGTH: usize = 128;

pub fn sanitize_function_name(name: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"[^a-zA-Z0-9_-]").unwrap());
    re.replace_all(name, "_")
        .chars()
        .take(MAX_FUNCTION_NAME_LENGTH)
        .collect()
}

pub fn is_valid_function_name(name: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r"^[a-zA-Z0-9_-]+$").unwrap());
    re.is_match(name)
}

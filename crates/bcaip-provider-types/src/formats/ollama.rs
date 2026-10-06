//! Ollama-specific response handling with XML tool call fallback.
//!
//! Some models running through Ollama (notably Qwen3-coder) output XML-style tool calls
//! when given many tools (6+), instead of using the native JSON tool_calls format.
//! This module wraps the standard OpenAI response parsing with XML fallback logic,
//! isolating this behavior to the Ollama provider only.
//!
//! Known affected models:
//! - qwen3-coder
//! - qwen3-coder-32b
use crate::conversations::{Message, MessageContentBlock, ProviderUsage};
use crate::formats::{
    is_valid_function_name, response_to_message_openai, response_to_streaming_message_openai,
};
use crate::maybe_send::MaybeSend;
use async_stream::try_stream;
use chrono;
use futures::Stream;
use regex::Regex;
use rmcp::model::{CallToolRequestParams, ErrorCode, ErrorData, Role, object};
use serde_json::Value;
use std::borrow::Cow;
use uuid::Uuid;
/// Parse XML-style tool calls from content (Ollama/Qwen3-coder fallback format).
///
/// Format: `<function=name><parameter=key>value</parameter>...</function>`
///
/// Returns a tuple of (prefix_text, tool_calls) where prefix_text is any text before the first function tag.
pub fn parse_xml_tool_calls(content: &str) -> (Option<String>, Vec<MessageContentBlock>) {
    let mut tool_calls = Vec::new();

    let function_re = Regex::new(r"<function=([^>]+)>([\s\S]*?)</function>").unwrap();
    let param_re = Regex::new(r"<parameter=([^>]+)>([\s\S]*?)</parameter>").unwrap();

    let prefix = content
        .find("<function=")
        .and_then(|idx| content.get(..idx))
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    for func_cap in function_re.captures_iter(content) {
        let function_name = func_cap[1].trim().to_string();
        let function_body = &func_cap[2];
        let mut arguments = serde_json::Map::new();
        for param_cap in param_re.captures_iter(function_body) {
            let param_name = param_cap[1].trim().to_string();
            let param_value = param_cap[2].trim().to_string();
            arguments.insert(param_name, serde_json::Value::String(param_value));
        }

        let id = Uuid::new_v4().to_string();

        if is_valid_function_name(&function_name) {
            tool_calls.push(MessageContentBlock::tool_request(
                id,
                Ok(CallToolRequestParams::new(function_name)
                    .with_arguments(object(serde_json::Value::Object(arguments)))),
            ));
        } else {
            let error = ErrorData {
                code: ErrorCode::INVALID_REQUEST,
                message: Cow::from(format!(
                    "The provided function name '{}' had invalid characters, it must match this regex [a-zA-Z0-9_-]+",
                    function_name
                )),
                data: None,
            };
            tool_calls.push(MessageContentBlock::tool_request(id, Err(error)));
        }
    }

    (prefix, tool_calls)
}

/// Convert OpenAI's API response to internal Message format, with XML tool call fallback.
///
/// This wraps the standard OpenAI response parsing and adds XML fallback for models
/// like Qwen3-coder that output XML tool calls when given many tools.
pub fn response_to_message_ollama(response: &Value) -> anyhow::Result<Message> {
    let message = response_to_message_openai(response)?;

    let has_tool_requests = message
        .content
        .iter()
        .any(|c| matches!(c, MessageContentBlock::ToolRequest(_)));

    if has_tool_requests {
        return Ok(message);
    }

    let original = response
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|m| m.get("message"));

    if let Some(original) = original
        && let Some(text) = original.get("content").and_then(|c| c.as_str())
        && text.contains("<function=")
    {
        let (prefix, xml_tool_calls) = parse_xml_tool_calls(text);
        if !xml_tool_calls.is_empty() {
            let mut content = Vec::new();
            if let Some(prefix_text) = prefix {
                content.push(MessageContentBlock::text(prefix_text));
            }
            content.extend(xml_tool_calls);

            return Ok(Message::new(
                Role::Assistant,
                chrono::Utc::now().timestamp(),
                content,
            ));
        }
    }

    Ok(message)
}

/// Extract text content from a message's content items.
fn extract_text_from_message(message: &Message) -> String {
    message
        .content
        .iter()
        .filter_map(|c| {
            if let MessageContentBlock::Text(text) = c {
                Some(text.text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("")
}

/// Check if a message contains only text content (no tool requests/responses).
fn is_text_only_message(message: &Message) -> bool {
    message
        .content
        .iter()
        .all(|c| matches!(c, MessageContentBlock::Text(_)))
}

/// Streaming message handler with XML tool call post-processing for Ollama.
///
/// This wraps the standard OpenAI streaming handler and post-processes messages
/// to detect and parse XML tool calls. When XML markers are detected in text
/// messages, it buffers them until the stream completes, then parses and emits
/// the tool calls.
///
/// This approach avoids exposing any internal types from openai.rs.
pub fn response_to_streaming_message_ollama<S>(
    stream: S,
) -> impl Stream<Item = anyhow::Result<(Option<Message>, Option<ProviderUsage>)>> + 'static
where
    S: Stream<Item = anyhow::Result<String>> + Unpin + MaybeSend + 'static,
{
    try_stream! {
        use futures::StreamExt;
        let base_stream = response_to_streaming_message_openai(stream);
        let mut base_stream = std::pin::pin!(base_stream);

        let mut accumulated_text = String::new();
        let mut xml_detected = false;
        let mut buffered_usage: Option<ProviderUsage> = None;

        while let Some(result) = base_stream.next().await {
            let (message_opt, usage) = result?;
            if usage.is_some() {
                buffered_usage = usage.clone();
            }

            if let Some(message) = message_opt {
                if is_text_only_message(&message) {
                    let text = extract_text_from_message(&message);
                    accumulated_text.push_str(&text);

                    if !xml_detected && accumulated_text.contains("<function=") {
                        xml_detected = true;
                    }

                    if xml_detected {
                        continue;
                    }
                }

                yield (Some(message), usage);
            } else if usage.is_some() && !xml_detected {
                yield (None, usage);
            }
        }

        if xml_detected && !accumulated_text.is_empty() {
            let (prefix, xml_tool_calls) = parse_xml_tool_calls(&accumulated_text);

            if !xml_tool_calls.is_empty() {
                let mut contents = Vec::new();
                if let Some(prefix_text) = prefix {
                    contents.push(MessageContentBlock::text(prefix_text));
                }
                contents.extend(xml_tool_calls);

                let msg = Message::new(
                    Role::Assistant,
                    chrono::Utc::now().timestamp(),
                    contents,
                );

                yield (Some(msg), buffered_usage);
            } else {
                let msg = Message::new(
                    Role::Assistant,
                    chrono::Utc::now().timestamp(),
                    vec![MessageContentBlock::text(&accumulated_text)],
                )
                .with_generated_id();

                yield (Some(msg), buffered_usage);
            }
        }
    }
}

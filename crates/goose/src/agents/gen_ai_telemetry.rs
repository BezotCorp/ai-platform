use crate::session::Session;
use bcaip_provider_types::conversations::{Message, MessageContent, ToolResult};
use bcaip_provider_types::conversations::{ProviderUsage, Usage};
use bcaip_provider_types::model::ModelConfig;
use rmcp::model::{CallToolRequestParams, CallToolResult, Role};
use serde_json::{Value, json};
use tracing::Span;
pub(super) const CAPTURE_MESSAGE_CONTENT_ENV: &str =
    "OTEL_INSTRUMENTATION_GENAI_CAPTURE_MESSAGE_CONTENT";

pub(super) fn capture_message_content() -> bool {
    std::env::var(CAPTURE_MESSAGE_CONTENT_ENV).is_ok_and(|value| value.eq_ignore_ascii_case("true"))
}

pub(super) fn input_messages_json(messages: &[Message]) -> String {
    Value::Array(messages.iter().map(message_json).collect()).to_string()
}

pub(super) fn simple_input_json(text: &str) -> String {
    json!([{"role": "user", "content": text}]).to_string()
}

pub(super) fn simple_output_json(text: &str) -> String {
    json!([{"role": "assistant", "content": text, "finish_reason": "stop"}]).to_string()
}

pub(super) fn output_message_json(message: &Message) -> String {
    // Message does not retain provider finish reasons; tool requests are the only
    // distinct completion signal available after streaming.
    let finish_reason = if message
        .content
        .iter()
        .any(|content| matches!(content, MessageContent::ToolRequest(_)))
    {
        "tool_call"
    } else {
        "stop"
    };
    let mut value = message_json(message);
    value["finish_reason"] = Value::String(finish_reason.to_string());
    Value::Array(vec![value]).to_string()
}

pub(super) fn append_message(accumulated: &mut Option<Message>, message: &Message) {
    match accumulated {
        Some(accumulated) => accumulated.content.extend(message.content.iter().cloned()),
        None => *accumulated = Some(message.clone()),
    }
}

pub(super) fn record_usage(span: &Span, usage: &Usage) {
    if let Some(tokens) = usage.input_tokens {
        span.record("gen_ai.usage.input_tokens", tokens);
    }
    if let Some(tokens) = usage.output_tokens {
        span.record("gen_ai.usage.output_tokens", tokens);
    }
    if let Some(tokens) = usage.cache_read_input_tokens {
        span.record("gen_ai.usage.cache_read.input_tokens", tokens);
    }
    if let Some(tokens) = usage.cache_write_input_tokens {
        span.record("gen_ai.usage.cache_creation.input_tokens", tokens);
    }
}

pub(super) fn record_provider_usage(span: &Span, usage: &ProviderUsage) {
    span.record("gen_ai.response.model", usage.model.as_str());
    record_usage(span, &usage.usage);
    if let Some(reasons) = &usage.finish_reasons {
        let reasons_json = serde_json::to_string(reasons).unwrap_or_default();
        span.record("gen_ai.response.finish_reasons", reasons_json.as_str());
    }
    if let Some(id) = &usage.response_id {
        span.record("gen_ai.response.id", id.as_str());
    }
}

pub(super) fn record_request_params(span: &Span, model_config: &ModelConfig) {
    if let Some(temperature) = model_config.temperature {
        span.record("gen_ai.request.temperature", temperature as f64);
    }
    if let Some(max_tokens) = model_config.max_tokens {
        span.record("gen_ai.request.max_tokens", max_tokens as i64);
    }
}

pub(super) fn record_tool_arguments(span: &Span, tool_call: &CallToolRequestParams) {
    if capture_message_content() {
        let arguments = tool_call
            .arguments
            .as_ref()
            .map(|arguments| Value::Object(arguments.clone()))
            .unwrap_or_else(|| Value::Object(serde_json::Map::new()));
        span.record(
            "gen_ai.tool.call.arguments",
            tracing::field::display(arguments),
        );
    }
}

pub(super) fn record_tool_result(span: &Span, result: &ToolResult<CallToolResult>) {
    if capture_message_content()
        && let Some(result_json) = successful_tool_result_json(result)
    {
        span.record("gen_ai.tool.call.result", result_json.as_str());
    }
}

pub(super) fn agent_name(session: &Session) -> &str {
    session
        .recipe
        .as_ref()
        .map_or("goose", |recipe| recipe.title.as_str())
}

pub(super) fn tool_result_json(result: &ToolResult<CallToolResult>) -> String {
    match result {
        Ok(result) if result.is_error != Some(true) => json!({
            "status": "success",
            "value": result,
        }),
        Ok(result) => json!({
            "status": "error",
            "value": result,
        }),
        Err(error) => json!({
            "status": "error",
            "error": error.to_string(),
        }),
    }
    .to_string()
}

pub(super) fn successful_tool_result_json(result: &ToolResult<CallToolResult>) -> Option<String> {
    match result {
        Ok(result) if result.is_error != Some(true) => {
            Some(serde_json::to_string(result).expect("CallToolResult must serialize"))
        }
        _ => None,
    }
}

fn message_json(message: &Message) -> Value {
    let role = if !message.content.is_empty()
        && message
            .content
            .iter()
            .all(|content| matches!(content, MessageContent::ToolResponse(_)))
    {
        "tool"
    } else {
        match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        }
    };

    let parts = consolidated_parts(&message.content);
    json!({
        "role": role,
        "parts": parts,
    })
}

/// Merge consecutive text and reasoning parts into single entries so that
/// streaming tokens don't each get their own JSON object in the OTEL output.
fn consolidated_parts(content: &[MessageContent]) -> Vec<Value> {
    let mut result: Vec<Value> = Vec::new();
    for item in content {
        let value = message_part_json(item);
        let item_type = value.get("type").and_then(|v| v.as_str());
        if matches!(item_type, Some("text" | "reasoning"))
            && let Some(last) = result.last_mut()
            && last.get("type") == value.get("type")
            && let (Some(existing), Some(new_content)) = (
                last.get("content").and_then(|v| v.as_str()),
                value.get("content").and_then(|v| v.as_str()),
            )
        {
            last["content"] = Value::String(format!("{}{}", existing, new_content));
            continue;
        }
        result.push(value);
    }
    result
}

fn tool_call_part(id: &str, tool_call: &ToolResult<CallToolRequestParams>) -> Value {
    match tool_call {
        Ok(tool_call) => json!({
            "type": "tool_call",
            "id": id,
            "name": tool_call.name,
            "arguments": tool_call
                .arguments
                .as_ref()
                .map(|arguments| Value::Object(arguments.clone()))
                .unwrap_or_else(|| Value::Object(serde_json::Map::new())),
        }),
        Err(error) => json!({
            "type": "tool_call_error",
            "id": id,
            "error": error.to_string(),
        }),
    }
}

fn message_part_json(content: &MessageContent) -> Value {
    match content {
        MessageContent::Text(text) => json!({
            "type": "text",
            "content": text.text,
        }),
        MessageContent::Image(image) => json!({
            "type": "blob",
            "modality": "image",
            "mime_type": image.mime_type,
            "content": image.data,
        }),
        MessageContent::Document(document) => json!({
            "type": "blob",
            "modality": "document",
            "mime_type": document.mime_type,
            "name": document.name,
            "content": document.data,
        }),
        MessageContent::ToolRequest(request) => tool_call_part(&request.id, &request.tool_call),
        MessageContent::ToolResponse(response) => json!({
            "type": "tool_call_response",
            "id": response.id,
            "response": match &response.tool_result {
                Ok(result) => serde_json::to_value(result)
                    .expect("CallToolResult must serialize"),
                Err(error) => json!({ "error": error.to_string() }),
            },
        }),
        MessageContent::Thinking(thinking) => json!({
            "type": "reasoning",
            "content": thinking.thinking,
        }),
        MessageContent::RedactedThinking(_) => json!({
            "type": "redacted_reasoning",
        }),
        MessageContent::ToolConfirmationRequest(request) => json!({
            "type": "tool_confirmation",
            "id": request.id,
            "name": request.tool_name,
            "arguments": request.arguments,
        }),
        MessageContent::ActionRequired(action) => json!({
            "type": "action_required",
            "data": action.data,
        }),
        MessageContent::SystemNotification(notification) => json!({
            "type": "system_notification",
            "content": notification.msg,
        }),
        MessageContent::Error(error) => json!({
            "type": "error",
            "kind": error.kind,
            "content": error.message,
        }),
    }
}

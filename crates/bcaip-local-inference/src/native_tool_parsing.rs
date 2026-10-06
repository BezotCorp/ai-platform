use crate::native_tool_format::{
    DEEPSEEK_CALL_BEGIN, DEEPSEEK_CALLS_BEGIN, FUNCTION_OPEN, LLAMA3_PYTHON_TAG,
    MISTRAL_TOOL_CALLS, TOOL_CALL_CLOSE, TOOL_CALL_OPEN,
};
use goose_provider_types::conversations::{Message, MessageContent};
use goose_provider_types::{
    errors::ProviderError, formats::is_valid_function_name, json::safely_parse_json,
};
use rmcp::model::{CallToolRequestParams, ErrorCode, ErrorData, object};
use serde_json::{Value, json};
use std::borrow::Cow;
use uuid::Uuid;

pub(crate) fn message_from_native_tool_text(
    generated_text: &str,
    message_id: &str,
) -> Result<Option<Message>, ProviderError> {
    let mut content = Vec::new();

    if serde_json::from_str::<Value>(generated_text.trim()).is_err()
        && let Some((prefix, tool_calls)) = parse_marked_tool_calls(generated_text)
    {
        if let Some(prefix) = prefix {
            content.push(MessageContent::text(prefix));
        }
        content.extend(tool_calls);
        let mut message = Message::new(
            rmcp::model::Role::Assistant,
            chrono::Utc::now().timestamp(),
            content,
        );
        message.id = Some(message_id.to_string());
        return Ok(Some(message));
    }

    if let Some(message) = parse_openai_message_json(generated_text) {
        append_text(&mut content, message.get("content"));
        append_tool_calls(&mut content, message.get("tool_calls"));
    } else if let Some(tool_calls) = parse_tool_calls_json(generated_text) {
        append_tool_calls(&mut content, Some(&tool_calls));
    } else if serde_json::from_str::<Value>(generated_text.trim()).is_ok() {
        return Ok(None);
    } else if generated_text.contains("<function=") {
        let (prefix, tool_calls) = parse_xml_tool_calls(generated_text);
        if let Some(prefix) = prefix {
            content.push(MessageContent::text(prefix));
        }
        content.extend(tool_calls);
    } else {
        return Ok(None);
    }

    if content
        .iter()
        .any(|content| matches!(content, MessageContent::ToolRequest(_)))
    {
        let mut message = Message::new(
            rmcp::model::Role::Assistant,
            chrono::Utc::now().timestamp(),
            content,
        );
        message.id = Some(message_id.to_string());
        Ok(Some(message))
    } else {
        Ok(None)
    }
}

fn parse_marked_tool_calls(text: &str) -> Option<(Option<String>, Vec<MessageContent>)> {
    let (prefix_end, tool_calls) = if let Some(start) = text.find(TOOL_CALL_OPEN) {
        (start, tool_call_tag_calls(text))
    } else if let Some(start) = text.find(MISTRAL_TOOL_CALLS) {
        let body = text.get(start + MISTRAL_TOOL_CALLS.len()..)?;
        (start, tool_calls_from_json_text(body))
    } else if let Some(start) = text.find(LLAMA3_PYTHON_TAG) {
        let body = text.get(start + LLAMA3_PYTHON_TAG.len()..)?;
        (start, tool_calls_from_json_text(body))
    } else {
        let start = text.find(DEEPSEEK_CALL_BEGIN)?;
        let prefix_end = text.find(DEEPSEEK_CALLS_BEGIN).unwrap_or(start);
        (prefix_end, parse_deepseek_tool_calls(text.get(start..)?))
    };

    if tool_calls.is_empty() {
        return None;
    }

    let prefix = text
        .get(..prefix_end)
        .map(str::trim)
        .filter(|prefix| !prefix.is_empty())
        .map(ToString::to_string);
    Some((prefix, tool_calls))
}

fn tool_call_tag_calls(text: &str) -> Vec<MessageContent> {
    text.split(TOOL_CALL_OPEN)
        .skip(1)
        .flat_map(|segment| {
            let inner = segment.split(TOOL_CALL_CLOSE).next().unwrap_or(segment);
            if inner.contains(FUNCTION_OPEN) {
                parse_xml_tool_calls(inner).1
            } else {
                tool_calls_from_json_text(inner)
            }
        })
        .collect()
}

fn tool_calls_from_json_text(text: &str) -> Vec<MessageContent> {
    for value in json_candidates(text) {
        if is_tool_call_array(&value) {
            return value
                .as_array()
                .map(|items| items.iter().map(tool_call_content).collect())
                .unwrap_or_default();
        }
        if let Some(items) = value
            .get("tool_calls")
            .filter(|calls| is_tool_call_array(calls))
            .and_then(|calls| calls.as_array())
        {
            return items.iter().map(tool_call_content).collect();
        }
        if is_tool_call_value(&value) {
            return vec![tool_call_content(&value)];
        }
    }
    Vec::new()
}

fn parse_deepseek_tool_calls(text: &str) -> Vec<MessageContent> {
    let call_re = regex::Regex::new(
        r"<｜tool▁call▁begin｜>\s*function<｜tool▁sep｜>([^\n]+)\n```json\n([\s\S]*?)\n```\s*<｜tool▁call▁end｜>",
    )
    .unwrap();
    call_re
        .captures_iter(text)
        .map(|cap| {
            tool_call_content(&json!({
                "function": {
                    "name": cap[1].trim(),
                    "arguments": cap[2].trim(),
                }
            }))
        })
        .collect()
}

fn parse_openai_message_json(generated_text: &str) -> Option<Value> {
    json_candidates(generated_text)
        .into_iter()
        .find(|value| value.get("tool_calls").is_some_and(is_tool_call_array))
}

fn parse_tool_calls_json(generated_text: &str) -> Option<Value> {
    for value in json_candidates(generated_text) {
        if is_tool_call_array(&value) {
            return Some(value);
        }
        if let Some(tool_calls) = value
            .get("tool_calls")
            .filter(|value| is_tool_call_array(value))
        {
            return Some(tool_calls.clone());
        }
        if is_tool_call_value(&value) {
            return Some(Value::Array(vec![value]));
        }
    }
    None
}

fn is_tool_call_array(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| !items.is_empty() && items.iter().all(is_tool_call_value))
}

fn is_tool_call_value(value: &Value) -> bool {
    let direct_name = value.get("name").and_then(|name| name.as_str()).is_some();
    let direct_arguments = value.get("arguments").is_some() || value.get("parameters").is_some();
    let function_name = value
        .get("function")
        .and_then(|function| function.get("name"))
        .and_then(|name| name.as_str())
        .is_some();

    (direct_name && direct_arguments) || function_name
}

fn json_candidates(text: &str) -> Vec<Value> {
    let mut candidates = Vec::new();
    let trimmed = text.trim();
    if let Ok(value) = serde_json::from_str::<Value>(trimmed) {
        candidates.push(value);
    }

    for (open, close) in [('{', '}'), ('[', ']')] {
        let starts = text.match_indices(open).map(|(idx, _)| idx);
        for start in starts {
            let mut depth = 0i32;
            let mut in_string = false;
            let mut escaped = false;
            for (offset, ch) in text[start..].char_indices() {
                if escaped {
                    escaped = false;
                    continue;
                }
                if ch == '\\' && in_string {
                    escaped = true;
                    continue;
                }
                if ch == '"' {
                    in_string = !in_string;
                    continue;
                }
                if in_string {
                    continue;
                }
                if ch == open {
                    depth += 1;
                } else if ch == close {
                    depth -= 1;
                    if depth == 0 {
                        let end = start + offset + ch.len_utf8();
                        if let Ok(value) = serde_json::from_str::<Value>(&text[start..end]) {
                            candidates.push(value);
                        }
                        break;
                    }
                }
            }
        }
    }

    candidates
}

fn append_text(content: &mut Vec<MessageContent>, value: Option<&Value>) {
    if let Some(text) = value.and_then(|value| value.as_str())
        && !text.is_empty()
    {
        content.push(MessageContent::text(text));
    }
}

fn append_tool_calls(content: &mut Vec<MessageContent>, value: Option<&Value>) {
    let Some(tool_calls) = value.and_then(|value| value.as_array()) else {
        return;
    };

    for tool_call in tool_calls {
        content.push(tool_call_content(tool_call));
    }
}

fn tool_call_content(tool_call: &Value) -> MessageContent {
    let id = tool_call
        .get("id")
        .and_then(|id| id.as_str())
        .map(ToString::to_string)
        .unwrap_or_else(|| Uuid::new_v4().to_string());

    let function = tool_call.get("function").unwrap_or(tool_call);
    let name = function
        .get("name")
        .and_then(|name| name.as_str())
        .unwrap_or_default()
        .to_string();

    if !is_valid_function_name(&name) {
        return MessageContent::tool_request(
            id,
            Err(ErrorData {
                code: ErrorCode::INVALID_REQUEST,
                message: Cow::from(format!(
                    "The provided function name '{}' had invalid characters, it must match this regex [a-zA-Z0-9_-]+",
                    name
                )),
                data: None,
            }),
        );
    }

    let arguments = function
        .get("arguments")
        .or_else(|| tool_call.get("arguments"))
        .or_else(|| function.get("parameters"))
        .or_else(|| tool_call.get("parameters"))
        .cloned()
        .unwrap_or_else(|| json!({}));

    let raw_arguments = arguments.to_string();
    let parsed = match arguments {
        Value::String(arguments) if arguments.trim().is_empty() => Ok(json!({})),
        Value::String(arguments) => safely_parse_json(&arguments),
        Value::Object(_) => Ok(arguments),
        Value::Null => Ok(json!({})),
        other => Ok(other),
    };

    match parsed {
        Ok(params) => MessageContent::tool_request(
            id,
            Ok(CallToolRequestParams::new(name).with_arguments(object(params))),
        ),
        Err(error) => {
            let message = format!(
                "Could not interpret tool use parameters for id {}: {}. Raw arguments: '{}'",
                id, error, raw_arguments
            );
            MessageContent::tool_request(
                id,
                Err(ErrorData {
                    code: ErrorCode::INVALID_PARAMS,
                    message: Cow::from(message),
                    data: None,
                }),
            )
        }
    }
}

fn parse_xml_tool_calls(content: &str) -> (Option<String>, Vec<MessageContent>) {
    let function_re = regex::Regex::new(r"<function=([^>]+)>([\s\S]*?)</function>").unwrap();
    let param_re = regex::Regex::new(r"<parameter=([^>]+)>([\s\S]*?)</parameter>").unwrap();

    let prefix = content
        .find("<function=")
        .and_then(|idx| content.get(..idx))
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(ToString::to_string);

    let mut tool_calls = Vec::new();
    for func_cap in function_re.captures_iter(content) {
        let function_name = func_cap[1].trim().to_string();
        let function_body = &func_cap[2];
        let mut arguments = serde_json::Map::new();
        for param_cap in param_re.captures_iter(function_body) {
            arguments.insert(
                param_cap[1].trim().to_string(),
                Value::String(param_cap[2].trim().to_string()),
            );
        }
        tool_calls.push(tool_call_content(&json!({
            "function": {
                "name": function_name,
                "arguments": arguments,
            }
        })));
    }

    (prefix, tool_calls)
}

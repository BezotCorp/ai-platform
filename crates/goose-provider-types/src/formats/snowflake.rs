use crate::conversation::message::{Message, MessageContentBlock};
use crate::conversation::token_usage::Usage;
use crate::documents::{UNSUPPORTED_PROVIDER_REASON, unsupported_document_text};
use crate::errors::ProviderError;
use crate::mcp_utils::extract_text_from_resource;
use crate::model::ModelConfig;
use anyhow::{Result, anyhow};
use rmcp::model::{CallToolRequestParams, Role, Tool, object};
use rmcp::object;
use serde_json::{Value, json};
use std::collections::HashSet;

/// Convert internal Message format to Snowflake's API message specification
pub fn format_messages(messages: &[Message]) -> Vec<Value> {
    let mut snowflake_messages = Vec::new();

    for message in messages {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };

        let mut text_content = String::new();

        for msg_content in &message.content {
            match msg_content {
                MessageContentBlock::Text(text) => {
                    if !text_content.is_empty() {
                        text_content.push('\n');
                    }
                    text_content.push_str(&text.text);
                }
                MessageContentBlock::ToolRequest(_tool_request) => {
                    // Skip tool requests in message formatting - tools are handled separately
                    // through the tools parameter in the API request
                    continue;
                }
                MessageContentBlock::ToolResponse(tool_response) => {
                    if let Ok(result) = &tool_response.tool_result {
                        let text = result
                            .content
                            .iter()
                            .filter_map(|c| {
                                if let Some(t) = c.as_text() {
                                    return Some(t.text.clone());
                                }
                                if let Some(r) = c.as_resource() {
                                    let text = extract_text_from_resource(&r.resource);
                                    if !text.is_empty() {
                                        return Some(text);
                                    }
                                }
                                None
                            })
                            .collect::<Vec<_>>()
                            .join("\n");

                        if !text_content.is_empty() {
                            text_content.push('\n');
                        }
                        if !text.is_empty() {
                            text_content.push_str(&format!("Tool result: {}", text));
                        }
                    }
                }
                MessageContentBlock::ToolConfirmationRequest(_) => {}
                MessageContentBlock::ActionRequired(_) => {}
                MessageContentBlock::SystemNotification(_) | MessageContentBlock::Error(_) => {
                    // Skip
                }
                MessageContentBlock::Thinking(_thinking) => {
                    // Skip thinking for now
                }
                MessageContentBlock::RedactedThinking(_redacted) => {
                    // Skip redacted thinking for now
                }
                MessageContentBlock::Image(_) => continue, // Snowflake doesn't support image content yet
                MessageContentBlock::Document(document) => {
                    if !text_content.is_empty() {
                        text_content.push('\n');
                    }
                    text_content.push_str(&unsupported_document_text(
                        document,
                        UNSUPPORTED_PROVIDER_REASON,
                    ));
                }
            }
        }

        // Add message if it has text content
        if !text_content.is_empty() {
            snowflake_messages.push(json!({
                "role": role,
                "content": text_content
            }));
        }
    }

    // Only add default message if we truly have no messages at all
    // This should be rare and only for edge cases
    if snowflake_messages.is_empty() {
        snowflake_messages.push(json!({
            "role": "user",
            "content": "Continue the conversation"
        }));
    }

    snowflake_messages
}

/// Convert internal Tool format to Snowflake's API tool specification
pub fn format_tools(tools: &[Tool]) -> Vec<Value> {
    let mut unique_tools = HashSet::new();
    let mut tool_specs = Vec::new();

    for tool in tools.iter() {
        if unique_tools.insert(tool.name.clone()) {
            let tool_spec = json!({
                "type": "generic",
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.input_schema
            });

            tool_specs.push(json!({"tool_spec": tool_spec}));
        }
    }

    tool_specs
}

/// Convert system message to Snowflake's API system specification
pub fn format_system(system: &str) -> Value {
    json!({
        "role": "system",
        "content": system,
    })
}

/// Convert Snowflake's streaming API response to internal Message format
pub fn parse_streaming_response(sse_data: &str) -> Result<Message> {
    let mut message = Message::assistant();
    let mut accumulated_text = String::new();
    let mut tool_use_id: Option<String> = None;
    let mut tool_name: Option<String> = None;
    let mut tool_input = String::new();

    // Parse each SSE event
    for line in sse_data.lines() {
        // SSE spec allows both "data: value" and "data:value" (space after colon is optional)
        if !line.starts_with("data:") {
            continue;
        }

        let json_str = line
            .strip_prefix("data: ")
            .or_else(|| line.strip_prefix("data:"))
            .unwrap(); // Remove "data:" prefix
        if json_str.trim().is_empty() || json_str.trim() == "[DONE]" {
            continue;
        }

        let event: Value = match serde_json::from_str(json_str) {
            Ok(v) => v,
            Err(_) => {
                continue;
            }
        };

        if let Some(choices) = event.get("choices").and_then(|c| c.as_array())
            && let Some(choice) = choices.first()
            && let Some(delta) = choice.get("delta")
        {
            match delta.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(content) = delta.get("content").and_then(|c| c.as_str()) {
                        accumulated_text.push_str(content);
                    }
                }
                Some("tool_use") => {
                    if let Some(id) = delta.get("tool_use_id").and_then(|i| i.as_str()) {
                        tool_use_id = Some(id.to_string());
                    }
                    if let Some(name) = delta.get("name").and_then(|n| n.as_str()) {
                        tool_name = Some(name.to_string());
                    }
                    if let Some(input) = delta.get("input").and_then(|i| i.as_str()) {
                        tool_input.push_str(input);
                    }
                }
                _ => {}
            }
        }
    }

    // Add accumulated text if any
    if !accumulated_text.is_empty() {
        message = message.with_text(accumulated_text);
    }

    // Add tool use if complete
    if let Some((id, name)) = tool_use_id.zip(tool_name) {
        if !tool_input.is_empty() {
            let input_value = serde_json::from_str::<Value>(&tool_input)
                .unwrap_or_else(|_| Value::String(tool_input.clone()));
            let tool_call = CallToolRequestParams::new(name).with_arguments(object(input_value));
            message = message.with_tool_request(&id, Ok(tool_call));
        } else {
            // Tool with no input - use empty object
            let tool_call = CallToolRequestParams::new(name).with_arguments(object!({}));
            message = message.with_tool_request(&id, Ok(tool_call));
        }
    }

    Ok(message)
}

/// Convert Snowflake's API response to internal Message format
pub fn response_to_message(response: &Value) -> Result<Message> {
    let mut message = Message::assistant();

    let content_list = response.get("content_list").and_then(|cl| cl.as_array());

    // Handle case where content_list is missing or empty
    let content_list = match content_list {
        Some(list) if !list.is_empty() => list,
        _ => {
            // If no content_list or empty, check if there's a direct content field
            if let Some(direct_content) = response.get("content").and_then(|c| c.as_str()) {
                if !direct_content.is_empty() {
                    message = message.with_text(direct_content.to_string());
                }
                return Ok(message);
            } else {
                // Return empty assistant message for empty responses
                return Ok(message);
            }
        }
    };

    // Process all content items in the list
    for content in content_list {
        match content.get("type").and_then(|t| t.as_str()) {
            Some("text") => {
                if let Some(text) = content.get("text").and_then(|t| t.as_str())
                    && !text.is_empty()
                {
                    message = message.with_text(text.to_string());
                }
            }
            Some("tool_use") => {
                let id = content
                    .get("tool_use_id")
                    .and_then(|i| i.as_str())
                    .ok_or_else(|| anyhow!("Missing tool_use id"))?;
                let name = content
                    .get("name")
                    .and_then(|n| n.as_str())
                    .ok_or_else(|| anyhow!("Missing tool_use name"))?
                    .to_string();

                let input = content
                    .get("input")
                    .ok_or_else(|| anyhow!("Missing tool input"))?
                    .clone();

                let tool_call = CallToolRequestParams::new(name).with_arguments(object(input));
                message = message.with_tool_request(id, Ok(tool_call));
            }
            Some("thinking") => {
                let thinking = content
                    .get("thinking")
                    .and_then(|t| t.as_str())
                    .ok_or_else(|| anyhow!("Missing thinking content"))?;
                let signature = content
                    .get("signature")
                    .and_then(|s| s.as_str())
                    .ok_or_else(|| anyhow!("Missing thinking signature"))?;
                message = message.with_thinking(thinking, signature);
            }
            Some("redacted_thinking") => {
                let data = content
                    .get("data")
                    .and_then(|d| d.as_str())
                    .ok_or_else(|| anyhow!("Missing redacted_thinking data"))?;
                message = message.with_redacted_thinking(data);
            }
            _ => {
                // Ignore unrecognized content types
            }
        }
    }

    Ok(message)
}

/// Extract usage information from Snowflake's API response
pub fn get_usage(data: &Value) -> Result<Usage> {
    // Extract usage data if available
    if let Some(usage) = data.get("usage") {
        let input_tokens = usage
            .get("input_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as i32);

        let output_tokens = usage
            .get("output_tokens")
            .and_then(|v| v.as_u64())
            .map(|v| v as i32);

        let total_tokens = match (input_tokens, output_tokens) {
            (Some(input), Some(output)) => Some(input + output),
            _ => None,
        };

        Ok(Usage::new(input_tokens, output_tokens, total_tokens))
    } else {
        tracing::debug!(
            "Failed to get usage data: {}",
            ProviderError::UsageError("No usage data found in response".to_string())
        );
        // If no usage data, return None for all values
        Ok(Usage::new(None, None, None))
    }
}

/// Create a complete request payload for Snowflake's API
pub fn create_request(
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
) -> Result<Value> {
    let mut snowflake_messages = format_messages(messages);
    let system_spec = format_system(system);

    // Add system message to the beginning of the messages
    snowflake_messages.insert(0, system_spec);

    // Check if we have any messages to send
    if snowflake_messages.is_empty() {
        return Err(anyhow!("No valid messages to send to Snowflake API"));
    }

    // Detect description generation requests and exclude tools to prevent interference
    // with normal tool execution flow
    let is_description_request =
        system.contains("Reply with only a description in four words or less");

    let tool_specs = if is_description_request {
        // For description generation, don't include any tools to avoid confusion
        format_tools(&[])
    } else {
        format_tools(tools)
    };

    let mut payload = json!({
        "model": model_config.model_name,
        "messages": snowflake_messages,
        "max_tokens": model_config.max_output_tokens(),
    });

    // Add tools if present and not a description request
    if !tool_specs.is_empty() {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("tools".to_string(), json!(tool_specs));
        } else {
            return Err(anyhow!(
                "Failed to create request payload: payload is not a JSON object"
            ));
        }
    }

    Ok(payload)
}

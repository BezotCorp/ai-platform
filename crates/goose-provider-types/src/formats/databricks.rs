use crate::cache_semantics::{CacheSemantics, apply_chat_payload_breakpoints};
use crate::conversations::{Message, MessageContentBlock, ToolResponse};
use crate::formats::anthropic::{
    ThinkingType, adaptive_output_effort, model_supports_temperature,
    requires_explicit_thinking_disable, thinking_block_is_stale, thinking_budget_tokens,
    thinking_type_for_provider,
};
use crate::json;
use crate::model::{ModelConfig, is_goose_internal_request_param};

use crate::document_format::{
    ASSISTANT_ROLE_REASON, DocumentFormat, UNSUPPORTED_MEDIA_TYPE_REASON, convert_document,
    document_media_type_is_supported, unsupported_document_text,
};
use crate::formats::openai::{
    extract_reasoning_effort, is_openai_responses_model, is_valid_function_name,
    openai_reasoning_effort_for_thinking, sanitize_function_name, validate_tool_schemas,
};
use crate::images::{ImageFormat, convert_image, detect_image_path, load_image_file};
use crate::mcp_utils::extract_text_from_resource;
use anyhow::{Error, anyhow};
use rmcp::model::{CallToolRequestParams, ContentBlock, ErrorCode, ErrorData, Role, Tool, object};
use serde::Serialize;
use serde_json::{Value, json};
use std::borrow::Cow;
use std::collections;

pub const DATABRICKS_PROVIDER_NAME: &str = "databricks";

#[derive(Serialize)]
struct DatabricksMessage {
    content: Value,
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

fn format_text_content(
    text: &str,
    image_format: &ImageFormat,
    supports_vision: bool,
) -> (Vec<Value>, bool) {
    let mut items = vec![json!({"type": "text", "text": text})];
    let has_image = if supports_vision {
        if let Some(path) = detect_image_path(text) {
            if let Ok(image) = load_image_file(path.as_ref()) {
                items.push(convert_image(&image, image_format));
            }
            true
        } else {
            false
        }
    } else {
        false
    };
    (items, has_image)
}

fn format_tool_response(
    response: &ToolResponse,
    image_format: &ImageFormat,
    supports_vision: bool,
) -> Vec<DatabricksMessage> {
    let mut result = Vec::new();

    match &response.tool_result {
        Ok(call_result) => {
            let abridged: Vec<_> = call_result.content.to_vec();

            let mut tool_content = Vec::new();
            let mut image_messages = Vec::new();

            for content in abridged {
                match content {
                    ContentBlock::Image(image) => {
                        if supports_vision {
                            tool_content.push(ContentBlock::text(
                            "This tool result included an image that is uploaded in the next message.",
                        ));
                            image_messages.push(DatabricksMessage {
                                role: "user".to_string(),
                                content: [convert_image(&image, image_format)].into(),
                                tool_calls: None,
                                tool_call_id: None,
                            });
                        } else {
                            tool_content.push(ContentBlock::text(
                                "This tool result included an image that was omitted as the model does not support vision.",
                            ));
                        }
                    }
                    ContentBlock::Resource(resource) => {
                        let text = extract_text_from_resource(&resource.resource);
                        tool_content.push(ContentBlock::text(text));
                    }
                    _ => tool_content.push(content),
                }
            }

            let tool_response_content: Value = json!(
                tool_content
                    .iter()
                    .filter_map(|c| c.as_text().map(|t| t.text.clone()))
                    .collect::<Vec<String>>()
                    .join(" ")
            );

            result.push(DatabricksMessage {
                content: tool_response_content,
                role: "tool".to_string(),
                tool_call_id: Some(response.id.clone()),
                tool_calls: None,
            });
            result.extend(image_messages);
        }
        Err(e) => {
            result.push(DatabricksMessage {
                role: "tool".to_string(),
                content: format!("The tool call returned the following error:\n{}", e).into(),
                tool_call_id: Some(response.id.clone()),
                tool_calls: None,
            });
        }
    }

    result
}

fn format_messages(
    messages: &[Message],
    image_format: &ImageFormat,
    current_model: Option<&str>,
    supports_vision: bool,
) -> Vec<DatabricksMessage> {
    let mut result = Vec::new();
    for message in messages {
        let thinking_is_stale = thinking_block_is_stale(message, current_model);
        let mut converted = DatabricksMessage {
            content: Value::Null,
            role: match message.role {
                Role::User => "user".to_string(),
                Role::Assistant => "assistant".to_string(),
            },
            tool_calls: None,
            tool_call_id: None,
        };

        let mut content_array = Vec::new();
        let mut has_tool_calls = false;
        let mut has_multiple_content = false;
        // Deferred so all tool-role messages stay consecutive (required by Claude via Databricks).
        let mut pending_image_messages: Vec<DatabricksMessage> = Vec::new();

        for content in &message.content {
            match content {
                MessageContentBlock::Text(text) => {
                    if !text.text.is_empty() {
                        let (items, multi) =
                            format_text_content(&text.text, image_format, supports_vision);
                        content_array.extend(items);
                        has_multiple_content |= multi;
                    }
                }
                MessageContentBlock::Thinking(content) => {
                    if !thinking_is_stale {
                        has_multiple_content = true;
                        content_array.push(json!({
                            "type": "reasoning",
                            "summary": [{
                                "type": "summary_text",
                                "text": content.thinking,
                                "signature": content.signature
                            }]
                        }));
                    }
                }
                MessageContentBlock::RedactedThinking(content) => {
                    if !thinking_is_stale {
                        has_multiple_content = true;
                        content_array.push(json!({
                            "type": "reasoning",
                            "summary": [{"type": "summary_encrypted_text", "data": content.data}]
                        }));
                    }
                }
                MessageContentBlock::ToolRequest(request) => {
                    has_tool_calls = true;
                    match &request.tool_call {
                        Ok(tool_call) => {
                            let sanitized_name = sanitize_function_name(&tool_call.name);
                            let arguments_str = tool_call
                                .arguments
                                .as_ref()
                                .map(|args| {
                                    serde_json::to_string(args).unwrap_or_else(|_| "{}".to_string())
                                })
                                .unwrap_or_else(|| "{}".to_string());

                            let tool_calls = converted.tool_calls.get_or_insert_default();
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

                            tool_calls.push(tool_call_json);
                        }
                        Err(_e) => {
                            // Mirror the OpenAI formatter: emitting the error as assistant
                            // text leaves no `tool_calls` entry, so the paired tool response
                            // orphans (a `role:"tool"` with no preceding assistant
                            // `tool_calls`) and strict APIs reject it. Emit a placeholder
                            // call with the same id; the error rides on the tool response.
                            let tool_calls = converted.tool_calls.get_or_insert_default();
                            tool_calls.push(json!({
                                "id": request.id,
                                "type": "function",
                                "function": {
                                    "name": "unparseable_tool_call",
                                    "arguments": "{}",
                                }
                            }));
                        }
                    }
                }
                MessageContentBlock::ToolResponse(response) => {
                    for msg in format_tool_response(response, image_format, supports_vision) {
                        if msg.role == "user" {
                            pending_image_messages.push(msg);
                        } else {
                            result.push(msg);
                        }
                    }
                }
                MessageContentBlock::Image(image) => {
                    if supports_vision {
                        content_array.push(convert_image(image, image_format));
                    } else {
                        content_array.push(json!({
                            "type": "text",
                            "text": "[image omitted: model does not support vision]"
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
                        content_array.push(convert_document(document, &DocumentFormat::OpenAi));
                    } else {
                        content_array.push(json!({
                            "type": "text",
                            "text": unsupported_document_text(document, UNSUPPORTED_MEDIA_TYPE_REASON)
                        }));
                    }
                }
                MessageContentBlock::SystemNotification(_)
                | MessageContentBlock::Error(_)
                | MessageContentBlock::ToolConfirmationRequest(_)
                | MessageContentBlock::ActionRequired(_) => {}
            }
        }

        result.extend(pending_image_messages);

        if !content_array.is_empty() {
            converted.content = if content_array.len() == 1
                && !has_multiple_content
                && content_array[0]["type"] == "text"
            {
                json!(content_array[0]["text"])
            } else {
                json!(content_array)
            };
        }

        if !content_array.is_empty() || has_tool_calls {
            result.push(converted);
        }
    }

    result
}

fn apply_claude_thinking_config(
    payload: &mut Value,
    provider_name: &str,
    model_config: &ModelConfig,
) {
    let obj = payload.as_object_mut().unwrap();

    match thinking_type_for_provider(provider_name, model_config) {
        ThinkingType::Adaptive => {
            obj.insert("thinking".to_string(), json!({ "type": "adaptive" }));
            obj.insert(
                "output_config".to_string(),
                json!({ "effort": adaptive_output_effort(model_config).to_string() }),
            );
            obj.insert(
                "max_completion_tokens".to_string(),
                json!(model_config.max_output_tokens()),
            );
        }
        ThinkingType::Enabled => {
            let budget_tokens = thinking_budget_tokens(model_config);
            let max_tokens = model_config.max_output_tokens() + budget_tokens;
            obj.insert("max_tokens".to_string(), json!(max_tokens));
            obj.insert(
                "thinking".to_string(),
                json!({
                    "type": "enabled",
                    "budget_tokens": budget_tokens
                }),
            );
            obj.insert("temperature".to_string(), json!(2));
        }
        ThinkingType::Disabled => {
            if requires_explicit_thinking_disable(provider_name, &model_config.model_name) {
                obj.insert("thinking".to_string(), json!({ "type": "disabled" }));
            }
            if model_supports_temperature(provider_name, model_config)
                && let Some(temp) = model_config.temperature
            {
                obj.insert("temperature".to_string(), json!(temp));
            }
            obj.insert(
                "max_completion_tokens".to_string(),
                json!(model_config.max_output_tokens()),
            );
        }
    }
}

pub fn format_tools_databricks(tools: &[Tool], _model_name: &str) -> anyhow::Result<Vec<Value>> {
    let mut tool_names = collections::HashSet::new();
    let mut result = Vec::new();

    for tool in tools {
        if !tool_names.insert(&tool.name) {
            return Err(anyhow!("Duplicate tool name: {}", tool.name));
        }

        // Databricks serving endpoints (including Gemini-backed ones) use the
        // OpenAI-compatible chat format, so tools always use "parameters" — not
        // the Google-native "parametersJsonSchema" field. "parameters" is
        // required even when a tool takes no arguments, so it is always sent.
        result.push(json!({
            "type": "function",
            "function": {
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.input_schema,
            },
        }));
    }

    Ok(result)
}

/// Convert Databricks' API response to internal Message format
#[allow(clippy::too_many_lines)]
pub fn response_to_message(response: &Value) -> anyhow::Result<Message> {
    let original = &response["choices"][0]["message"];
    let mut content = Vec::new();

    // Handle array-based content
    if let Some(content_array) = original.get("content").and_then(|c| c.as_array()) {
        for content_item in content_array {
            match content_item.get("type").and_then(|t| t.as_str()) {
                Some("text") => {
                    if let Some(text) = content_item.get("text").and_then(|t| t.as_str()) {
                        content.push(MessageContentBlock::text(text));
                    }
                }
                Some("reasoning") => {
                    if let Some(summary_array) =
                        content_item.get("summary").and_then(|s| s.as_array())
                    {
                        for summary in summary_array {
                            match summary.get("type").and_then(|t| t.as_str()) {
                                Some("summary_text") => {
                                    let text = summary
                                        .get("text")
                                        .and_then(|t| t.as_str())
                                        .unwrap_or_default();
                                    let signature = summary
                                        .get("signature")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or_default();
                                    content.push(MessageContentBlock::thinking(text, signature));
                                }
                                Some("summary_encrypted_text") => {
                                    if let Some(data) = summary.get("data").and_then(|d| d.as_str())
                                    {
                                        content.push(MessageContentBlock::redacted_thinking(data));
                                    }
                                }
                                _ => continue,
                            }
                        }
                    }
                }
                _ => continue,
            }
        }
    } else if let Some(text) = original.get("content").and_then(|t| t.as_str()) {
        // Handle legacy single string content
        content.push(MessageContentBlock::text(text));
    }

    // Handle tool calls
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

            if !is_valid_function_name(&function_name) {
                let error = ErrorData {
                    code: ErrorCode::INVALID_REQUEST,
                    message: Cow::from(format!(
                        "The provided function name '{}' had invalid characters, it must match this regex [a-zA-Z0-9_-]+",
                        function_name
                    )),
                    data: None,
                };
                content.push(MessageContentBlock::tool_request(id, Err(error)));
            } else {
                match json::parse_tool_arguments(&arguments_str) {
                    Some(params) if params.is_object() => {
                        content.push(MessageContentBlock::tool_request(
                            id,
                            Ok(CallToolRequestParams::new(function_name)
                                .with_arguments(object(params))),
                        ));
                    }
                    // Valid JSON but NOT an object (a bare array/string/number).
                    // Surface a tool error so the model retries instead of
                    // crashing the run (rmcp's `object()` debug-asserts).
                    Some(_) => {
                        let error = ErrorData {
                            code: ErrorCode::INVALID_PARAMS,
                            message: Cow::from(format!(
                                "Tool arguments for {} (id {}) must be a JSON object. Raw arguments: '{}'",
                                function_name, id, arguments_str
                            )),
                            data: None,
                        };
                        content.push(MessageContentBlock::tool_request(id, Err(error)));
                    }
                    None => {
                        let message_text = json::truncation_error_message(&arguments_str)
                            .unwrap_or_else(|| {
                                format!("Could not interpret tool use parameters for id {id}")
                            });
                        let error = ErrorData {
                            code: ErrorCode::INVALID_PARAMS,
                            message: Cow::from(message_text),
                            data: None,
                        };
                        content.push(MessageContentBlock::tool_request(id, Err(error)));
                    }
                }
            }
        }
    }

    Ok(Message::new(
        Role::Assistant,
        chrono::Utc::now().timestamp(),
        content,
    ))
}

/// Check if the model name indicates a Claude/Anthropic model that supports cache control.
fn is_claude_model(model_name: &str) -> bool {
    model_name.contains("claude")
}

pub fn create_request(
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    image_format: &ImageFormat,
) -> anyhow::Result<Value, Error> {
    create_request_for_provider(
        DATABRICKS_PROVIDER_NAME,
        model_config,
        system,
        messages,
        tools,
        image_format,
    )
}

pub fn create_request_for_provider(
    provider_name: &str,
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    image_format: &ImageFormat,
) -> anyhow::Result<Value, Error> {
    if model_config.model_name.starts_with("o1-mini") {
        return Err(anyhow!(
            "o1-mini model is not currently supported since goose uses tool calling and o1-mini does not support it. Please use o1 or o3 models instead."
        ));
    }

    let (model_name, legacy_reasoning_effort) = extract_reasoning_effort(&model_config.model_name);
    let is_openai_reasoning_model = is_openai_responses_model(&model_name);
    let reasoning_effort = if is_openai_reasoning_model {
        model_config
            .thinking_effort()
            .map_or(legacy_reasoning_effort, |effort| {
                openai_reasoning_effort_for_thinking(&model_name, effort)
            })
    } else {
        None
    };

    let system_message = DatabricksMessage {
        role: "system".to_string(),
        content: system.into(),
        tool_calls: None,
        tool_call_id: None,
    };

    let model_supports_vision = model_config.supports_vision.unwrap_or_default();
    let messages_spec = format_messages(
        messages,
        image_format,
        Some(&model_config.model_name),
        model_supports_vision,
    );
    let mut tools_spec = if !tools.is_empty() {
        format_tools_databricks(tools, &model_config.model_name)?
    } else {
        vec![]
    };

    // Validate tool schemas
    validate_tool_schemas(&mut tools_spec);

    let mut messages_array = vec![system_message];
    messages_array.extend(messages_spec);

    let mut payload = json!({
        "model": model_name,
        "messages": messages_array
    });

    if let Some(effort) = reasoning_effort {
        payload
            .as_object_mut()
            .unwrap()
            .insert("reasoning_effort".to_string(), json!(effort));
    }

    if !tools_spec.is_empty() {
        payload
            .as_object_mut()
            .unwrap()
            .insert("tools".to_string(), json!(tools_spec));
    }

    if is_claude_model(&model_config.model_name) {
        apply_claude_thinking_config(&mut payload, provider_name, model_config);
    } else {
        // open ai reasoning models currently don't support temperature
        if !is_openai_reasoning_model
            && model_supports_temperature(provider_name, model_config)
            && let Some(temp) = model_config.temperature
        {
            payload
                .as_object_mut()
                .unwrap()
                .insert("temperature".to_string(), json!(temp));
        }

        payload.as_object_mut().unwrap().insert(
            "max_completion_tokens".to_string(),
            json!(model_config.max_output_tokens()),
        );
    }

    if CacheSemantics::for_model("databricks", &model_config.model_name).uses_explicit_breakpoints()
        && !model_config.prompt_cache_disabled()
    {
        apply_chat_payload_breakpoints(&mut payload);
    }

    // Add request_params to the payload (e.g., anthropic_beta for extended context)
    if let Some(params) = &model_config.request_params
        && let Some(obj) = payload.as_object_mut()
    {
        for (key, value) in params {
            if is_goose_internal_request_param(key) {
                continue;
            }
            obj.insert(key.clone(), value.clone());
        }
    }

    Ok(payload)
}

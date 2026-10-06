use crate::conversations::{Message, MessageContentBlock, ProviderMetadata, ProviderUsage, Usage};
use crate::document_format::{
    UNSUPPORTED_MEDIA_TYPE_REASON, document_media_type_is_supported, unsupported_document_text,
};
use crate::errors::ProviderError;
use crate::formats::openai::{is_valid_function_name, sanitize_function_name};
use crate::{
    maybe_send::MaybeSend, mcp_utils::extract_text_from_resource, model::ModelConfig,
    thinking::ThinkingEffort,
};
use anyhow::Result;
use rmcp::model::{
    CallToolRequestParams, ContentBlock, ErrorCode, ErrorData, ResourceContents, Role, Tool, object,
};
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::borrow::Cow;
use std::collections::HashMap;
use uuid::Uuid;
pub const THOUGHT_SIGNATURE_KEY: &str = "thoughtSignature";
const SYNTHETIC_THOUGHT_SIGNATURE: &str = "skip_thought_signature_validator";
const DEFAULT_THINKING_BUDGET: i32 = 8192;

pub fn metadata_with_signature(signature: &str) -> ProviderMetadata {
    let mut map = ProviderMetadata::new();
    map.insert(THOUGHT_SIGNATURE_KEY.to_string(), json!(signature));
    map
}

pub fn get_thought_signature(metadata: &Option<ProviderMetadata>) -> Option<&str> {
    metadata
        .as_ref()
        .and_then(|m| m.get(THOUGHT_SIGNATURE_KEY))
        .and_then(|v| v.as_str())
}

fn is_user_loop_boundary(message: &Message) -> bool {
    message.role == Role::User
        && message
            .content
            .iter()
            .any(|content| !matches!(content, MessageContentBlock::ToolResponse(_)))
}

fn insert_thought_signature(part: &mut Map<String, Value>, signature: &str) {
    part.insert(THOUGHT_SIGNATURE_KEY.to_string(), json!(signature));
}

fn maybe_insert_signature_from_metadata(
    part: &mut Map<String, Value>,
    metadata: &Option<ProviderMetadata>,
) {
    if let Some(signature) = get_thought_signature(metadata) {
        insert_thought_signature(part, signature);
    }
}

fn build_function_response_part(
    id: &str,
    name: &str,
    text: String,
    media: Vec<Value>,
) -> Map<String, Value> {
    let mut part = Map::new();
    let mut function_response = Map::new();
    function_response.insert("id".to_string(), json!(id));
    function_response.insert("name".to_string(), json!(name));
    function_response.insert("response".to_string(), json!({"content": {"text": text}}));
    if !media.is_empty() {
        function_response.insert("parts".to_string(), json!(media));
    }
    part.insert("functionResponse".to_string(), json!(function_response));
    part
}

/// Convert internal Message format to Google's API message specification
pub fn format_messages_google(
    messages: &[Message],
    nested_function_response_media: bool,
) -> Vec<Value> {
    let filtered: Vec<_> = messages
        .iter()
        .filter(|m| m.is_agent_visible())
        .filter(|message| {
            message.content.iter().any(|content| {
                !matches!(
                    content,
                    MessageContentBlock::ToolConfirmationRequest(_)
                        | MessageContentBlock::ActionRequired(_)
                )
            })
        })
        .collect();

    // Record names as we walk the conversation so a reused tool-call id
    // resolves to the nearest preceding request, not a later overwrite.
    let mut tool_names: HashMap<&str, String> = HashMap::new();

    let active_loop_start_idx = filtered
        .iter()
        .enumerate()
        .rev()
        .find(|(_, m)| is_user_loop_boundary(m))
        .map(|(i, _)| i);

    filtered
        .iter()
        .enumerate()
        .filter_map(|(idx, message)| {
            let role = if message.role == Role::User {
                "user"
            } else {
                "model"
            };
            let include_signature = active_loop_start_idx.is_none_or(|start_idx| idx >= start_idx);
            // Only the first model tool call in a turn is guaranteed to carry
            // a signature for loop continuity.
            let mut needs_synthetic_for_first_model_tool_call =
                include_signature && message.role != Role::User;
            let mut parts = Vec::new();
            for message_content in message.content.iter() {
                match message_content {
                    MessageContentBlock::Text(text) => {
                        if !text.text.is_empty() {
                            parts.push(json!({"text": text.text}));
                        }
                    }
                    MessageContentBlock::ToolRequest(request) => match &request.tool_call {
                        Ok(tool_call) => {
                            let name = sanitize_function_name(&tool_call.name);
                            tool_names.insert(request.id.as_str(), name.clone());
                            let mut function_call_part = Map::new();
                            function_call_part.insert("id".to_string(), json!(request.id));
                            function_call_part.insert("name".to_string(), json!(name));

                            if let Some(args) = &tool_call.arguments
                                && !args.is_empty() {
                                    function_call_part
                                        .insert("args".to_string(), args.clone().into());
                                }

                            let mut part = Map::new();
                            part.insert("functionCall".to_string(), json!(function_call_part));

                            if include_signature {
                                if let Some(signature) = get_thought_signature(&request.metadata) {
                                    insert_thought_signature(&mut part, signature);
                                } else if needs_synthetic_for_first_model_tool_call {
                                    insert_thought_signature(
                                        &mut part,
                                        SYNTHETIC_THOUGHT_SIGNATURE,
                                    );
                                }
                            }
                            needs_synthetic_for_first_model_tool_call = false;

                            parts.push(json!(part));
                        }
                        Err(e) => {
                            parts.push(json!({"text":format!("Error: {}", e)}));
                        }
                    },
                    MessageContentBlock::ToolResponse(response) => match &response.tool_result {
                        Ok(result) => {
                            let mut tool_content = Vec::new();
                            let mut media = Vec::new();
                            for content in result.content.iter().cloned() {
                                let inline = match &content {
                                    ContentBlock::Image(image) => {
                                        Some((image.mime_type.clone(), image.data.clone()))
                                    }
                                    ContentBlock::Resource(embedded) => match &embedded.resource {
                                        ResourceContents::BlobResourceContents {
                                            blob,
                                            mime_type,
                                            ..
                                        } => mime_type
                                            .clone()
                                            .filter(|m| !m.is_empty())
                                            .map(|mime| (mime, blob.clone())),
                                        _ => None,
                                    },
                                    _ => None,
                                };
                                match inline {
                                    Some((mime, data)) if nested_function_response_media => {
                                        media.push(json!({
                                            "inlineData": {"mimeType": mime, "data": data}
                                        }));
                                    }
                                    Some((mime, data)) => {
                                        parts.push(json!({
                                            "inline_data": {"mime_type": mime, "data": data}
                                        }));
                                    }
                                    None => tool_content.push(content),
                                }
                            }
                            let mut text = tool_content
                                .iter()
                                .filter_map(|c| match c {
                                    ContentBlock::Text(t) => Some(t.text.clone()),
                                    ContentBlock::Resource(raw_embedded_resource) => Some(
                                        extract_text_from_resource(&raw_embedded_resource.resource),
                                    ),
                                    _ => None,
                                })
                                .collect::<Vec<_>>()
                                .join("\n");

                            if text.is_empty() {
                                text = "Tool call is done.".to_string();
                            }
                            let name = tool_names
                                .get(response.id.as_str())
                                .map(String::as_str)
                                .unwrap_or(response.id.as_str());
                            let mut part =
                                build_function_response_part(&response.id, name, text, media);
                            if include_signature {
                                maybe_insert_signature_from_metadata(&mut part, &response.metadata);
                            }
                            parts.push(json!(part));
                        }
                        Err(e) => {
                            let name = tool_names
                                .get(response.id.as_str())
                                .map(String::as_str)
                                .unwrap_or(response.id.as_str());
                            let mut part = build_function_response_part(
                                &response.id,
                                name,
                                format!("Error: {}", e),
                                Vec::new(),
                            );
                            if include_signature {
                                maybe_insert_signature_from_metadata(&mut part, &response.metadata);
                            }
                            parts.push(json!(part));
                        }
                    },
                    MessageContentBlock::Thinking(_) => {}
                    MessageContentBlock::Image(image) => {
                        parts.push(json!({
                            "inline_data": {
                                "mime_type": image.mime_type,
                                "data": image.data,
                            }
                        }));
                    }
                    MessageContentBlock::Document(document) => {
                        if document_media_type_is_supported(&document.mime_type) {
                            parts.push(json!({
                                "inline_data": {
                                    "mime_type": document.mime_type,
                                    "data": document.data,
                                }
                            }));
                        } else {
                            parts.push(json!({
                                "text": unsupported_document_text(document, UNSUPPORTED_MEDIA_TYPE_REASON)
                            }));
                        }
                    }

                    _ => {}
                }
            }
            if parts.is_empty() {
                None
            } else {
                Some(json!({"role": role, "parts": parts}))
            }
        })
        .collect()
}

pub fn format_tools_google(tools: &[Tool]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            let mut parameters = Map::new();
            parameters.insert("name".to_string(), json!(tool.name));
            parameters.insert("description".to_string(), json!(tool.description));

            // Use parametersJsonSchema which supports full JSON Schema including $ref/$defs
            if tool
                .input_schema
                .get("properties")
                .and_then(|v| v.as_object())
                .is_some_and(|p| !p.is_empty())
            {
                parameters.insert("parametersJsonSchema".to_string(), json!(tool.input_schema));
            }
            json!(parameters)
        })
        .collect()
}

fn process_response_part_impl(
    part: &Value,
    last_signature: &mut Option<String>,
) -> Option<MessageContentBlock> {
    let signature = part.get(THOUGHT_SIGNATURE_KEY).and_then(|v| v.as_str());
    let is_thought = part
        .get("thought")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);

    if let Some(sig) = signature {
        *last_signature = Some(sig.to_string());
    }

    let text_value = part.get("text");
    if let Some(text) = text_value.and_then(|v| v.as_str()) {
        if text.is_empty() {
            return None;
        }
        if is_thought {
            match signature {
                Some(sig) => Some(MessageContentBlock::thinking(
                    text.to_string(),
                    sig.to_string(),
                )),
                None => Some(MessageContentBlock::thinking(text.to_string(), "")),
            }
        } else {
            Some(MessageContentBlock::text(text.to_string()))
        }
    } else if text_value.is_some() {
        tracing::warn!(
            "Google response part has 'text' field but it's not a string: {:?}",
            text_value
        );
        None
    } else if let Some(function_call) = part.get("functionCall") {
        let id = function_call
            .get("id")
            .and_then(Value::as_str)
            .map(str::to_string)
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let name = function_call["name"].as_str().unwrap_or_default();

        if !is_valid_function_name(name) {
            let error = ErrorData {
                code: ErrorCode::INVALID_REQUEST,
                message: Cow::from(format!(
                    "The provided function name '{}' had invalid characters, it must match this regex [a-zA-Z0-9_-]+",
                    name
                )),
                data: None,
            };
            Some(MessageContentBlock::tool_request(id, Err(error)))
        } else {
            let arguments = function_call
                .get("args")
                .map(|params| object(params.clone()));
            let effective_signature = signature.or(last_signature.as_deref());
            let metadata = effective_signature.map(metadata_with_signature);

            Some(MessageContentBlock::tool_request_with_metadata(
                id,
                Ok({
                    let mut params = CallToolRequestParams::new(name.to_string());
                    if let Some(args) = arguments {
                        params = params.with_arguments(args);
                    }
                    params
                }),
                metadata.as_ref(),
            ))
        }
    } else {
        None
    }
}

pub fn response_to_message_google(response: Value) -> Result<Message> {
    let role = Role::Assistant;
    let created = chrono::Utc::now().timestamp();

    let parts = response
        .get("candidates")
        .and_then(|v| v.as_array())
        .and_then(|c| c.first())
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.as_array());

    let Some(parts) = parts else {
        return Ok(Message::new(role, created, Vec::new()));
    };

    let mut content = Vec::new();
    let mut last_signature: Option<String> = None;

    for part in parts {
        if let Some(msg_content) = process_response_part_impl(part, &mut last_signature) {
            content.push(msg_content);
        }
    }
    Ok(Message::new(role, created, content))
}

/// Extract usage information from Google's API response
pub fn get_usage_google(data: &Value) -> Result<Usage> {
    if let Some(usage_meta_data) = data.get("usageMetadata") {
        let input_tokens = usage_meta_data
            .get("promptTokenCount")
            .and_then(|v| v.as_u64())
            .map(|v| v as i32);
        // `candidatesTokenCount` is the visible output; thinking models
        // (Gemini 2.5/3) report reasoning tokens separately in
        // `thoughtsTokenCount`, and per the API spec `totalTokenCount` =
        // prompt + thoughts + candidates. Fold thoughts into `output_tokens` so
        // the record reconciles (input + output == total) and cost, which
        // Google bills at the output rate, is correct -- matching the OpenAI
        // (completion_tokens includes reasoning) and Anthropic (output_tokens
        // includes thinking) adapters.
        let candidates_tokens = usage_meta_data
            .get("candidatesTokenCount")
            .and_then(|v| v.as_u64());
        let thoughts_tokens = usage_meta_data
            .get("thoughtsTokenCount")
            .and_then(|v| v.as_u64());
        let output_tokens = match (candidates_tokens, thoughts_tokens) {
            (None, None) => None,
            (candidates, thoughts) => {
                Some((candidates.unwrap_or(0) + thoughts.unwrap_or(0)) as i32)
            }
        };
        let total_tokens = usage_meta_data
            .get("totalTokenCount")
            .and_then(|v| v.as_u64())
            .map(|v| v as i32);
        // promptTokenCount already includes cachedContentTokenCount
        let cached_tokens = usage_meta_data
            .get("cachedContentTokenCount")
            .and_then(|v| v.as_u64())
            .map(|v| v as i32);
        Ok(Usage::new(input_tokens, output_tokens, total_tokens)
            .with_cache_tokens(cached_tokens, None))
    } else {
        tracing::debug!(
            "Failed to get usage data: {}",
            ProviderError::UsageError("No usage data found in response".to_string())
        );
        // If no usage data, return None for all values
        Ok(Usage::new(None, None, None))
    }
}

pub fn response_to_streaming_message_google<S>(
    mut stream: S,
) -> impl futures::Stream<Item = anyhow::Result<(Option<Message>, Option<ProviderUsage>)>> + 'static
where
    S: futures::Stream<Item = anyhow::Result<String>> + Unpin + MaybeSend + 'static,
{
    use async_stream::try_stream;
    use futures::StreamExt;
    try_stream! {
        let mut final_usage: Option<ProviderUsage> = None;
        let mut last_signature: Option<String> = None;
        let stream_id = Uuid::new_v4().to_string();
        let mut incomplete_data: Option<String> = None;
        let mut last_finish_reason: Option<String> = None;
        let mut last_response_id: Option<String> = None;

        while let Some(line_result) = stream.next().await {
            let line = line_result?;

            if line.trim().is_empty() {
                continue;
            }

            let data_part = if line.starts_with("data: ") {
                line.strip_prefix("data: ").unwrap()
            } else if line.starts_with("event:") || line.starts_with("id:") || line.starts_with("retry:") {
                continue;
            } else if incomplete_data.is_some() {
                &line
            } else {
                continue;
            };

            if data_part.trim() == "[DONE]" {
                break;
            }

            let chunk: Value = if let Some(ref mut incomplete) = incomplete_data {
                incomplete.push_str(data_part);
                match serde_json::from_str(incomplete) {
                    Ok(v) => {
                        incomplete_data = None;
                        v
                    }
                    Err(e) => {
                        if e.is_eof() {
                            continue;
                        }
                        tracing::warn!("Failed to parse streaming chunk: {}", e);
                        incomplete_data = None;
                        continue;
                    }
                }
            } else {
                match serde_json::from_str(data_part) {
                    Ok(v) => v,
                    Err(e) => {
                        if e.is_eof() {
                            incomplete_data = Some(data_part.to_string());
                            continue;
                        }
                        tracing::warn!("Failed to parse streaming chunk: {}", e);
                        continue;
                    }
                }
            };

            if let Some(error) = chunk.get("error") {
                let message = error
                    .get("message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("Unknown error");
                let status = error
                    .get("status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("UNKNOWN");
                Err::<(), ProviderError>(ProviderError::RequestFailed(format!(
                    "Google API error ({status}): {message}"
                )))?;
            }

            if let Ok(usage) = get_usage_google(&chunk)
                && (usage.input_tokens.is_some() || usage.output_tokens.is_some()) {
                    let model = chunk.get("modelVersion")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();
                    final_usage = Some(ProviderUsage::new(model, usage));
                }

            if let Some(response_id) = chunk.get("responseId").and_then(|v| v.as_str()) {
                last_response_id = Some(response_id.to_string());
            }

            let candidate = chunk
                .get("candidates")
                .and_then(|v| v.as_array())
                .and_then(|c| c.first());
            if let Some(reason) = candidate
                .and_then(|c| c.get("finishReason"))
                .and_then(|v| v.as_str())
            {
                last_finish_reason = Some(reason.to_string());
            }

            let parts = candidate
                .and_then(|c| c.get("content"))
                .and_then(|c| c.get("parts"))
                .and_then(|p| p.as_array());

            if let Some(parts) = parts {
                for part in parts {
                    if let Some(content) = process_response_part_impl(part, &mut last_signature) {
                        let message = Message::new(
                            Role::Assistant,
                            chrono::Utc::now().timestamp(),
                            vec![content],
                        ).with_id(stream_id.clone());
                        yield (Some(message), None);
                    }
                }
            }
        }

        if let Some(mut usage) = final_usage {
            if let Some(reason) = last_finish_reason {
                usage.finish_reasons = Some(vec![reason]);
            }
            usage.response_id = last_response_id;
            yield (None, Some(usage));
        }
    }
}

#[derive(Serialize)]
struct TextPart<'a> {
    text: &'a str,
}

#[derive(Serialize)]
struct SystemInstruction<'a> {
    parts: [TextPart<'a>; 1],
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ToolsWrapper {
    function_declarations: Vec<Value>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GenerationConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    temperature: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_config: Option<ThinkingConfig>,
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum ThinkingLevel {
    Minimal,
    Low,
    Medium,
    High,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ThinkingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_level: Option<ThinkingLevel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    thinking_budget: Option<i32>,
    include_thoughts: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct GoogleRequest<'a> {
    system_instruction: SystemInstruction<'a>,
    contents: Vec<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tools: Option<ToolsWrapper>,
    #[serde(skip_serializing_if = "Option::is_none")]
    generation_config: Option<GenerationConfig>,
}

fn get_thinking_config(
    model_config: &ModelConfig,
    thinking_budget: Option<i32>,
) -> Option<ThinkingConfig> {
    if model_config.reasoning == Some(false)
        || model_config.thinking_effort() == Some(ThinkingEffort::Off)
    {
        let model_name = model_config.model_name.to_lowercase();
        if model_name.starts_with("gemini-3.5") || model_name.starts_with("gemini-3.6") {
            return Some(ThinkingConfig {
                thinking_level: Some(ThinkingLevel::Minimal),
                thinking_budget: None,
                include_thoughts: false,
            });
        }
        // Gemini 2.5 Flash defaults to dynamic thinking; only an explicit budget
        // of 0 turns it off. Other families can't be disabled, so leave them unset.
        if model_config
            .model_name
            .to_lowercase()
            .starts_with("gemini-2.5-flash")
        {
            return Some(ThinkingConfig {
                thinking_level: None,
                thinking_budget: Some(0),
                include_thoughts: false,
            });
        }
        return None;
    }
    let model_name = model_config.model_name.to_lowercase();
    let is_gemini_3 = model_name.starts_with("gemini-3");
    let is_gemini_25 = model_name.starts_with("gemini-2.5");
    if !is_gemini_3 && !is_gemini_25 {
        return None;
    }

    if is_gemini_3 {
        let effort = model_config
            .thinking_effort()
            .unwrap_or(ThinkingEffort::Off);
        if effort == ThinkingEffort::Off {
            return None;
        }
        let thinking_level = match effort {
            ThinkingEffort::Off | ThinkingEffort::Low => ThinkingLevel::Low,
            ThinkingEffort::Medium if model_name.starts_with("gemini-3-pro") => ThinkingLevel::Low,
            ThinkingEffort::Medium => ThinkingLevel::Medium,
            ThinkingEffort::High | ThinkingEffort::Max => ThinkingLevel::High,
        };

        Some(ThinkingConfig {
            thinking_level: Some(thinking_level),
            thinking_budget: None,
            include_thoughts: true,
        })
    } else {
        let thinking_budget = match model_config
            .request_param::<i32>("thinking_budget")
            .or(thinking_budget)
        {
            Some(budget) if budget >= 0 => budget,
            Some(budget) => {
                tracing::warn!(
                    "Invalid thinking budget '{}' for model '{}'. Must be >= 0. Using '{}'.",
                    budget,
                    model_config.model_name,
                    DEFAULT_THINKING_BUDGET,
                );
                DEFAULT_THINKING_BUDGET
            }
            None => DEFAULT_THINKING_BUDGET,
        };
        Some(ThinkingConfig {
            thinking_level: None,
            thinking_budget: Some(thinking_budget),
            include_thoughts: true,
        })
    }
}

pub fn create_request_google(
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
) -> Result<Value> {
    create_request_impl(model_config, system, messages, tools, None)
}

pub fn create_request_with_thinking_budget(
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    thinking_budget: Option<i32>,
) -> Result<Value> {
    create_request_impl(model_config, system, messages, tools, thinking_budget)
}

fn create_request_impl(
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
    thinking_budget: Option<i32>,
) -> Result<Value> {
    let tools_wrapper = if tools.is_empty() {
        None
    } else {
        Some(ToolsWrapper {
            function_declarations: format_tools_google(tools),
        })
    };

    let thinking_config = get_thinking_config(model_config, thinking_budget);
    let temperature = (!model_config
        .model_name
        .to_lowercase()
        .starts_with("gemini-3"))
    .then(|| model_config.temperature.map(|t| t as f64))
    .flatten();

    let generation_config = Some(GenerationConfig {
        temperature,
        max_output_tokens: Some(model_config.max_output_tokens()),
        thinking_config,
    });

    let request = GoogleRequest {
        system_instruction: SystemInstruction {
            parts: [TextPart { text: system }],
        },
        contents: format_messages_google(
            messages,
            model_config
                .model_name
                .to_lowercase()
                .starts_with("gemini-3"),
        ),
        tools: tools_wrapper,
        generation_config,
    };

    Ok(serde_json::to_value(request)?)
}

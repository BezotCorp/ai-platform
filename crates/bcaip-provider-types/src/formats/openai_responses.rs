use crate::conversations::{Message, MessageContentBlock, ProviderUsage, Usage};
use crate::document_format::{
    ASSISTANT_ROLE_REASON, DocumentFormat, UNSUPPORTED_MEDIA_TYPE_REASON, convert_document,
    document_media_type_is_supported, unsupported_document_text,
};
use crate::errors::ProviderError;
use crate::formats::openai::{
    extract_reasoning_effort, is_openai_responses_model, openai_reasoning_effort_for_thinking,
    sanitize_function_name,
};
use crate::utils::{sanitize_unicode_tags, strip_unicode_tags};
use crate::{maybe_send::MaybeSend, mcp_utils::extract_text_from_resource, model::ModelConfig};
use anyhow::{Error, anyhow};
use async_stream::try_stream;
use chrono;
use futures::Stream;
use rmcp::model::{CallToolRequestParams, ContentBlock, Role, Tool, object};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashSet;
#[derive(Debug, Serialize, Deserialize)]
pub struct ResponsesApiResponse {
    pub id: String,
    pub object: String,
    pub created_at: i64,
    pub status: String,
    pub model: String,
    pub output: Vec<ResponseOutputItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponseReasoningInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ResponseUsage>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub struct SummaryText {
    pub text: String,
}

fn reasoning_from_summary(summary: &[SummaryText]) -> Option<MessageContentBlock> {
    let text: String = summary
        .iter()
        .map(|s| sanitize_unicode_tags(&s.text))
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        None
    } else {
        Some(MessageContentBlock::thinking(text, ""))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponseOutputItem {
    Reasoning {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default)]
        summary: Vec<SummaryText>,
    },
    Message {
        // `id` and `status` are required when the OpenAI API emits these
        // items, but Codex rollout files (which reuse the same shape on
        // disk) sometimes omit them. Keep deserialization permissive.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        role: String,
        content: Vec<ResponseContentBlock>,
    },
    FunctionCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        name: String,
        arguments: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponseContentBlock {
    OutputText {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        annotations: Option<Vec<Value>>,
    },
    Refusal {
        refusal: String,
    },
    ReasoningText {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: Value,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseReasoningInfo {
    pub effort: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ResponseIncompleteDetails {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

fn is_output_token_limit_incomplete_reason(reason: &str) -> bool {
    matches!(reason, "max_output_tokens" | "max_tokens")
}

fn response_reached_output_token_limit(
    status: &str,
    incomplete_details: Option<&ResponseIncompleteDetails>,
) -> bool {
    status == "incomplete"
        && incomplete_details
            .and_then(|details| details.reason.as_deref())
            .is_some_and(is_output_token_limit_incomplete_reason)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct InputTokensDetails {
    #[serde(default)]
    pub cached_tokens: Option<i32>,
    #[serde(default)]
    pub cache_write_tokens: Option<i32>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseUsage {
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub total_tokens: i32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_tokens_details: Option<InputTokensDetails>,
}

impl ResponseUsage {
    fn to_usage(&self) -> Usage {
        // input_tokens already includes both cache reads and cache writes
        let details = self.input_tokens_details.as_ref();
        let cached_tokens = details.and_then(|d| d.cached_tokens);
        let cache_write_tokens = details.and_then(|d| d.cache_write_tokens);
        Usage::new(
            Some(self.input_tokens),
            Some(self.output_tokens),
            Some(self.total_tokens),
        )
        .with_cache_tokens(cached_tokens, cache_write_tokens)
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponsesStreamEvent {
    #[serde(rename = "response.created")]
    ResponseCreated {
        sequence_number: i32,
        response: ResponseMetadata,
    },
    #[serde(rename = "response.in_progress")]
    ResponseInProgress {
        sequence_number: i32,
        response: ResponseMetadata,
    },
    #[serde(rename = "response.output_item.added")]
    OutputItemAdded {
        sequence_number: i32,
        output_index: i32,
        item: ResponseOutputItemInfo,
    },
    #[serde(rename = "response.content_part.added")]
    ContentBlockPartAdded {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        part: ContentBlockPart,
    },
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        delta: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        logprobs: Option<Vec<Value>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        obfuscation: Option<String>,
    },
    #[serde(rename = "response.output_item.done")]
    OutputItemDone {
        sequence_number: i32,
        output_index: i32,
        item: ResponseOutputItemInfo,
    },
    #[serde(rename = "response.content_part.done")]
    ContentBlockPartDone {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        part: ContentBlockPart,
    },
    #[serde(rename = "response.output_text.done")]
    OutputTextDone {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        logprobs: Option<Vec<Value>>,
    },
    #[serde(rename = "response.completed")]
    ResponseCompleted {
        sequence_number: i32,
        response: ResponseMetadata,
    },
    #[serde(rename = "response.incomplete")]
    ResponseIncomplete {
        sequence_number: i32,
        response: ResponseMetadata,
    },
    #[serde(rename = "response.failed")]
    ResponseFailed { sequence_number: i32, error: Value },
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        delta: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        obfuscation: Option<String>,
    },
    #[serde(rename = "response.function_call_arguments.done")]
    FunctionCallArgumentsDone {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        arguments: String,
    },
    #[serde(rename = "response.refusal.delta")]
    RefusalDelta {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        delta: String,
    },
    #[serde(rename = "response.refusal.done")]
    RefusalDone {
        sequence_number: i32,
        item_id: String,
        output_index: i32,
        content_index: i32,
        refusal: String,
    },
    #[serde(rename = "error")]
    Error { error: Value },
    #[serde(rename = "keepalive")]
    Keepalive {
        #[serde(default)]
        sequence_number: Option<i32>,
    },
}

fn is_known_responses_stream_event_type(event_type: &str) -> bool {
    matches!(
        event_type,
        "response.created"
            | "response.in_progress"
            | "response.output_item.added"
            | "response.content_part.added"
            | "response.output_text.delta"
            | "response.output_item.done"
            | "response.content_part.done"
            | "response.output_text.done"
            | "response.completed"
            | "response.incomplete"
            | "response.failed"
            | "response.function_call_arguments.delta"
            | "response.function_call_arguments.done"
            | "response.refusal.delta"
            | "response.refusal.done"
            | "error"
            | "keepalive"
    )
}

fn parse_responses_stream_event(data_line: &str) -> anyhow::Result<Option<ResponsesStreamEvent>> {
    let raw_event: Value = serde_json::from_str(data_line).map_err(|e| {
        ProviderError::stream_decode_error(format!(
            "Failed to parse Responses stream event: {}: {:?}",
            e, data_line
        ))
    })?;

    let Some(event_type) = raw_event.get("type").and_then(Value::as_str) else {
        return Ok(None);
    };

    if !is_known_responses_stream_event_type(event_type) {
        return Ok(None);
    }

    let event = serde_json::from_value(raw_event).map_err(|e| {
        ProviderError::stream_decode_error(format!(
            "Failed to parse Responses stream event: {}: {:?}",
            e, data_line
        ))
    })?;
    Ok(Some(event))
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ResponseMetadata {
    pub id: String,
    pub object: String,
    pub created_at: i64,
    pub status: String,
    pub model: String,
    #[serde(default)]
    pub output: Vec<ResponseOutputItemInfo>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<ResponseUsage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<ResponseReasoningInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub incomplete_details: Option<ResponseIncompleteDetails>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ResponseOutputItemInfo {
    Reasoning {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default)]
        summary: Vec<SummaryText>,
    },
    Message {
        #[serde(skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        role: String,
        content: Vec<ContentBlockPart>,
    },
    FunctionCall {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        status: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        call_id: Option<String>,
        name: String,
        arguments: String,
    },
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type")]
#[serde(rename_all = "snake_case")]
pub enum ContentBlockPart {
    OutputText {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        annotations: Option<Vec<Value>>,
        #[serde(skip_serializing_if = "Option::is_none")]
        logprobs: Option<Vec<Value>>,
    },
    Refusal {
        refusal: String,
    },
    ReasoningText {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: String,
    },
}

fn add_message_items(input_items: &mut Vec<Value>, messages: &[Message], supports_vision: bool) {
    for message in messages.iter().filter(|m| m.is_agent_visible()) {
        let role = match message.role {
            Role::User => "user",
            Role::Assistant => "assistant",
        };

        let mut text_items = Vec::new();

        for content in &message.content {
            match content {
                MessageContentBlock::Text(text) if !text.text.is_empty() => {
                    if message.role == Role::Assistant {
                        // Responses output_text items require annotations even when empty.
                        text_items.push(json!({
                            "type": "output_text",
                            "text": text.text,
                            "annotations": []
                        }));
                    } else {
                        text_items.push(json!({
                            "type": "input_text",
                            "text": text.text
                        }));
                    }
                }
                MessageContentBlock::ToolRequest(request) if message.role == Role::Assistant => {
                    if !text_items.is_empty() {
                        input_items.push(json!({
                            "type": "message",
                            "role": role,
                            "content": text_items
                        }));
                        text_items = Vec::new();
                    }

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

                            tracing::debug!(
                                "Replaying function_call with call_id: {}, name: {}",
                                request.id,
                                tool_call.name
                            );
                            input_items.push(json!({
                                "type": "function_call",
                                "call_id": request.id,
                                "name": sanitized_name,
                                "arguments": arguments_str
                            }));
                        }
                        Err(e) => {
                            input_items.push(json!({
                                "type": "function_call_output",
                                "call_id": request.id,
                                "output": format!("Error: {}", e.message)
                            }));
                        }
                    }
                }
                MessageContentBlock::Image(image) => {
                    if supports_vision {
                        text_items.push(json!({
                            "type": "input_image",
                            "image_url": format!("data:{};base64,{}", image.mime_type, image.data)
                        }));
                    } else {
                        text_items.push(json!({
                            "type": "input_text",
                            "text": "[image omitted: model does not support vision]"
                        }));
                    }
                }
                MessageContentBlock::Document(document) => {
                    if message.role != Role::User {
                        text_items.push(json!({
                            "type": "output_text",
                            "text": unsupported_document_text(document, ASSISTANT_ROLE_REASON),
                            "annotations": []
                        }));
                    } else if document_media_type_is_supported(&document.mime_type) {
                        let mut converted = convert_document(document, &DocumentFormat::OpenAi);
                        let mut file = converted["file"].take();
                        file["type"] = json!("input_file");
                        text_items.push(file);
                    } else {
                        text_items.push(json!({
                            "type": "input_text",
                            "text": unsupported_document_text(document, UNSUPPORTED_MEDIA_TYPE_REASON)
                        }));
                    }
                }
                MessageContentBlock::ToolResponse(response) => {
                    if !text_items.is_empty() {
                        input_items.push(json!({
                            "type": "message",
                            "role": role,
                            "content": text_items
                        }));
                        text_items = Vec::new();
                    }

                    match &response.tool_result {
                        Ok(contents) => {
                            let has_images = supports_vision
                                && contents
                                    .content
                                    .iter()
                                    .any(|c| matches!(c, ContentBlock::Image(_)));

                            let output = if has_images {
                                json!(contents
                                    .content
                                    .iter()
                                    .map(|c| match c {
                                        ContentBlock::Text(t) => json!({
                                            "type": "input_text", "text": t.text
                                        }),
                                        ContentBlock::Resource(r) => json!({
                                            "type": "input_text",
                                            "text": extract_text_from_resource(&r.resource)
                                        }),
                                        ContentBlock::Image(image) => json!({
                                            "type": "input_image",
                                            "image_url": format!(
                                                "data:{};base64,{}",
                                                image.mime_type, image.data
                                            )
                                        }),
                                        ContentBlock::Audio(_) => json!({
                                            "type": "input_text", "text": "[Audio content]"
                                        }),
                                        ContentBlock::ResourceLink(_) => json!({
                                            "type": "input_text", "text": "[Resource link]"
                                        }),
                                        _ => json!({
                                            "type": "input_text", "text": "[Unsupported content]"
                                        }),
                                    })
                                    .collect::<Vec<Value>>())
                            } else {
                                json!(
                                    contents
                                        .content
                                        .iter()
                                        .map(|c| match c {
                                            ContentBlock::Text(t) => t.text.clone(),
                                            ContentBlock::Resource(r) => {
                                                extract_text_from_resource(&r.resource)
                                            }
                                            ContentBlock::Audio(_) => "[Audio content]".into(),
                                            ContentBlock::ResourceLink(_) => {
                                                "[Resource link]".into()
                                            }
                                            ContentBlock::Image(_) =>
                                                "[image omitted: model does not support vision]"
                                                    .into(),
                                            _ => "[Unsupported content]".into(),
                                        })
                                        .collect::<Vec<String>>()
                                        .join("\n")
                                )
                            };

                            input_items.push(json!({
                                "type": "function_call_output",
                                "call_id": response.id,
                                "output": output
                            }));
                        }
                        Err(error_data) => {
                            tracing::debug!(
                                "Sending function_call_output error with call_id: {}",
                                response.id
                            );
                            input_items.push(json!({
                                "type": "function_call_output",
                                "call_id": response.id,
                                "output": format!("Error: {}", error_data.message)
                            }));
                        }
                    }
                }
                _ => {}
            }
        }

        if !text_items.is_empty() {
            input_items.push(json!({
                "type": "message",
                "role": role,
                "content": text_items
            }));
        }
    }
}

fn is_gpt_5_6_model(model_name: &str) -> bool {
    let normalized = model_name.to_ascii_lowercase();
    ["gpt-5.6", "gpt-5-6"].iter().any(|needle| {
        normalized.match_indices(needle).any(|(start, matched)| {
            let before = start
                .checked_sub(1)
                .and_then(|index| normalized.as_bytes().get(index));
            let after = normalized.as_bytes().get(start + matched.len());

            before.is_none_or(|byte| matches!(byte, b'-' | b'/'))
                && after.is_none_or(|byte| matches!(byte, b'-' | b'/'))
        })
    })
}

pub fn create_responses_request(
    model_config: &ModelConfig,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
) -> anyhow::Result<Value, Error> {
    let (wire_model_name, _) = extract_reasoning_effort(&model_config.model_name);
    create_responses_request_for_model(
        model_config,
        &wire_model_name,
        &model_config.model_name,
        system,
        messages,
        tools,
    )
}

pub fn create_responses_request_for_model(
    model_config: &ModelConfig,
    wire_model_name: &str,
    capability_model_name: &str,
    system: &str,
    messages: &[Message],
    tools: &[Tool],
) -> anyhow::Result<Value, Error> {
    let mut input_items = Vec::new();

    if !system.is_empty() {
        input_items.push(json!({
            "type": "message",
            "role": "system",
            "content": [{
                "type": "input_text",
                "text": system
            }]
        }));
    }

    add_message_items(
        &mut input_items,
        messages,
        model_config.supports_vision.unwrap_or_default(),
    );

    let (model_name, legacy_reasoning_effort) = extract_reasoning_effort(capability_model_name);
    // All models routed here are responses-capable; temperature is rejected
    // by the API for reasoning models regardless of whether an explicit
    // effort suffix was provided.
    let is_reasoning_model = is_openai_responses_model(&model_name);
    let reasoning_effort = if is_reasoning_model {
        if let Some(effort) = legacy_reasoning_effort.as_deref() {
            if effort.eq_ignore_ascii_case("none") {
                legacy_reasoning_effort
            } else {
                effort
                    .parse()
                    .ok()
                    .and_then(|effort| openai_reasoning_effort_for_thinking(&model_name, effort))
                    .or(legacy_reasoning_effort)
            }
        } else {
            model_config
                .thinking_effort()
                .and_then(|effort| openai_reasoning_effort_for_thinking(&model_name, effort))
        }
    } else {
        None
    };

    let store = model_config.request_param::<bool>("store").unwrap_or(false);
    let reasoning_mode = model_config
        .request_param::<String>("reasoning_mode")
        .map(|mode| {
            let normalized = mode.to_ascii_lowercase();
            match normalized.as_str() {
                "standard" | "pro" => Ok(normalized),
                _ => Err(anyhow!(
                    "Invalid reasoning_mode '{}'. Supported values are: standard, pro",
                    mode
                )),
            }
        })
        .transpose()?;
    if reasoning_mode.is_some() && !is_gpt_5_6_model(&model_name) {
        return Err(anyhow!(
            "reasoning_mode is only supported for GPT-5.6 models"
        ));
    }
    let mut payload = json!({
        "model": wire_model_name,
        "input": input_items,
        "store": store,
    });

    if reasoning_effort.is_some() || reasoning_mode.is_some() {
        let mut reasoning = serde_json::Map::new();
        if let Some(effort) = reasoning_effort {
            reasoning.insert("effort".to_string(), json!(effort));
            reasoning.insert("summary".to_string(), json!("auto"));
        }
        if let Some(mode) = reasoning_mode {
            reasoning.insert("mode".to_string(), json!(mode));
        }
        payload
            .as_object_mut()
            .unwrap()
            .insert("reasoning".to_string(), Value::Object(reasoning));
    }

    if !tools.is_empty() {
        let tools_spec: Vec<Value> = tools
            .iter()
            .map(|tool| {
                json!({
                    "type": "function",
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.input_schema,
                    "strict": false,
                })
            })
            .collect();

        payload
            .as_object_mut()
            .unwrap()
            .insert("tools".to_string(), json!(tools_spec));
    }

    if !is_reasoning_model && let Some(temp) = model_config.temperature {
        payload
            .as_object_mut()
            .unwrap()
            .insert("temperature".to_string(), json!(temp));
    }

    if let Some(max_tokens) = model_config.max_tokens {
        payload
            .as_object_mut()
            .unwrap()
            .insert("max_output_tokens".to_string(), json!(max_tokens));
    }

    Ok(payload)
}

fn sanitize_tool_arguments(value: Value) -> anyhow::Result<Value> {
    match value {
        Value::String(text) => Ok(Value::String(strip_unicode_tags(&text))),
        Value::Array(values) => Ok(Value::Array(
            values
                .into_iter()
                .map(sanitize_tool_arguments)
                .collect::<anyhow::Result<_>>()?,
        )),
        Value::Object(values) => {
            let mut sanitized = serde_json::Map::new();
            for (key, value) in values {
                let key = strip_unicode_tags(&key);
                if sanitized.contains_key(&key) {
                    return Err(anyhow!(
                        "Responses tool arguments contain duplicate key after Unicode tag sanitization"
                    ));
                }
                sanitized.insert(key, sanitize_tool_arguments(value)?);
            }
            Ok(Value::Object(sanitized))
        }
        value => Ok(value),
    }
}

fn parse_tool_arguments(arguments: &str) -> anyhow::Result<Value> {
    if arguments.is_empty() {
        Ok(json!({}))
    } else {
        match serde_json::from_str(arguments) {
            Ok(value) => sanitize_tool_arguments(value),
            Err(_) => Ok(json!({})),
        }
    }
}

fn sanitize_tool_request_id(id: &str, seen_ids: &mut HashSet<String>) -> anyhow::Result<String> {
    let id = strip_unicode_tags(id);
    if !seen_ids.insert(id.clone()) {
        return Err(anyhow!(
            "Responses tool calls contain duplicate ID after Unicode tag sanitization"
        ));
    }
    Ok(id)
}

pub fn responses_api_to_message(response: &ResponsesApiResponse) -> anyhow::Result<Message> {
    let mut content = Vec::new();
    let mut tool_request_ids = HashSet::new();

    for item in &response.output {
        match item {
            ResponseOutputItem::Reasoning { summary, .. } => {
                content.extend(reasoning_from_summary(summary));
            }
            ResponseOutputItem::Message {
                content: msg_content,
                ..
            } => {
                for block in msg_content {
                    match block {
                        ResponseContentBlock::OutputText { text, .. } => {
                            let text = sanitize_unicode_tags(text);
                            if !text.is_empty() {
                                content.push(MessageContentBlock::text(text));
                            }
                        }
                        ResponseContentBlock::Refusal { refusal } => {
                            let refusal = sanitize_unicode_tags(refusal);
                            if !refusal.is_empty() {
                                content.push(MessageContentBlock::text(refusal));
                            }
                        }
                        ResponseContentBlock::ReasoningText { text } => {
                            let text = sanitize_unicode_tags(text);
                            if !text.is_empty() {
                                content.push(MessageContentBlock::thinking(text, ""));
                            }
                        }
                        ResponseContentBlock::ToolCall { id, name, input } => {
                            let id = sanitize_tool_request_id(id, &mut tool_request_ids)?;
                            content.push(MessageContentBlock::tool_request(
                                id,
                                Ok(CallToolRequestParams::new(strip_unicode_tags(name))
                                    .with_arguments(object(sanitize_tool_arguments(
                                        input.clone(),
                                    )?))),
                            ));
                        }
                    }
                }
            }
            ResponseOutputItem::FunctionCall {
                id,
                call_id,
                name,
                arguments,
                ..
            } => {
                let request_id = call_id.clone().or_else(|| id.clone()).ok_or_else(|| {
                    anyhow!("Responses function_call output missing call_id and id")
                })?;
                let request_id = sanitize_tool_request_id(&request_id, &mut tool_request_ids)?;
                let parsed_args = parse_tool_arguments(arguments)?;

                content.push(MessageContentBlock::tool_request(
                    request_id,
                    Ok(CallToolRequestParams::new(strip_unicode_tags(name))
                        .with_arguments(object(parsed_args))),
                ));
            }
        }
    }

    let mut message = Message::new(Role::Assistant, chrono::Utc::now().timestamp(), content);

    message = message.with_id(response.id.clone());

    Ok(message)
}

pub fn get_responses_usage(response: &ResponsesApiResponse) -> Usage {
    response
        .usage
        .as_ref()
        .map_or_else(Usage::default, ResponseUsage::to_usage)
}

fn process_streaming_output_items(
    output_items: Vec<ResponseOutputItemInfo>,
    is_text_response: bool,
) -> anyhow::Result<Vec<MessageContentBlock>> {
    let mut content = Vec::new();
    let mut tool_request_ids = HashSet::new();

    for item in output_items {
        match item {
            ResponseOutputItemInfo::Reasoning { summary, .. } => {
                content.extend(reasoning_from_summary(&summary));
            }
            ResponseOutputItemInfo::Message { content: parts, .. } => {
                for part in parts {
                    match part {
                        ContentBlockPart::OutputText { text, .. } => {
                            let text = sanitize_unicode_tags(&text);
                            if !text.is_empty() && !is_text_response {
                                content.push(MessageContentBlock::text(text));
                            }
                        }
                        ContentBlockPart::Refusal { refusal } => {
                            let refusal = sanitize_unicode_tags(&refusal);
                            if !refusal.is_empty() && !is_text_response {
                                content.push(MessageContentBlock::text(refusal));
                            }
                        }
                        ContentBlockPart::ReasoningText { text } => {
                            let text = sanitize_unicode_tags(&text);
                            if !text.is_empty() {
                                content.push(MessageContentBlock::thinking(text, ""));
                            }
                        }
                        ContentBlockPart::ToolCall {
                            id,
                            name,
                            arguments,
                        } => {
                            let id = sanitize_tool_request_id(&id, &mut tool_request_ids)?;
                            let parsed_args = parse_tool_arguments(&arguments)?;

                            content.push(MessageContentBlock::tool_request(
                                id,
                                Ok(CallToolRequestParams::new(strip_unicode_tags(&name))
                                    .with_arguments(object(parsed_args))),
                            ));
                        }
                    }
                }
            }
            ResponseOutputItemInfo::FunctionCall {
                id,
                call_id,
                name,
                arguments,
                ..
            } => {
                let request_id = call_id.or(id).ok_or_else(|| {
                    anyhow!("Responses function_call output missing call_id and id")
                })?;
                let request_id = sanitize_tool_request_id(&request_id, &mut tool_request_ids)?;
                let parsed_args = parse_tool_arguments(&arguments)?;

                content.push(MessageContentBlock::tool_request(
                    request_id,
                    Ok(CallToolRequestParams::new(strip_unicode_tags(&name))
                        .with_arguments(object(parsed_args))),
                ));
            }
        }
    }

    Ok(content)
}

fn output_token_limit_marker(id: Option<String>) -> Message {
    let mut message = Message::assistant();
    if let Some(id) = id {
        message = message.with_id(id);
    }
    message.metadata.output_token_limit_reached = true;
    message
}

/// Parse a line per the SSE grammar and return its field name:
/// - `field: value` / `field:value` -> `Some(field)`
/// - `field` (no colon, empty value) -> `Some(field)`
/// - `: comment` -> `Some("")`
///
/// Returns `None` when the line does not look like an SSE field (e.g. a
/// bare JSON payload such as `{"type": ...}`), so callers can fall back
/// to parsing it as JSON.
fn sse_field_name(line: &str) -> Option<&str> {
    let field = line.split_once(':').map_or(line, |(name, _)| name);
    if field.is_empty() {
        return Some("");
    }
    let field_like = !field.contains(char::is_whitespace)
        && field
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
    field_like.then_some(field)
}

pub fn responses_api_to_streaming_message<S>(
    mut stream: S,
) -> impl Stream<Item = anyhow::Result<(Option<Message>, Option<ProviderUsage>)>> + 'static
where
    S: Stream<Item = anyhow::Result<String>> + Unpin + MaybeSend + 'static,
{
    try_stream! {
        use futures::StreamExt;
        let mut accumulated_text = String::new();
        let mut response_id: Option<String> = None;
        let mut model_name: Option<String> = None;
        let mut final_usage: Option<ProviderUsage> = None;
        let mut output_items: Vec<ResponseOutputItemInfo> = Vec::new();
        let mut is_text_response = false;
        let mut output_token_limit_reached = false;

        'outer: while let Some(response) = stream.next().await {
            let response_str = response?;

            // Skip empty lines
            if response_str.trim().is_empty() {
                continue;
            }
            if response_str.starts_with(':') {
                continue;
            }

            // Parse SSE format: "event: <type>\ndata: <json>"
            // For now, we only care about the data line
            // SSE spec allows both "data: value" and "data:value" (space after colon is optional)
            let data_line = if response_str.starts_with("data: ") {
                response_str.strip_prefix("data: ").unwrap()
            } else if response_str.starts_with("data:") {
                response_str.strip_prefix("data:").unwrap()
            } else if sse_field_name(&response_str).is_some_and(|f| f != "data") {
                // Skip payload-free SSE fields: event, id, retry, comments,
                // colon-less fields with empty values, and unknown extension
                // fields — the SSE spec requires all of these to be ignored.
                continue;
            } else {
                // Try to parse as-is when there's no prefix (bare JSON frames)
                &response_str
            };

            if data_line == "[DONE]" {
                break 'outer;
            }

            let Some(event) = parse_responses_stream_event(data_line)? else {
                continue;
            };

            match event {
                ResponsesStreamEvent::ResponseCreated { response, .. } |
                ResponsesStreamEvent::ResponseInProgress { response, .. } => {
                    response_id = Some(response.id);
                    model_name = Some(response.model);
                }

                ResponsesStreamEvent::OutputTextDelta { delta, .. } => {
                    is_text_response = true;
                    let delta = strip_unicode_tags(&delta);
                    if !delta.is_empty() {
                        accumulated_text.push_str(&delta);

                        // Yield incremental text updates for true streaming
                        let mut msg = Message::new(
                            Role::Assistant,
                            chrono::Utc::now().timestamp(),
                            vec![MessageContentBlock::text(&delta)],
                        );

                        // Add ID so desktop client knows these deltas are part of the same message
                        if let Some(id) = &response_id {
                            msg = msg.with_id(id.clone());
                        }

                        yield (Some(msg), None);
                    }
                }

                ResponsesStreamEvent::OutputItemDone { item, .. } => {
                    output_items.push(item);
                }

                ResponsesStreamEvent::OutputTextDone { .. } => {
                    // Text is already complete from deltas, this is just a summary event
                }

                ResponsesStreamEvent::ResponseCompleted { response, .. } => {
                    let model = model_name.as_ref().unwrap_or(&response.model);
                    let usage = response.usage.as_ref().map_or_else(
                        Usage::default,
                        ResponseUsage::to_usage,
                    );
                    let mut pu = ProviderUsage::new(model.clone(), usage);
                    pu.finish_reasons = Some(vec![response.status.clone()]);
                    pu.response_id = Some(response.id.clone());
                    final_usage = Some(pu);

                    // For complete output, use the response output items
                    if !response.output.is_empty() {
                        output_items = response.output;
                    }

                    break 'outer;
                }

                ResponsesStreamEvent::ResponseIncomplete { response, .. } => {
                    let model = model_name.as_ref().unwrap_or(&response.model);
                    let usage = response.usage.as_ref().map_or_else(
                        Usage::default,
                        ResponseUsage::to_usage,
                    );
                    let mut pu = ProviderUsage::new(model.clone(), usage);
                    pu.finish_reasons = Some(vec![response
                        .incomplete_details
                        .as_ref()
                        .and_then(|details| details.reason.clone())
                        .unwrap_or_else(|| response.status.clone())]);
                    pu.response_id = Some(response.id.clone());
                    final_usage = Some(pu);
                    response_id = Some(response.id.clone());
                    output_token_limit_reached = response_reached_output_token_limit(
                        &response.status,
                        response.incomplete_details.as_ref(),
                    );

                    if !response.output.is_empty() {
                        output_items = response
                            .output
                            .into_iter()
                            .filter(|item| match item {
                                ResponseOutputItemInfo::FunctionCall { status, .. } => {
                                    status.as_deref() == Some("completed")
                                }
                                _ => true,
                            })
                            .collect();
                    }

                    break 'outer;
                }

                ResponsesStreamEvent::FunctionCallArgumentsDelta { .. } => {
                    // Function call arguments are being streamed, but we'll get the complete
                    // arguments in the OutputItemDone event, so we can ignore deltas for now
                }

                ResponsesStreamEvent::FunctionCallArgumentsDone { .. } => {
                    // Arguments are complete, will be in the OutputItemDone event
                }

                ResponsesStreamEvent::RefusalDelta { delta, .. } => {
                    is_text_response = true;
                    let delta = strip_unicode_tags(&delta);
                    if !delta.is_empty() {
                        accumulated_text.push_str(&delta);

                        let mut msg = Message::new(
                            Role::Assistant,
                            chrono::Utc::now().timestamp(),
                            vec![MessageContentBlock::text(&delta)],
                        );

                        if let Some(id) = &response_id {
                            msg = msg.with_id(id.clone());
                        }

                        yield (Some(msg), None);
                    }
                }

                ResponsesStreamEvent::RefusalDone { .. } => {
                    // Refusal text already streamed via deltas
                }

                ResponsesStreamEvent::ResponseFailed { error, .. } => {
                    Err::<(), ProviderError>(ProviderError::RequestFailed(format!(
                        "Responses API failed: {:?}",
                        error
                    )))?;
                }

                ResponsesStreamEvent::Error { error } => {
                    Err::<(), ProviderError>(ProviderError::RequestFailed(format!(
                        "Responses API error: {:?}",
                        error
                    )))?;
                }

                _ => {
                    // Ignore other event types (OutputItemAdded, ContentBlockPartAdded, ContentBlockPartDone)
                }
            }
        }

        // Process final output items and yield usage data
        let content = process_streaming_output_items(output_items, is_text_response)?;

        if !content.is_empty() {
            let mut message = Message::new(Role::Assistant, chrono::Utc::now().timestamp(), content);
            if let Some(id) = response_id {
                message = message.with_id(id);
            }
            message.metadata.output_token_limit_reached = output_token_limit_reached;
            yield (Some(message), final_usage);
        } else if output_token_limit_reached {
            yield (Some(output_token_limit_marker(response_id)), final_usage);
        } else if let Some(usage) = final_usage {
            yield (None, Some(usage));
        }
    }
}

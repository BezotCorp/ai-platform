use crate::mcp_utils::ToolResult;
use crate::{
    acp::{server::message_meta::merge_message_meta, tools::AcpAwareToolMeta},
    agents::extension_manager::TRUSTED_TOOL_UPDATE_META_KEY,
};
use agent_client_protocol::schema::v1::{
    BlobResourceContents, Content, ContentBlock, EmbeddedResource, EmbeddedResourceResource,
    ImageContent, Meta, TextContent, TextResourceContents, ToolCall, ToolCallContent, ToolCallId,
    ToolCallLocation, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
};
use bcaip_provider_types::conversations::{Message, ToolNameParts, ToolRequest, ToolResponse};
use rmcp::model::{CallToolResult, ContentBlock as RmcpContentBlock, ResourceContents};

pub(crate) fn format_tool_name(tool_name: &str) -> String {
    let parts = ToolNameParts::from(tool_name);
    if let Some(extension_name) = parts.extension_name {
        format!(
            "{}: {}",
            extension_name.replace('_', " "),
            parts.tool_name.replace('_', " ")
        )
    } else {
        parts.tool_name.replace('_', " ")
    }
}

fn default_tool_title(tool_name: &str, arguments: Option<&serde_json::Value>) -> String {
    let base = format_tool_name(tool_name);

    let detail = arguments.and_then(|args| {
        let obj = args.as_object()?;
        let keys = if matches!(tool_name, "developer__shell" | "shell") {
            [
                "command", "path", "file", "query", "url", "uri", "name", "pattern", "source",
            ]
        } else {
            [
                "path", "file", "command", "query", "url", "uri", "name", "pattern", "source",
            ]
        };
        for key in &keys {
            if let Some(v) = obj.get(*key) {
                let s = match v {
                    serde_json::Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if !s.is_empty() {
                    let mut lines = s.lines();
                    let first_line = lines.next().unwrap_or(&s);
                    if first_line.len() > 60 || lines.next().is_some() {
                        return Some(format!("{}…", crate::utils::safe_truncate(first_line, 57)));
                    }
                    return Some(first_line.to_string());
                }
            }
        }
        None
    });

    match detail {
        Some(d) => format!("{base} · {d}"),
        None => base,
    }
}

pub(crate) fn goose_tool_call_meta(tool_request: &ToolRequest) -> Option<Meta> {
    let tool_call = tool_request.tool_call.as_ref().ok()?;
    let tool_name = tool_call.name.to_string();
    let extension_name = tool_request
        .tool_name_parts()
        .and_then(|parts| parts.extension_name)
        .map(ToString::to_string);

    let mut tool_call_meta = serde_json::Map::new();
    tool_call_meta.insert("toolName".to_string(), serde_json::Value::String(tool_name));
    if let Some(extension_name) = extension_name {
        tool_call_meta.insert(
            "extensionName".to_string(),
            serde_json::Value::String(extension_name),
        );
    }

    let mut goose_meta = serde_json::Map::new();
    goose_meta.insert(
        "toolCall".to_string(),
        serde_json::Value::Object(tool_call_meta),
    );

    let mut meta = serde_json::Map::new();
    meta.insert("goose".to_string(), serde_json::Value::Object(goose_meta));
    Some(meta)
}

fn build_initial_tool_call(tool_request: &ToolRequest, include_generated_title: bool) -> ToolCall {
    let tool_name = match &tool_request.tool_call {
        Ok(tool_call) => tool_call.name.to_string(),
        Err(_) => "error".to_string(),
    };
    let args_value = tool_request
        .tool_call
        .as_ref()
        .ok()
        .and_then(|tc| tc.arguments.as_ref())
        .map(|a| serde_json::Value::Object(a.clone()));
    let default_tool_call_title = default_tool_title(&tool_name, args_value.as_ref());
    let goose_meta = goose_tool_call_meta(tool_request);

    let initial_title = if include_generated_title {
        tool_request
            .generated_title()
            .map(str::to_string)
            .unwrap_or(default_tool_call_title)
    } else {
        default_tool_call_title
    };

    let mut tool_call = ToolCall::new(ToolCallId::new(tool_request.id.clone()), initial_title)
        .status(ToolCallStatus::Pending);
    if let Some(args) = args_value {
        tool_call = tool_call.raw_input(args);
    }

    tool_call.meta(goose_meta)
}

pub(crate) fn build_initial_tool_call_with_message_meta(
    tool_request: &ToolRequest,
    message: &Message,
    include_generated_title: bool,
) -> ToolCall {
    let mut tool_call = build_initial_tool_call(tool_request, include_generated_title);
    let meta = tool_call.meta.take().unwrap_or_default();
    tool_call.meta(merge_message_meta(meta, message))
}

pub(crate) fn build_permission_tool_call_update(
    request_id: &str,
    tool_name: &str,
    arguments: serde_json::Map<String, serde_json::Value>,
    prompt: Option<String>,
) -> ToolCallUpdate {
    let arguments = serde_json::Value::Object(arguments);
    let mut fields = ToolCallUpdateFields::new()
        .title(default_tool_title(tool_name, Some(&arguments)))
        .kind(ToolKind::default())
        .status(ToolCallStatus::Pending)
        .raw_input(arguments);

    if let Some(prompt) = prompt {
        fields = fields.content(vec![ToolCallContent::Content(Content::new(
            ContentBlock::Text(TextContent::new(prompt)),
        ))]);
    }

    ToolCallUpdate::new(ToolCallId::new(request_id), fields)
}

fn json_u32(value: &serde_json::Value) -> Option<u32> {
    value.as_u64().and_then(|value| u32::try_from(value).ok())
}

fn extract_tool_locations_from_response(
    tool_response: &ToolResponse,
) -> Option<Vec<ToolCallLocation>> {
    let result = tool_response.tool_result.as_ref().ok()?;
    let meta = result.meta.as_ref()?;
    let locations_val = meta.get("tool_locations")?;
    let entries: Vec<serde_json::Value> = serde_json::from_value(locations_val.clone()).ok()?;
    let locations = entries
        .into_iter()
        .filter_map(|entry| {
            let path = entry.get("path")?.as_str()?;
            let line = entry.get("line").and_then(json_u32);
            Some(ToolCallLocation::new(path).line(line))
        })
        .collect::<Vec<_>>();
    if locations.is_empty() {
        None
    } else {
        Some(locations)
    }
}

fn extract_tool_locations_from_request(tool_request: &ToolRequest) -> Vec<ToolCallLocation> {
    let Some(parts) = tool_request.tool_name_parts() else {
        return Vec::new();
    };
    if parts.extension_name != Some("developer") {
        return Vec::new();
    }

    let Ok(tool_call) = &tool_request.tool_call else {
        return Vec::new();
    };
    let Some(path) = tool_call
        .arguments
        .as_ref()
        .and_then(|args| args.get("path"))
        .and_then(|path| path.as_str())
    else {
        return Vec::new();
    };

    let line = match parts.tool_name {
        "read" => tool_call
            .arguments
            .as_ref()
            .and_then(|arguments| arguments.get("line"))
            .and_then(json_u32),
        "write" | "edit" => Some(1),
        _ => return Vec::new(),
    };
    vec![ToolCallLocation::new(path).line(line)]
}

pub(crate) fn trusted_update_meta(tool_response: &ToolResponse) -> Option<Meta> {
    let tool_result = tool_response.tool_result.as_ref().ok()?;
    let goose_meta = tool_result
        .meta
        .as_ref()?
        .0
        .get(TRUSTED_TOOL_UPDATE_META_KEY)?
        .clone();
    let mut meta_map = serde_json::Map::new();
    meta_map.insert("goose".to_string(), goose_meta);
    Some(meta_map)
}

fn build_tool_call_content(tool_result: &ToolResult<CallToolResult>) -> Vec<ToolCallContent> {
    match tool_result {
        Ok(result) => result
            .content
            .iter()
            .filter_map(|content| match content {
                RmcpContentBlock::Text(val) => Some(ToolCallContent::Content(Content::new(
                    ContentBlock::Text(TextContent::new(val.text.clone())),
                ))),
                RmcpContentBlock::Image(val) => Some(ToolCallContent::Content(Content::new(
                    ContentBlock::Image(ImageContent::new(val.data.clone(), val.mime_type.clone())),
                ))),
                RmcpContentBlock::Resource(val) => {
                    let resource = match &val.resource {
                        ResourceContents::TextResourceContents {
                            mime_type,
                            text,
                            uri,
                            ..
                        } => EmbeddedResourceResource::TextResourceContents(
                            TextResourceContents::new(text.clone(), uri.clone())
                                .mime_type(mime_type.clone()),
                        ),
                        ResourceContents::BlobResourceContents {
                            mime_type,
                            blob,
                            uri,
                            ..
                        } => EmbeddedResourceResource::BlobResourceContents(
                            BlobResourceContents::new(blob.clone(), uri.clone())
                                .mime_type(mime_type.clone()),
                        ),
                        _ => return None,
                    };
                    Some(ToolCallContent::Content(Content::new(
                        ContentBlock::Resource(EmbeddedResource::new(resource)),
                    )))
                }
                RmcpContentBlock::Audio(_) | RmcpContentBlock::ResourceLink(_) => None,
                _ => None,
            })
            .collect(),
        Err(error) => vec![ToolCallContent::Content(Content::new(ContentBlock::Text(
            TextContent::new(error.message.to_string()),
        )))],
    }
}

fn extract_tool_raw_output(tool_result: &ToolResult<CallToolResult>) -> Option<serde_json::Value> {
    tool_result
        .as_ref()
        .ok()
        .and_then(|result| result.structured_content.clone())
}

pub(crate) fn tool_call_update_fields_from_response(
    tool_response: &ToolResponse,
    tool_request: Option<&ToolRequest>,
    include_content_for_acp_aware_tools: bool,
) -> ToolCallUpdateFields {
    let is_failed = match &tool_response.tool_result {
        Ok(result) => result.is_error == Some(true),
        Err(_) => true,
    };
    let status = if is_failed {
        ToolCallStatus::Failed
    } else {
        ToolCallStatus::Completed
    };

    let mut fields = ToolCallUpdateFields::new().status(status);
    if let Some(raw_output) = extract_tool_raw_output(&tool_response.tool_result) {
        fields = fields.raw_output(raw_output);
    }
    let is_acp_aware = tool_response
        .tool_result
        .as_ref()
        .is_ok_and(|result| result.is_acp_aware());
    let include_content = include_content_for_acp_aware_tools || is_failed || !is_acp_aware;
    let include_locations = !is_acp_aware;

    if include_content {
        fields = fields.content(build_tool_call_content(&tool_response.tool_result));
    }

    if include_locations {
        let locations = extract_tool_locations_from_response(tool_response).unwrap_or_else(|| {
            tool_request
                .map(extract_tool_locations_from_request)
                .unwrap_or_default()
        });
        if !locations.is_empty() {
            fields = fields.locations(locations);
        }
    }

    fields
}

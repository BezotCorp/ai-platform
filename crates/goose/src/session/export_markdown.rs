use crate::utils::safe_truncate;
use bcaip_provider_types::conversations::{
    ActionRequiredData, Message, MessageContent, ToolNameParts, ToolRequest, ToolResponse,
};
use rmcp::model::{ContentBlock, ResourceContents, Role};
use serde_json::Value;
const MAX_STRING_LENGTH_MD_EXPORT: usize = 4096; // Generous limit for export
const REDACTED_PREFIX_LENGTH: usize = 100; // Show first 100 chars before trimming

fn value_to_simple_markdown_string(value: &Value, export_full_strings: bool) -> String {
    match value {
        Value::String(s) => {
            if !export_full_strings && s.chars().count() > MAX_STRING_LENGTH_MD_EXPORT {
                let prefix = safe_truncate(s, REDACTED_PREFIX_LENGTH);
                let trimmed_chars = s.chars().count() - prefix.chars().count();
                format!("`{}[ ... trimmed : {} chars ... ]`", prefix, trimmed_chars)
            } else {
                // Escape backticks and newlines for inline code.
                let escaped = s.replace('`', "\\`").replace("\n", "\\\\n");
                format!("`{}`", escaped)
            }
        }
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => format!("*{}*", b),
        Value::Null => "_null_".to_string(),
        _ => "`[Complex Value]`".to_string(),
    }
}

fn value_to_markdown(value: &Value, depth: usize, export_full_strings: bool) -> String {
    let mut md_string = String::new();
    let base_indent_str = "  ".repeat(depth); // Basic indentation for nesting

    match value {
        Value::Object(map) => {
            if map.is_empty() {
                md_string.push_str(&format!("{}*empty object*\n", base_indent_str));
            } else {
                for (key, val) in map {
                    md_string.push_str(&format!("{}*   **{}**: ", base_indent_str, key));
                    match val {
                        Value::String(s) => {
                            if s.contains('\n') || s.chars().count() > 80 {
                                // Heuristic for block
                                md_string.push_str(&format!(
                                    "\n{}    ```\n{}{}\n{}    ```\n",
                                    base_indent_str,
                                    base_indent_str,
                                    s.trim(),
                                    base_indent_str
                                ));
                            } else {
                                md_string.push_str(&format!("`{}`\n", s.replace('`', "\\`")));
                            }
                        }
                        _ => {
                            // Use recursive call for all values including complex objects/arrays
                            md_string.push('\n');
                            md_string.push_str(&value_to_markdown(
                                val,
                                depth + 2,
                                export_full_strings,
                            ));
                        }
                    }
                }
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                md_string.push_str(&format!("{}*   *empty list*\n", base_indent_str));
            } else {
                for item in arr {
                    md_string.push_str(&format!("{}*   - ", base_indent_str));
                    match item {
                        Value::String(s) => {
                            if s.contains('\n') || s.chars().count() > 80 {
                                // Heuristic for block
                                md_string.push_str(&format!(
                                    "\n{}      ```\n{}{}\n{}      ```\n",
                                    base_indent_str,
                                    base_indent_str,
                                    s.trim(),
                                    base_indent_str
                                ));
                            } else {
                                md_string.push_str(&format!("`{}`\n", s.replace('`', "\\`")));
                            }
                        }
                        _ => {
                            // Use recursive call for all values including complex objects/arrays
                            md_string.push('\n');
                            md_string.push_str(&value_to_markdown(
                                item,
                                depth + 2,
                                export_full_strings,
                            ));
                        }
                    }
                }
            }
        }
        _ => {
            md_string.push_str(&format!(
                "{}{}\n",
                base_indent_str,
                value_to_simple_markdown_string(value, export_full_strings)
            ));
        }
    }
    md_string
}

fn is_shell_tool_name(tool_name: &str) -> bool {
    matches!(tool_name, "shell")
}

fn is_developer_file_tool_name(tool_name: &str) -> bool {
    matches!(tool_name, "write" | "edit")
}

pub fn tool_request_to_markdown(req: &ToolRequest, export_all_content: bool) -> String {
    let mut md = String::new();
    match &req.tool_call {
        Ok(call) => {
            let name_parts = ToolNameParts::from(call.name.as_ref());
            let namespace = match name_parts.extension_name {
                Some(extension_name) => extension_name,
                None if is_shell_tool_name(call.name.as_ref())
                    || is_developer_file_tool_name(call.name.as_ref()) =>
                {
                    "developer"
                }
                None => "Tool",
            };

            md.push_str(&format!(
                "#### Tool Call: `{}` (namespace: `{}`)\n",
                name_parts.tool_name, namespace
            ));
            md.push_str("**Arguments:**\n");

            match call.name.as_ref() {
                name if is_shell_tool_name(name) => {
                    if let Some(Value::String(command)) =
                        call.arguments.as_ref().and_then(|args| args.get("command"))
                    {
                        md.push_str(&format!(
                            "*   **command**:\n    ```sh\n    {}\n    ```\n",
                            command.trim()
                        ));
                    }
                    let other_args: serde_json::Map<String, Value> = call
                        .arguments
                        .as_ref()
                        .map(|obj| {
                            obj.iter()
                                .filter(|(k, _)| k.as_str() != "command")
                                .map(|(k, v)| (k.clone(), v.clone()))
                                .collect()
                        })
                        .unwrap_or_default();
                    if !other_args.is_empty() {
                        md.push_str(&value_to_markdown(
                            &Value::Object(other_args),
                            0,
                            export_all_content,
                        ));
                    }
                }
                name if is_developer_file_tool_name(name) => {
                    if let Some(Value::String(path)) =
                        call.arguments.as_ref().and_then(|args| args.get("path"))
                    {
                        md.push_str(&format!("*   **path**: `{}`\n", path));
                    }

                    if let Some(args) = &call.arguments {
                        let mut other_args = args.clone();
                        other_args.remove("path");
                        if !other_args.is_empty() {
                            md.push_str(&value_to_markdown(
                                &Value::Object(other_args),
                                0,
                                export_all_content,
                            ));
                        }
                    } else {
                        md.push_str("*No arguments*\n");
                    }
                }
                _ => {
                    if let Some(args) = &call.arguments {
                        md.push_str(&value_to_markdown(
                            &Value::Object(args.clone()),
                            0,
                            export_all_content,
                        ));
                    } else {
                        md.push_str("*No arguments*\n");
                    }
                }
            }
        }
        Err(e) => {
            md.push_str(&format!(
                "**Error in Tool Call:**\n```\n{}
```\n",
                e
            ));
        }
    }
    md
}

fn tool_response_to_markdown_for_audience(resp: &ToolResponse, audience: Option<Role>) -> String {
    let mut md = String::new();
    md.push_str("#### Tool Response:\n");

    match &resp.tool_result {
        Ok(result) => {
            if result.content.is_empty() {
                md.push_str("*No textual output from tool.*\n");
            }

            for content in &result.content {
                if let Some(ref role) = audience {
                    let content_audience = match content {
                        ContentBlock::Text(t) => {
                            t.annotations.as_ref().and_then(|a| a.audience.as_ref())
                        }
                        ContentBlock::Image(i) => {
                            i.annotations.as_ref().and_then(|a| a.audience.as_ref())
                        }
                        ContentBlock::Audio(a) => {
                            a.annotations.as_ref().and_then(|a| a.audience.as_ref())
                        }
                        ContentBlock::Resource(r) => {
                            r.annotations.as_ref().and_then(|a| a.audience.as_ref())
                        }
                        ContentBlock::ResourceLink(r) => {
                            r.annotations.as_ref().and_then(|a| a.audience.as_ref())
                        }
                        _ => None,
                    };
                    if let Some(content_audience) = content_audience {
                        if !content_audience.contains(role) {
                            continue;
                        }
                    }
                }

                match content {
                    ContentBlock::Text(text_content) => {
                        let trimmed_text = text_content.text.trim();
                        if (trimmed_text.starts_with('{') && trimmed_text.ends_with('}'))
                            || (trimmed_text.starts_with('[') && trimmed_text.ends_with(']'))
                        {
                            md.push_str(&format!("```json\n{}\n```\n", trimmed_text));
                        } else if trimmed_text.starts_with('<')
                            && trimmed_text.ends_with('>')
                            && trimmed_text.contains("</")
                        {
                            md.push_str(&format!("```xml\n{}\n```\n", trimmed_text));
                        } else {
                            md.push_str(&text_content.text);
                            md.push_str("\n\n");
                        }
                    }
                    ContentBlock::Image(image_content) => {
                        if image_content.mime_type.starts_with("image/") {
                            // For actual images, provide a placeholder that indicates it's an image
                            md.push_str(&format!(
                                "**Image:** `(type: {}, data: first 30 chars of base64...)`\n\n",
                                image_content.mime_type
                            ));
                        } else {
                            // For non-image mime types, just indicate it's binary data
                            md.push_str(&format!(
                                "**Binary Content:** `(type: {}, length: {} bytes)`\n\n",
                                image_content.mime_type,
                                image_content.data.len()
                            ));
                        }
                    }
                    ContentBlock::Resource(resource) => {
                        match &resource.resource {
                            ResourceContents::TextResourceContents {
                                uri,
                                mime_type,
                                text,
                                meta: _,
                            } => {
                                // Extract file extension from the URI for syntax highlighting
                                let file_extension = uri.split('.').next_back().unwrap_or("");
                                let syntax_type = match file_extension {
                                    "rs" => "rust",
                                    "js" => "javascript",
                                    "ts" => "typescript",
                                    "py" => "python",
                                    "json" => "json",
                                    "yaml" | "yml" => "yaml",
                                    "md" => "markdown",
                                    "html" => "html",
                                    "css" => "css",
                                    "sh" => "bash",
                                    _ => mime_type
                                        .as_ref()
                                        .map(|mime| if mime == "text" { "" } else { mime })
                                        .unwrap_or(""),
                                };

                                md.push_str(&format!("**File:** `{}`\n", uri));
                                md.push_str(&format!(
                                    "```{}\n{}\n```\n\n",
                                    syntax_type,
                                    text.trim()
                                ));
                            }
                            ResourceContents::BlobResourceContents {
                                uri,
                                mime_type,
                                blob,
                                ..
                            } => {
                                md.push_str(&format!(
                                    "**Binary File:** `{}` (type: {}, {} bytes)\n\n",
                                    uri,
                                    mime_type.as_ref().map(|s| s.as_str()).unwrap_or("unknown"),
                                    blob.len()
                                ));
                            }
                            _ => {}
                        }
                    }
                    ContentBlock::ResourceLink(_link) => {
                        // Show a simple placeholder for resource links when exporting
                        md.push_str("[resource link]\n\n");
                    }
                    ContentBlock::Audio(_) => {
                        md.push_str("[audio content not displayed in Markdown export]\n\n")
                    }
                    _ => {}
                }
            }
        }
        Err(e) => {
            md.push_str(&format!(
                "**Error in Tool Response:**\n```\n{}
```\n",
                e
            ));
        }
    }
    md
}

pub fn message_to_markdown(message: &Message, export_all_content: bool) -> String {
    let audience = (!export_all_content).then_some(Role::Assistant);
    message_to_markdown_for_audience(message, export_all_content, audience)
}

pub fn user_projected_message_to_markdown(message: &Message) -> String {
    message_to_markdown_for_audience(message, false, Some(Role::User))
}

fn message_to_markdown_for_audience(
    message: &Message,
    export_all_content: bool,
    audience: Option<Role>,
) -> String {
    let mut md = String::new();
    for content in &message.content {
        match content {
            MessageContent::ActionRequired(action) => match &action.data {
                ActionRequiredData::ToolConfirmation { tool_name, .. } => {
                    md.push_str(&format!(
                        "**Action Required** (tool_confirmation): {}\n\n",
                        tool_name
                    ));
                }
                ActionRequiredData::Elicitation { message, .. } => {
                    md.push_str(&format!(
                        "**Action Required** (elicitation): {}\n\n",
                        message
                    ));
                }
                ActionRequiredData::ElicitationResponse { id, user_data, .. } => {
                    md.push_str(&format!(
                        "**Action Required** (elicitation_response): {}\n```json\n{}\n```\n\n",
                        id,
                        serde_json::to_string_pretty(user_data)
                            .unwrap_or_else(|_| "{}".to_string())
                    ));
                }
                ActionRequiredData::ToolConfirmationResponse { id, permission } => {
                    md.push_str(&format!(
                        "**Action Required** (tool_confirmation_response): {} {:?}\n\n",
                        id, permission
                    ));
                }
            },
            MessageContent::Text(text) => {
                md.push_str(&text.text);
                md.push_str("\n\n");
            }
            MessageContent::ToolRequest(req) => {
                md.push_str(&tool_request_to_markdown(req, export_all_content));
                md.push('\n');
            }
            MessageContent::ToolResponse(resp) => {
                md.push_str(&tool_response_to_markdown_for_audience(
                    resp,
                    audience.clone(),
                ));
                md.push('\n');
            }
            MessageContent::Image(image) => {
                md.push_str(&format!(
                    "**Image:** `(type: {}, data placeholder: {}...)`\n\n",
                    image.mime_type,
                    image.data.chars().take(30).collect::<String>()
                ));
            }
            MessageContent::Thinking(thinking) => {
                md.push_str("**Thinking:**\n");
                md.push_str("> ");
                md.push_str(&thinking.thinking.replace("\n", "\n> "));
                md.push_str("\n\n");
            }
            MessageContent::RedactedThinking(_) => {
                md.push_str("**Thinking:**\n");
                md.push_str("> *Thinking was redacted*\n\n");
            }
            MessageContent::Error(error) => {
                md.push_str(&format!("**Error**: {}\n\n", error.message));
            }
            MessageContent::SystemNotification(notification) => {
                md.push_str(&format!("*{}*\n\n", notification.msg));
            }
            _ => {
                md.push_str(
                    "`WARNING: Message content type could not be rendered to Markdown`\n\n",
                );
            }
        }
    }
    md.trim_end_matches("\n").to_string()
}

pub fn export_session_to_markdown(messages: Vec<Message>, session_name: &str) -> String {
    let mut markdown_output = String::new();

    markdown_output.push_str(&format!("# Session Export: {}\n\n", session_name));

    if messages.is_empty() {
        markdown_output.push_str("*(This session has no messages)*\n");
        return markdown_output;
    }

    markdown_output.push_str(&format!("*Total messages: {}*\n\n---\n\n", messages.len()));

    let mut skip_next_if_tool_response = false;

    for message in &messages {
        let is_only_tool_response = message.role == Role::User
            && message
                .content
                .iter()
                .all(|content| matches!(content, MessageContent::ToolResponse(_)));

        if skip_next_if_tool_response && is_only_tool_response {
            markdown_output.push_str(&user_projected_message_to_markdown(message));
            markdown_output.push_str("\n\n---\n\n");
            skip_next_if_tool_response = false;
            continue;
        }

        skip_next_if_tool_response = false;

        if !is_only_tool_response {
            let role_prefix = match message.role {
                Role::User => "### User:\n",
                Role::Assistant => "### Assistant:\n",
            };
            markdown_output.push_str(role_prefix);
        }

        markdown_output.push_str(&user_projected_message_to_markdown(message));
        markdown_output.push_str("\n\n---\n\n");

        if message
            .content
            .iter()
            .any(|content| matches!(content, MessageContent::ToolRequest(_)))
        {
            skip_next_if_tool_response = true;
        }
    }

    markdown_output
}

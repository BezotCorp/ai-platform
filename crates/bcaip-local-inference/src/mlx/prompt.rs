use super::gemma4::{gemma4_messages, gemma4_messages_with_system, is_gemma4};
use super::{mlx_error::mlx_error, tool_mode::ToolMode};
use crate::extract_text_content;
use crate::tool_emulation::{build_emulator_tool_description, load_tiny_model_prompt};
use goose_provider_types::formats::{format_messages, format_tools};
use goose_provider_types::images::ImageFormat;
use goose_provider_types::{conversations::Message, errors::ProviderError};
use safemlx_lm::models::LoadedModel;
use safemlx_lm_utils::tokenizer::{Chat, Conversation, Role};
use serde_json::json;
pub(crate) fn build_prompt(
    model: &mut LoadedModel,
    model_name: &str,
    system: &str,
    messages: &[Message],
    tools: &[rmcp::model::Tool],
    tool_mode: ToolMode,
) -> Result<String, ProviderError> {
    match tool_mode {
        ToolMode::Native => {
            let conversations = openai_messages(system, messages);
            let tool_specs =
                format_tools(tools).map_err(|e| ProviderError::ExecutionError(e.to_string()))?;
            if let Some(prompt) = model
                .apply_chat_template_json([conversations], Some(&tool_specs), true)
                .map_err(mlx_error)?
            {
                return Ok(prompt);
            }

            Ok(render_prompt(system, messages))
        }
        ToolMode::Emulated { code_mode_enabled } => {
            let system_prompt = format!(
                "{}{}",
                load_tiny_model_prompt(),
                build_emulator_tool_description(tools, code_mode_enabled)
            );
            if is_gemma4(model) {
                let conversations = gemma4_messages_with_system(&system_prompt, messages);
                if let Some(prompt) = model
                    .apply_chat_template_json([conversations], None, true)
                    .map_err(mlx_error)?
                {
                    return Ok(prompt);
                }
            }

            let conversations = chat_conversations(&system_prompt, messages);
            if let Some(prompt) = model
                .apply_chat_template([Chat::Owned(conversations)], None, true)
                .map_err(mlx_error)?
            {
                return Ok(prompt);
            }

            Ok(render_prompt(&system_prompt, messages))
        }
        ToolMode::None => {
            if is_gemma4(model) {
                let conversations = gemma4_messages(model_name, system, messages);
                if let Some(prompt) = model
                    .apply_chat_template_json([conversations], None, true)
                    .map_err(mlx_error)?
                {
                    return Ok(prompt);
                }
            }

            let conversations = chat_conversations(system, messages);
            if let Some(prompt) = model
                .apply_chat_template([Chat::Owned(conversations)], None, true)
                .map_err(mlx_error)?
            {
                return Ok(prompt);
            }

            Ok(render_prompt(system, messages))
        }
    }
}
pub(crate) fn openai_messages(system: &str, messages: &[Message]) -> Vec<serde_json::Value> {
    let mut values = vec![serde_json::json!({
        "role": "system",
        "content": system,
    })];
    values.extend(format_messages(messages, &ImageFormat::OpenAi));
    values
}
pub(crate) fn chat_conversations(
    system: &str,
    messages: &[Message],
) -> Vec<Conversation<Role, String>> {
    let mut conversations = Vec::new();
    if !system.trim().is_empty() {
        conversations.push(Conversation {
            role: Role::System,
            content: system.trim().to_string(),
        });
    }
    for message in messages.iter().filter(|message| message.is_agent_visible()) {
        let role = match message.role {
            rmcp::model::Role::User => Role::User,
            rmcp::model::Role::Assistant => Role::Assistant,
        };
        let text = extract_text_content(message);
        if !text.trim().is_empty() {
            conversations.push(Conversation {
                role,
                content: text.trim().to_string(),
            });
        }
    }
    conversations
}
pub(crate) fn render_prompt(system: &str, messages: &[Message]) -> String {
    let mut prompt = String::new();
    if !system.trim().is_empty() {
        prompt.push_str("System: ");
        prompt.push_str(system.trim());
        prompt.push('\n');
    }
    for message in messages.iter().filter(|message| message.is_agent_visible()) {
        let role = match message.role {
            rmcp::model::Role::User => "User",
            rmcp::model::Role::Assistant => "Assistant",
        };
        let text = extract_text_content(message);
        if !text.trim().is_empty() {
            prompt.push_str(role);
            prompt.push_str(": ");
            prompt.push_str(text.trim());
            prompt.push('\n');
        }
    }
    prompt.push_str("Assistant: ");
    prompt
}

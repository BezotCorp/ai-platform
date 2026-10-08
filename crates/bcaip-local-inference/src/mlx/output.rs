use crate::mlx::tool_mode::ToolMode;
use crate::tool_emulation::{StreamingEmulatorParser, message_for_emulator_action};
use crate::{
    native_tool_parsing::message_from_native_tool_text, thinking_output::ThinkingOutputFilter,
};
use bcaip_provider_types::conversations::ProviderUsage;
use bcaip_provider_types::conversations::{Message, MessageContent};
use bcaip_provider_types::errors::ProviderError;

pub(crate) fn emit_generated_response(
    generated_text: &str,
    generation_prompt: &str,
    enable_thinking: bool,
    message_id: &str,
    tool_mode: ToolMode,
    tx: &tokio::sync::mpsc::Sender<Result<(Option<Message>, Option<ProviderUsage>), ProviderError>>,
) -> Result<(), ProviderError> {
    if generated_text.is_empty() {
        return Ok(());
    }

    let (content, thinking) =
        split_generated_thinking(generated_text, generation_prompt, enable_thinking);

    match tool_mode {
        ToolMode::None => {
            emit_assistant_message(message_id, &thinking, &content, tx)?;
        }
        ToolMode::Native => {
            if let Some(mut message) = message_from_native_tool_text(&content, message_id)? {
                prepend_thinking(&mut message, &thinking);
                tx.blocking_send(Ok((Some(message), None))).map_err(|_| {
                    ProviderError::ExecutionError("Failed to stream MLX response".to_string())
                })?;
            } else {
                emit_assistant_message(message_id, &thinking, &content, tx)?;
            }
        }
        ToolMode::Emulated { code_mode_enabled } => {
            emit_assistant_message(message_id, &thinking, "", tx)?;
            let mut parser = StreamingEmulatorParser::new(code_mode_enabled);
            let mut actions = parser.process_chunk(&content);
            actions.extend(parser.flush());

            for action in actions {
                let (message, _) = message_for_emulator_action(&action, message_id);
                tx.blocking_send(Ok((Some(message), None))).map_err(|_| {
                    ProviderError::ExecutionError("Failed to stream MLX response".to_string())
                })?;
            }
        }
    }
    Ok(())
}
pub(crate) fn split_generated_thinking(
    generated_text: &str,
    generation_prompt: &str,
    enable_thinking: bool,
) -> (String, String) {
    let mut filter = ThinkingOutputFilter::new(enable_thinking, generation_prompt);
    let mut filtered = filter.push_text(generated_text);
    let final_filtered = filter.finish();
    filtered.content.push_str(&final_filtered.content);
    filtered.thinking.push_str(&final_filtered.thinking);
    (filtered.content, filtered.thinking)
}
pub(crate) fn emit_assistant_message(
    message_id: &str,
    thinking: &str,
    content: &str,
    tx: &tokio::sync::mpsc::Sender<Result<(Option<Message>, Option<ProviderUsage>), ProviderError>>,
) -> Result<(), ProviderError> {
    if thinking.is_empty() && content.is_empty() {
        return Ok(());
    }

    let mut message = Message::assistant();
    if !thinking.is_empty() {
        message = message.with_thinking(thinking, "");
    }
    if !content.is_empty() {
        message = message.with_text(content);
    }
    message.id = Some(message_id.to_string());
    tx.blocking_send(Ok((Some(message), None)))
        .map_err(|_| ProviderError::ExecutionError("Failed to stream MLX response".to_string()))
}
pub(crate) fn prepend_thinking(message: &mut Message, thinking: &str) {
    if !thinking.is_empty() {
        message
            .content
            .insert(0, MessageContent::thinking(thinking, ""));
    }
}

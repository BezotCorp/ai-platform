use crate::mlx::tool_mode::ToolMode;
use crate::thinking_output::ThinkingOutputFilter;
use crate::tool_emulation::{StreamingEmulatorParser, message_for_emulator_action};
use bcaip_provider_types::{
    conversations::{Message, ProviderUsage},
    errors::ProviderError,
};
pub(crate) struct MlxStreamEmitter<'a> {
    message_id: &'a str,
    tool_mode: ToolMode,
    tx: &'a tokio::sync::mpsc::Sender<
        Result<(Option<Message>, Option<ProviderUsage>), ProviderError>,
    >,
    output_filter: ThinkingOutputFilter,
    emulator_parser: Option<StreamingEmulatorParser>,
    stop_after_tool_call: bool,
}

impl<'a> MlxStreamEmitter<'a> {
    pub(crate) fn new(
        message_id: &'a str,
        tool_mode: ToolMode,
        enable_thinking: bool,
        generation_prompt: &str,
        tx: &'a tokio::sync::mpsc::Sender<
            Result<(Option<Message>, Option<ProviderUsage>), ProviderError>,
        >,
    ) -> Self {
        let emulator_parser = match tool_mode {
            ToolMode::Emulated { code_mode_enabled } => {
                Some(StreamingEmulatorParser::new(code_mode_enabled))
            }
            ToolMode::None | ToolMode::Native => None,
        };
        Self {
            message_id,
            tool_mode,
            tx,
            output_filter: ThinkingOutputFilter::new(enable_thinking, generation_prompt),
            emulator_parser,
            stop_after_tool_call: false,
        }
    }

    pub(crate) fn can_stream(&self) -> bool {
        !matches!(self.tool_mode, ToolMode::Native)
    }

    pub(crate) fn push_text(&mut self, text: &str) -> Result<bool, ProviderError> {
        let filtered = self.output_filter.push_text(text);
        if !filtered.content.is_empty() {
            self.emit_content(&filtered.content)?;
        }
        Ok(!self.stop_after_tool_call)
    }

    pub(crate) fn finish(&mut self) -> Result<(), ProviderError> {
        self.flush_filtered_output()?;
        let actions = self
            .emulator_parser
            .as_mut()
            .map(StreamingEmulatorParser::flush)
            .unwrap_or_default();
        for action in actions {
            let (message, is_tool) = message_for_emulator_action(&action, self.message_id);
            if is_tool {
                self.flush_filtered_output()?;
            }
            self.send(message)?;
            self.stop_after_tool_call |= is_tool;
            if is_tool {
                break;
            }
        }
        Ok(())
    }

    fn flush_filtered_output(&mut self) -> Result<(), ProviderError> {
        let filtered = self.output_filter.finish();
        if !filtered.thinking.is_empty() {
            let mut message = Message::assistant().with_thinking(filtered.thinking, "");
            message.id = Some(self.message_id.to_string());
            self.send(message)?;
        }
        if !filtered.content.is_empty() {
            self.emit_content(&filtered.content)?;
        }
        Ok(())
    }

    fn emit_content(&mut self, content: &str) -> Result<(), ProviderError> {
        match self.tool_mode {
            ToolMode::None => {
                let mut message = Message::assistant().with_text(content);
                message.id = Some(self.message_id.to_string());
                self.send(message)
            }
            ToolMode::Emulated { .. } => {
                let actions = self
                    .emulator_parser
                    .as_mut()
                    .map(|parser| parser.process_chunk(content))
                    .unwrap_or_default();
                for action in actions {
                    let (message, is_tool) = message_for_emulator_action(&action, self.message_id);
                    if is_tool {
                        self.flush_filtered_output()?;
                    }
                    self.send(message)?;
                    self.stop_after_tool_call |= is_tool;
                    if is_tool {
                        break;
                    }
                }
                Ok(())
            }
            ToolMode::Native => Ok(()),
        }
    }

    fn send(&self, message: Message) -> Result<(), ProviderError> {
        self.tx
            .blocking_send(Ok((Some(message), None)))
            .map_err(|_| ProviderError::ExecutionError("Failed to stream MLX response".to_string()))
    }
}

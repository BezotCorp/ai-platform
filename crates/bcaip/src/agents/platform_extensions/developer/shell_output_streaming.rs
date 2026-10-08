use rmcp::model::{CustomNotification, ServerNotification};
use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::agents::tool_execution::ToolCallNotificationEmitter;
pub(super) const SHELL_LIVE_OUTPUT_FLUSH_INTERVAL: Duration = Duration::from_millis(150);
const SHELL_LIVE_OUTPUT_BATCH_BYTES: usize = 16 * 1024;
const SHELL_LIVE_OUTPUT_LIMIT_BYTES: usize = 256 * 1024;

pub const DEVELOPER_SHELL_OUTPUT_NOTIFICATION_METHOD: &str = "bcaip/developer_shell_output";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ShellOutputStream {
    Stdout,
    Stderr,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ShellOutputNotificationChunk {
    pub stream: ShellOutputStream,
    pub output: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct ShellOutputNotificationParams {
    pub sequence: u64,
    pub chunks: Vec<ShellOutputNotificationChunk>,
    pub truncated: bool,
}

pub fn parse_shell_output_notification(
    notification: &CustomNotification,
) -> Option<ShellOutputNotificationParams> {
    if notification.method != DEVELOPER_SHELL_OUTPUT_NOTIFICATION_METHOD {
        return None;
    }
    serde_json::from_value(notification.params.clone()?).ok()
}

pub(super) struct ShellOutputBatcher {
    emitter: ToolCallNotificationEmitter,
    sequence: u64,
    chunks: Vec<ShellOutputNotificationChunk>,
    buffered_bytes: usize,
    live_output_bytes: usize,
    emitted_first_line: bool,
    truncated: bool,
}

impl ShellOutputBatcher {
    pub(super) fn new(emitter: ToolCallNotificationEmitter) -> Self {
        Self {
            emitter,
            sequence: 0,
            chunks: Vec::new(),
            buffered_bytes: 0,
            live_output_bytes: 0,
            emitted_first_line: false,
            truncated: false,
        }
    }

    pub(super) fn push_line(&mut self, is_stderr: bool, line: &str) -> bool {
        if self.truncated {
            return false;
        }

        let line = line.strip_suffix('\r').unwrap_or(line);
        let output_bytes = line.len() + 1;
        if self.live_output_bytes + output_bytes > SHELL_LIVE_OUTPUT_LIMIT_BYTES {
            self.flush();
            self.truncated = true;
            self.emit_notification(Vec::new(), true);
            return true;
        }

        self.live_output_bytes += output_bytes;
        self.buffered_bytes += output_bytes;

        let stream = if is_stderr {
            ShellOutputStream::Stderr
        } else {
            ShellOutputStream::Stdout
        };
        if let Some(chunk) = self
            .chunks
            .last_mut()
            .filter(|chunk| chunk.stream == stream)
        {
            chunk.output.push_str(line);
            chunk.output.push('\n');
        } else {
            self.chunks.push(ShellOutputNotificationChunk {
                stream,
                output: format!("{line}\n"),
            });
        }

        if !self.emitted_first_line || self.buffered_bytes >= SHELL_LIVE_OUTPUT_BATCH_BYTES {
            self.emitted_first_line = true;
            return self.flush();
        }

        false
    }

    pub(super) fn flush(&mut self) -> bool {
        if self.chunks.is_empty() {
            return false;
        }

        self.buffered_bytes = 0;
        let chunks = std::mem::take(&mut self.chunks);
        self.emit_notification(chunks, false);
        true
    }

    fn emit_notification(&mut self, chunks: Vec<ShellOutputNotificationChunk>, truncated: bool) {
        self.sequence += 1;
        let params = ShellOutputNotificationParams {
            sequence: self.sequence,
            chunks,
            truncated,
        };
        if let Ok(params) = serde_json::to_value(params) {
            self.emitter
                .emit_best_effort(ServerNotification::CustomNotification(
                    CustomNotification::new(
                        DEVELOPER_SHELL_OUTPUT_NOTIFICATION_METHOD,
                        Some(params),
                    ),
                ));
        }
    }
}

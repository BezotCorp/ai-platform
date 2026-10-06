use crate::{native_tool_format::NativeToolFormat, native_tool_stream_end::NativeToolStreamEnd};
/// Streams assistant text while holding back everything from the first tool call marker onwards,
/// so the UI never shows raw tool call syntax.
pub(crate) struct NativeToolStreamSplitter {
    format: NativeToolFormat,
    pending: String,
    tool_text: String,
    in_tool_call: bool,
    emitted_content: bool,
}

impl NativeToolStreamSplitter {
    pub(crate) fn new(format: NativeToolFormat) -> Self {
        Self {
            format,
            pending: String::new(),
            tool_text: String::new(),
            in_tool_call: false,
            emitted_content: false,
        }
    }

    pub(crate) fn push(&mut self, chunk: &str) -> String {
        if self.in_tool_call {
            self.tool_text.push_str(chunk);
            return String::new();
        }

        self.pending.push_str(chunk);

        if self.format.reply_may_be_bare_json() && !self.emitted_content {
            let trimmed = self.pending.trim_start();
            if trimmed.is_empty() {
                return String::new();
            }
            if trimmed.starts_with(['{', '[']) {
                let tool_text = std::mem::take(&mut self.pending);
                self.begin_tool_call(tool_text);
                return String::new();
            }
        }

        let marker_start = self
            .format
            .start_markers()
            .iter()
            .filter_map(|marker| self.pending.find(marker))
            .min();
        if let Some(start) = marker_start {
            let tool_text = self.pending.split_off(start);
            let content = std::mem::take(&mut self.pending);
            self.begin_tool_call(tool_text);
            return self.mark_emitted(content);
        }

        let hold_len = self.partial_marker_suffix_len();
        let emit_len = self.pending.len() - hold_len;
        let held = self.pending.split_off(emit_len);
        let content = std::mem::replace(&mut self.pending, held);
        self.mark_emitted(content)
    }

    pub(crate) fn finish(&mut self) -> NativeToolStreamEnd {
        if self.in_tool_call {
            return NativeToolStreamEnd {
                content: String::new(),
                tool_text: Some(std::mem::take(&mut self.tool_text)),
            };
        }
        NativeToolStreamEnd {
            content: std::mem::take(&mut self.pending),
            tool_text: None,
        }
    }

    fn begin_tool_call(&mut self, tool_text: String) {
        self.tool_text = tool_text;
        self.in_tool_call = true;
    }

    fn mark_emitted(&mut self, content: String) -> String {
        if !content.trim().is_empty() {
            self.emitted_content = true;
        }
        content
    }

    fn partial_marker_suffix_len(&self) -> usize {
        self.pending
            .char_indices()
            .map(|(idx, _)| idx)
            .filter(|idx| {
                self.pending.get(*idx..).is_some_and(|suffix| {
                    self.format
                        .start_markers()
                        .iter()
                        .any(|marker| marker.len() > suffix.len() && marker.starts_with(suffix))
                })
            })
            .map(|idx| self.pending.len() - idx)
            .max()
            .unwrap_or(0)
    }
}

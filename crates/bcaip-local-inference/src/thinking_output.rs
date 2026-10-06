use goose_provider_types::thinking::{FilterOut, ThinkFilter};
use std::mem;
pub(crate) struct ThinkingOutputFilter {
    enabled: bool,
    think_filter: ThinkFilter,
    pending_inline_thinking: String,
    accumulated_thinking: String,
}

impl ThinkingOutputFilter {
    pub(crate) fn new(enable_thinking: bool, generation_prompt: &str) -> Self {
        let mut think_filter = ThinkFilter::new();
        if enable_thinking && !generation_prompt.is_empty() {
            let _ = think_filter.push(generation_prompt);
        }

        Self {
            enabled: enable_thinking,
            think_filter,
            pending_inline_thinking: String::new(),
            accumulated_thinking: String::new(),
        }
    }

    pub(crate) fn push_text(&mut self, text: &str) -> FilterOut {
        if !self.enabled {
            return FilterOut {
                content: text.to_string(),
                thinking: String::new(),
            };
        }

        let mut filtered = self.think_filter.push(text);
        if !filtered.thinking.is_empty() {
            self.pending_inline_thinking.push_str(&filtered.thinking);
            filtered.thinking.clear();
        }
        filtered
    }

    /// Drains inline thinking seen so far so callers can stream it live; it still counts towards
    /// `accumulated_thinking`.
    pub(crate) fn take_streamed_thinking(&mut self) -> Option<String> {
        if self.pending_inline_thinking.is_empty() {
            return None;
        }
        let thinking = mem::take(&mut self.pending_inline_thinking);
        self.accumulated_thinking.push_str(&thinking);
        Some(thinking)
    }

    pub(crate) fn finish(&mut self) -> FilterOut {
        let mut filtered = if self.enabled {
            mem::take(&mut self.think_filter).finish()
        } else {
            FilterOut::default()
        };

        let mut thinking = mem::take(&mut self.pending_inline_thinking);
        thinking.push_str(&filtered.thinking);
        if !thinking.is_empty() {
            self.accumulated_thinking.push_str(&thinking);
        }
        filtered.thinking = thinking;

        filtered
    }

    pub(crate) fn accumulated_thinking(&self) -> &str {
        &self.accumulated_thinking
    }
}

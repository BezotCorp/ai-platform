pub(crate) const TOOL_CALL_OPEN: &str = "<tool_call>";
pub(crate) const TOOL_CALL_CLOSE: &str = "</tool_call>";
pub(crate) const FUNCTION_OPEN: &str = "<function=";
pub(crate) const MISTRAL_TOOL_CALLS: &str = "[TOOL_CALLS]";
pub(crate) const LLAMA3_PYTHON_TAG: &str = "<|python_tag|>";
pub(crate) const DEEPSEEK_CALLS_BEGIN: &str = "<｜tool▁calls▁begin｜>";
pub(crate) const DEEPSEEK_CALL_BEGIN: &str = "<｜tool▁call▁begin｜>";

/// How a model's chat template asks it to emit tool calls, inferred from the template source.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NativeToolFormat {
    /// `<tool_call>{json}</tool_call>` or `<tool_call><function=..>` (Hermes, Qwen).
    ToolCallTag,
    /// `[TOOL_CALLS][{json}, ...]` (Mistral).
    MistralToolCalls,
    /// `<|python_tag|>{json}` or a bare `{"name": .., "parameters": ..}` reply (Llama 3.x).
    Llama3Json,
    /// DeepSeek R1/V3 tool call special tokens.
    DeepSeek,
    /// Template mentions tools but matches no known format; every known shape is accepted.
    Generic,
}

impl NativeToolFormat {
    pub(crate) fn detect(template_source: &str) -> Option<Self> {
        if template_source.contains(TOOL_CALL_OPEN) || template_source.contains(FUNCTION_OPEN) {
            Some(Self::ToolCallTag)
        } else if template_source.contains(MISTRAL_TOOL_CALLS) {
            Some(Self::MistralToolCalls)
        } else if template_source.contains(LLAMA3_PYTHON_TAG) {
            Some(Self::Llama3Json)
        } else if template_source.contains(DEEPSEEK_CALLS_BEGIN) {
            Some(Self::DeepSeek)
        } else {
            None
        }
    }

    pub(crate) fn start_markers(self) -> &'static [&'static str] {
        match self {
            Self::ToolCallTag => &[TOOL_CALL_OPEN, FUNCTION_OPEN],
            Self::MistralToolCalls => &[MISTRAL_TOOL_CALLS],
            Self::Llama3Json => &[LLAMA3_PYTHON_TAG],
            Self::DeepSeek => &[DEEPSEEK_CALLS_BEGIN, DEEPSEEK_CALL_BEGIN],
            Self::Generic => &[
                TOOL_CALL_OPEN,
                FUNCTION_OPEN,
                MISTRAL_TOOL_CALLS,
                LLAMA3_PYTHON_TAG,
                DEEPSEEK_CALLS_BEGIN,
                DEEPSEEK_CALL_BEGIN,
            ],
        }
    }

    pub(crate) fn reply_may_be_bare_json(self) -> bool {
        matches!(self, Self::Llama3Json | Self::Generic)
    }
}

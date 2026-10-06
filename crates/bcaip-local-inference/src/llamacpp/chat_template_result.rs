use crate::native_tool_format::NativeToolFormat;
pub(crate) struct ChatTemplateResult {
    pub prompt: String,
    /// Suffix the template appends when `add_generation_prompt` is set.
    pub generation_prompt: String,
    pub additional_stops: Vec<String>,
    /// Present only when tools were supplied.
    pub tool_format: Option<NativeToolFormat>,
}

impl ChatTemplateResult {
    pub(crate) fn supports_native_tool_calling(&self) -> bool {
        matches!(self.tool_format, Some(format) if format != NativeToolFormat::Generic)
    }
}

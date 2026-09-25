#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ContextBudget {
    pub capacity_tokens: usize,
    pub system_tokens: usize,
    pub output_tokens: usize,
    pub tool_tokens: usize,
}

impl ContextBudget {
    /// Returns the remaining estimated context budget.
    ///
    /// Actual token counting belongs to the model provider.
    pub fn available_tokens(self) -> Option<usize> {
        let reserved = self
            .system_tokens
            .checked_add(self.output_tokens)?
            .checked_add(self.tool_tokens)?;

        self.capacity_tokens.checked_sub(reserved)
    }
}

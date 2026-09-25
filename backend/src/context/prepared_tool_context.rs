use serde_json::Value;

pub(crate) struct PreparedToolContext {
    pub messages: Vec<Value>,
    pub compacted_rounds: Vec<usize>,
    pub omitted_history_indices: Vec<usize>,
    pub latest_round_compacted: bool,
}

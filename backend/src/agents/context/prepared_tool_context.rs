use serde_json::Value;

pub(crate) struct PreparedToolContext {
    pub messages: Vec<Value>,
    pub compacted_rounds: Vec<usize>,
    pub omitted_history_indices: Vec<usize>,
    pub omitted_memory_ids: Vec<i64>,
    pub latest_round_compacted: bool,
}

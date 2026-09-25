use serde_json::Value;

pub(crate) struct ChatTurn {
    pub content: String,
    pub tool_calls: Vec<Value>,
}

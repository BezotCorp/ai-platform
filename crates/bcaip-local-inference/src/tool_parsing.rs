use rmcp::model::Tool;
use serde_json::{Value, json};

pub(crate) fn compact_tools_json(tools: &[Tool]) -> Option<String> {
    let compact: Vec<Value> = tools
        .iter()
        .map(|t| {
            json!({
                "type": "function",
                "function": {
                    "name": t.name,
                    "description": t.description.as_ref().map(|d| d.as_ref()).unwrap_or(""),
                }
            })
        })
        .collect();
    serde_json::to_string(&compact).ok()
}

use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Serialize)]
pub(crate) struct Event {
    #[serde(rename = "type")]
    pub kind: String,
    pub request_id: String,
    pub data: Value,
}

impl Event {
    pub(crate) fn new(kind: &str, request_id: &str, data: Value) -> Self {
        Self {
            kind: kind.to_owned(),
            request_id: request_id.to_owned(),
            data,
        }
    }
}

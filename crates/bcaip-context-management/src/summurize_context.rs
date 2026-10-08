use serde::Serialize;

#[derive(Serialize)]
pub struct SummarizeContext {
    pub messages: String,
}

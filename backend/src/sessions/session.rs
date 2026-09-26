use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Session {
    pub(crate) id: String,
    pub(crate) revision: i64,
    pub(crate) created_at: i64,
    pub(crate) updated_at: i64,
}

use serde::Serialize;

use crate::agents::ExecutionMode;

#[derive(Debug, Serialize)]
pub(crate) struct SavedConfiguration {
    pub id: String,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
    pub mode: ExecutionMode,
}

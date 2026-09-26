use rusqlite::{Result, Row};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct ConfigurationSummary {
    pub id: String,
    pub revision: i64,
    pub created_at: i64,
    pub updated_at: i64,
}

impl ConfigurationSummary {
    pub(crate) fn from_row(row: &Row<'_>) -> Result<Self> {
        Ok(Self {
            id: row.get(0)?,
            revision: row.get(1)?,
            created_at: row.get(2)?,
            updated_at: row.get(3)?,
        })
    }
}

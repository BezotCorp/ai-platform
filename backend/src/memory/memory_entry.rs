#[derive(Clone)]
pub(crate) struct MemoryEntry {
    pub id: i64,
    pub project: String,
    pub query: String,
    pub content: String,
    pub source: String,
    pub checksum: String,
    pub revision: i64,
}

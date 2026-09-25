use crate::agents::{checksum, relevance};

#[derive(Clone)]
pub(crate) struct MemoryEntry {
    id: i64,
    project: String,
    query: String,
    content: String,
    source: String,
    checksum: String,
    revision: i64,
}

impl MemoryEntry {
    pub(crate) fn from_storage(
        id: i64,
        project: String,
        query: String,
        content: String,
        source: String,
        checksum: String,
        revision: i64,
    ) -> Self {
        Self {
            id,
            project,
            query,
            content,
            source,
            checksum,
            revision,
        }
    }

    pub(crate) fn checksum_for(query: &str, content: &str) -> String {
        checksum(&format!("{query}\\n{content}"))
    }

    pub(crate) fn verify_checksum(&self) -> bool {
        Self::checksum_for(&self.query, &self.content) == self.checksum
    }

    pub(crate) fn relevance_score(&self, request: &str, role: &str) -> usize {
        relevance(request, &self.query)
            .saturating_mul(4)
            .saturating_add(relevance(request, &self.content).saturating_mul(2))
            .saturating_add(relevance(role, &self.content))
    }

    pub(crate) fn matches_request(&self, request: &str) -> bool {
        relevance(request, &self.query) > 0 || relevance(request, &self.content) > 0
    }

    pub(crate) fn id(&self) -> i64 {
        self.id
    }

    pub(crate) fn project(&self) -> &str {
        &self.project
    }

    pub(crate) fn query(&self) -> &str {
        &self.query
    }

    pub(crate) fn content(&self) -> &str {
        &self.content
    }

    pub(crate) fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn checksum(&self) -> &str {
        &self.checksum
    }

    pub(crate) fn revision(&self) -> i64 {
        self.revision
    }
}

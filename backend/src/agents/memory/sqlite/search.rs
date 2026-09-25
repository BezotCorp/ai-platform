use anyhow::Result;
use rusqlite::{Connection, params};

use crate::agents::MemoryEntry;

pub(crate) fn find(
    connection: &Connection,
    project: &str,
    request: &str,
    role: &str,
    limit: usize,
) -> Result<Vec<MemoryEntry>> {
    const QUERY: &str = "SELECT id, project, query, content, source, checksum, revision
         FROM memory_entries WHERE project = ?1
         ORDER BY updated_at DESC LIMIT 256";
    let mut statement = connection.prepare(QUERY)?;
    let rows = statement.query_map(params![project], |row| {
        Ok(MemoryEntry::from_storage(
            row.get(0)?,
            row.get(1)?,
            row.get(2)?,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ))
    })?;
    let mut ranked = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    ranked.sort_by(|left, right| {
        right
            .relevance_score(request, role)
            .cmp(&left.relevance_score(request, role))
            .then_with(|| right.id().cmp(&left.id()))
    });
    ranked.retain(|entry| entry.matches_request(request));
    ranked.truncate(limit);
    Ok(ranked)
}

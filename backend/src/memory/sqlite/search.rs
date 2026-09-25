use anyhow::Result;
use rusqlite::{Connection, params};

use crate::{context::relevance, memory::MemoryEntry};

pub(crate) fn find(
    connection: &Connection,
    project: &str,
    request: &str,
    role: &str,
    limit: usize,
) -> Result<Vec<MemoryEntry>> {
    let mut statement = connection.prepare(
        "SELECT id, project, query, content, source, checksum, revision
         FROM memory_entries WHERE project = ?1
         ORDER BY updated_at DESC LIMIT 256",
    )?;
    let rows = statement.query_map(params![project], |row| {
        Ok(MemoryEntry {
            id: row.get(0)?,
            project: row.get(1)?,
            query: row.get(2)?,
            content: row.get(3)?,
            source: row.get(4)?,
            checksum: row.get(5)?,
            revision: row.get(6)?,
        })
    })?;

    let mut ranked = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    ranked.sort_by(|left, right| {
        let left_score = relevance(request, &left.query)
            .saturating_mul(4)
            .saturating_add(relevance(request, &left.content).saturating_mul(2))
            .saturating_add(relevance(role, &left.content));
        let right_score = relevance(request, &right.query)
            .saturating_mul(4)
            .saturating_add(relevance(request, &right.content).saturating_mul(2))
            .saturating_add(relevance(role, &right.content));
        right_score.cmp(&left_score).then_with(|| right.id.cmp(&left.id))
    });
    ranked.retain(|entry| {
        relevance(request, &entry.query) > 0
            || relevance(request, &entry.content) > 0
    });
    ranked.truncate(limit);
    Ok(ranked)
}

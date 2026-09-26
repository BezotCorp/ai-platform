use anyhow::{Context, Result};
use rusqlite::{Connection, params};

pub(crate) fn save(
    connection: &Connection,
    project: &str,
    query: &str,
    content: &str,
    checksum: &str,
) -> Result<()> {
    connection
        .execute(
            "INSERT INTO memory_entries (
                project,
                query,
                content,
                source,
                checksum
            )
            VALUES (
                ?1,
                ?2,
                ?3,
                'agent_final_answer',
                ?4
            )
            ON CONFLICT(project, checksum)
            DO UPDATE SET
                updated_at = unixepoch(),
                revision = revision + 1",
            params![project, query, content, checksum],
        )
        .context("Écriture de la mémoire SQLite impossible")?;
    Ok(())
}

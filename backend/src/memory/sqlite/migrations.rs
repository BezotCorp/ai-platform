use anyhow::Result;
use rusqlite::Connection;

pub(crate) fn apply(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS schema_version (
             version INTEGER PRIMARY KEY
         );
         CREATE TABLE IF NOT EXISTS memory_entries (
             id INTEGER PRIMARY KEY,
             project TEXT NOT NULL,
             query TEXT NOT NULL,
             content TEXT NOT NULL,
             source TEXT NOT NULL,
             checksum TEXT NOT NULL,
             revision INTEGER NOT NULL DEFAULT 1,
             created_at INTEGER NOT NULL DEFAULT (unixepoch()),
             updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
             UNIQUE(project, checksum)
         );
         CREATE INDEX IF NOT EXISTS memory_entries_project_updated
             ON memory_entries(project, updated_at DESC);
         INSERT OR IGNORE INTO schema_version(version) VALUES (1);",
    )?;

    let latest: Option<i64> = connection.query_row(
        "SELECT MAX(version) FROM schema_version",
        [],
        |row| row.get(0),
    )?;
    if latest.is_some_and(|version| version > 1) {
        anyhow::bail!("Version de schéma SQLite non prise en charge");
    }

    Ok(())
}

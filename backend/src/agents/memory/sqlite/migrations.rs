use anyhow::{Result, bail};
use rusqlite::Connection;

const SCHEMA_VERSION: i64 = 1;

pub(crate) fn apply(connection: &mut Connection) -> Result<()> {
    let transaction = connection.transaction()?;
    transaction.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version INTEGER PRIMARY KEY
        );",
    )?;
    let latest: Option<i64> =
        transaction.query_row("SELECT MAX(version) FROM schema_version", [], |row| {
            row.get(0)
        })?;
    match latest {
        Some(version) if version > SCHEMA_VERSION => {
            bail!("Version de schéma SQLite non prise en charge");
        }
        Some(SCHEMA_VERSION) => {
            // La base possède déjà la version attendue.
        }
        None => {
            transaction.execute_batch(
                "CREATE TABLE memory_entries (
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

                CREATE INDEX memory_entries_project_updated
                    ON memory_entries(project, updated_at DESC);

                INSERT INTO schema_version(version)
                    VALUES (1);",
            )?;
        }
        Some(version) => {
            bail!("Migration depuis la version {version} non implémentée");
        }
    }
    transaction.commit()?;
    Ok(())
}

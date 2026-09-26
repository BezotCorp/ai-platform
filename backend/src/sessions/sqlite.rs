use anyhow::{Result, bail};
use rusqlite::Connection;

pub(crate) fn apply(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS session_schema_version (
            version INTEGER PRIMARY KEY
        );",
    )?;
    let version: Option<i64> = connection.query_row(
        "SELECT MAX(version) FROM session_schema_version",
        [],
        |row| row.get(0),
    )?;
    match version {
        None => {
            connection.execute_batch(
                "CREATE TABLE sessions (
                    project TEXT NOT NULL,
                    id TEXT NOT NULL,
                    revision INTEGER NOT NULL DEFAULT 1,
                    messages TEXT NOT NULL,
                    created_at INTEGER NOT NULL
                        DEFAULT (unixepoch()),
                    updated_at INTEGER NOT NULL
                        DEFAULT (unixepoch()),
                    PRIMARY KEY(project, id)
                );

                CREATE INDEX sessions_project_updated
                    ON sessions(project, updated_at DESC);

                INSERT INTO session_schema_version(version)
                    VALUES (1);",
            )?;
        }
        Some(1) => {}
        Some(version) => {
            bail!("Version du schéma sessions non prise en charge : {version}");
        }
    }
    Ok(())
}

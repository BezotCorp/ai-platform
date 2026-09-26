use std::{path::Path, time::Duration};

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OpenFlags};

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

pub(crate) fn open_writer(path: &Path) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)
        .with_context(|| format!("Impossible d'ouvrir SQLite : {}", path.display()))?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.execute_batch("PRAGMA foreign_keys = ON;")?;
    Ok(connection)
}

pub(crate) fn configure_writer(connection: &Connection) -> Result<()> {
    connection.execute_batch(
        "PRAGMA journal_mode = WAL;
         PRAGMA synchronous = FULL;",
    )?;
    Ok(())
}

pub(crate) fn open_reader(path: &Path) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)
        .with_context(|| format!("Impossible d'ouvrir le lecteur SQLite : {}", path.display()))?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;
         PRAGMA query_only = ON;",
    )?;
    Ok(connection)
}

pub(crate) fn check_integrity(connection: &Connection) -> Result<()> {
    let result: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    if result != "ok" {
        bail!("Échec du contrôle d'intégrité SQLite : {result}");
    }
    Ok(())
}

use std::{path::Path, time::Duration};

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};

use super::migrations;

pub(crate) fn open(path: &Path) -> Result<Connection> {
    let flags = OpenFlags::SQLITE_OPEN_READ_WRITE
        | OpenFlags::SQLITE_OPEN_CREATE
        | OpenFlags::SQLITE_OPEN_NO_MUTEX;
    let connection = Connection::open_with_flags(path, flags)
        .with_context(|| format!("Impossible d'ouvrir SQLite : {}", path.display()))?;
    connection.busy_timeout(Duration::from_secs(5))?;
    migrations::apply(&connection)?;
    Ok(connection)
}

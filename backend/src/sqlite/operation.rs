use anyhow::Result;
use rusqlite::Connection;

use crate::sqlite::operation_result::OperationResult;

pub(crate) type Operation =
    Box<dyn FnOnce(&Connection) -> Result<OperationResult> + Send + 'static>;

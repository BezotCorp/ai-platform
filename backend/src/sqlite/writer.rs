use std::thread;

use anyhow::Result;
use rusqlite::{Connection, TransactionBehavior};
use tokio::sync::mpsc;

use crate::sqlite::job::{self, Job};

pub(crate) struct Writer {
    sender: mpsc::Sender<Job>,
    handle: thread::JoinHandle<()>,
}

impl Writer {
    pub(crate) fn start(connection: Connection) -> Result<Self> {
        let (sender, handle) = job::start(
            "sqlite-writer".to_owned(),
            connection,
            TransactionBehavior::Immediate,
        )?;
        Ok(Self { sender, handle })
    }

    pub(crate) async fn execute<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        job::submit(&self.sender, operation).await
    }

    pub(crate) fn into_parts(self) -> (mpsc::Sender<Job>, thread::JoinHandle<()>) {
        (self.sender, self.handle)
    }
}

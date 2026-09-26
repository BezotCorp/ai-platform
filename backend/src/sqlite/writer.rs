use std::{
    sync::{Arc, atomic::AtomicBool},
    thread,
};

use anyhow::Result;
use rusqlite::{Connection, TransactionBehavior};
use tokio::sync::mpsc;

use crate::sqlite::job::Job;

pub(crate) struct Writer {
    sender: mpsc::Sender<Job>,
    handle: thread::JoinHandle<()>,
}

impl Writer {
    pub(crate) fn start(connection: Connection, healthy: Arc<AtomicBool>) -> Result<Self> {
        let (sender, handle) = Job::start(
            "sqlite-writer".to_owned(),
            connection,
            TransactionBehavior::Immediate,
            healthy,
        )?;
        Ok(Self { sender, handle })
    }

    pub(crate) async fn execute<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        Job::submit(&self.sender, operation).await
    }

    pub(crate) fn into_parts(self) -> (mpsc::Sender<Job>, thread::JoinHandle<()>) {
        (self.sender, self.handle)
    }
}

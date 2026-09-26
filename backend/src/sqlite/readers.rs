use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    thread,
};

use anyhow::{Result, bail};
use rusqlite::{Connection, TransactionBehavior};
use tokio::sync::mpsc;

use crate::sqlite::job::Job;

pub(crate) struct Readers {
    senders: Vec<mpsc::Sender<Job>>,
    handles: Vec<thread::JoinHandle<()>>,
    next: AtomicUsize,
}

impl Readers {
    pub(crate) fn start(connections: Vec<Connection>, healthy: Arc<AtomicBool>) -> Result<Self> {
        if connections.is_empty() {
            bail!("Au moins un lecteur SQLite est nécessaire");
        }
        let mut senders = Vec::with_capacity(connections.len());
        let mut handles: Vec<thread::JoinHandle<()>> = Vec::with_capacity(connections.len());
        for (index, connection) in connections.into_iter().enumerate() {
            let started = Job::start(
                format!("sqlite-reader-{index}"),
                connection,
                TransactionBehavior::Deferred,
                Arc::clone(&healthy),
            );
            let (sender, handle) = match started {
                Ok(started) => started,
                Err(error) => {
                    // Fermer les canaux avant d'attendre les threads.
                    drop(senders);
                    for handle in handles {
                        handle.join().map_err(|_| {
                            anyhow::anyhow!("Un lecteur SQLite s'est terminé anormalement")
                        })?;
                    }
                    return Err(error);
                }
            };
            senders.push(sender);
            handles.push(handle);
        }
        Ok(Self {
            senders,
            handles,
            next: AtomicUsize::new(0),
        })
    }

    pub(crate) async fn execute<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let index = self.next.fetch_add(1, Ordering::Relaxed) % self.senders.len();
        Job::submit(&self.senders[index], operation).await
    }

    pub(crate) fn into_parts(self) -> (Vec<mpsc::Sender<Job>>, Vec<thread::JoinHandle<()>>) {
        (self.senders, self.handles)
    }
}

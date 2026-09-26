use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use anyhow::{Context, Result};
use rusqlite::{Connection, TransactionBehavior};
use tokio::sync::{mpsc, oneshot};

use crate::sqlite::{operation::Operation, operation_result::OperationResult};

pub(crate) struct Job {
    pub operation: Operation,
    pub response: oneshot::Sender<Result<OperationResult>>,
}

impl Job {
    fn execute(
        connection: &mut Connection,
        operation: Operation,
        behavior: TransactionBehavior,
    ) -> Result<OperationResult> {
        let transaction = connection.transaction_with_behavior(behavior)?;
        let result = operation(&transaction)?;
        transaction.commit()?;
        Ok(result)
    }

    pub(crate) fn start(
        name: String,
        mut connection: Connection,
        behavior: TransactionBehavior,
        healthy: Arc<AtomicBool>,
    ) -> Result<(mpsc::Sender<Job>, thread::JoinHandle<()>)> {
        let (sender, mut receiver) = mpsc::channel::<Job>(64);
        let handle = thread::Builder::new()
            .name(name)
            .spawn(move || {
                while let Some(job) = receiver.blocking_recv() {
                    if job.response.is_closed() {
                        continue;
                    }
                    if !healthy.load(Ordering::Acquire) {
                        let _ = job
                            .response
                            .send(Err(anyhow::anyhow!("Gestionnaire SQLite indisponible")));
                        continue;
                    }
                    let result = catch_unwind(AssertUnwindSafe(|| {
                        Self::execute(&mut connection, job.operation, behavior)
                    }));
                    match result {
                        Ok(result) => {
                            if job.response.send(result).is_err() {
                                eprintln!(
                                    "SQLite : demandeur absent \
                                     après exécution"
                                );
                            }
                        }
                        Err(_) => {
                            healthy.store(false, Ordering::Release);

                            let _ = job.response.send(Err(anyhow::anyhow!(
                                "Panic dans une opération SQLite ; \
                                     gestionnaire invalidé"
                            )));
                            receiver.close();
                            break;
                        }
                    }
                }
            })
            .context("Impossible de démarrer un thread SQLite")?;
        Ok((sender, handle))
    }

    pub(crate) async fn submit<T, F>(sender: &mpsc::Sender<Job>, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        let (response, receiver) = oneshot::channel();
        let job = Job {
            operation: Box::new(move |connection| Ok(Box::new(operation(connection)?))),
            response,
        };
        sender
            .send(job)
            .await
            .map_err(|_| anyhow::anyhow!("Le service SQLite est arrêté"))?;
        let result = receiver
            .await
            .context("Le thread SQLite a interrompu l'opération")??;
        result
            .downcast::<T>()
            .map(|value| *value)
            .map_err(|_| anyhow::anyhow!("Type de réponse SQLite incohérent"))
    }
}

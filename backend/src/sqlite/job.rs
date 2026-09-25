use std::thread;

use anyhow::{Context, Result};
use rusqlite::{Connection, TransactionBehavior};
use tokio::sync::{mpsc, oneshot};

use crate::sqlite::{operation::Operation, operation_result::OperationResult};

pub(crate) struct Job {
    pub operation: Operation,
    pub response: oneshot::Sender<Result<OperationResult>>,
}

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
) -> Result<(mpsc::Sender<Job>, thread::JoinHandle<()>)> {
    let (sender, mut receiver) = mpsc::channel::<Job>(64);
    let handle = thread::Builder::new()
        .name(name)
        .spawn(move || {
            while let Some(job) = receiver.blocking_recv() {
                // Une opération annulée avant son démarrage
                // n'a pas besoin d'être exécutée.
                if job.response.is_closed() {
                    continue;
                }
                // Une fois démarrée, la transaction se termine
                // même si son demandeur abandonne la réponse.
                let result = execute(&mut connection, job.operation, behavior);
                let _ = job.response.send(result);
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
        operation: Box::new(move |connection| {
            let result = operation(connection)?;
            Ok(Box::new(result))
        }),
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

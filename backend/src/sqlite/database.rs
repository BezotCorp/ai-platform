use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

use anyhow::{Context, Result, bail};
use rusqlite::Connection;

use crate::sqlite::{connection, database_inner::DatabaseInner, readers::Readers, writer::Writer};

#[derive(Clone)]
pub(crate) struct Database {
    inner: Arc<DatabaseInner>,
}

impl Database {
    /// Initialise les tables métier avant de démarrer les lecteurs.
    ///
    /// `initialize` appartient au module qui utilise SQLite :
    /// l'infrastructure n'importe aucun schéma métier.
    pub(crate) async fn open<F>(path: PathBuf, reader_count: usize, initialize: F) -> Result<Self>
    where
        F: FnOnce(&mut Connection) -> Result<()> + Send + 'static,
    {
        if !(1..=16).contains(&reader_count) {
            bail!("Le nombre de lecteurs SQLite doit être compris entre 1 et 16");
        }
        let inner = tokio::task::spawn_blocking(move || -> Result<DatabaseInner> {
            let mut writer_connection = connection::open_writer(&path)?;
            // Vérifier la base avant toute migration.
            connection::check_integrity(&writer_connection)?;
            connection::configure_writer(&writer_connection)?;
            // Initialiser le schéma avant d'ouvrir les lecteurs.
            initialize(&mut writer_connection)?;
            let mut reader_connections = Vec::with_capacity(reader_count);
            for _ in 0..reader_count {
                reader_connections.push(connection::open_reader(&path)?);
            }
            let healthy = Arc::new(AtomicBool::new(true));

            let readers = Readers::start(reader_connections, Arc::clone(&healthy))?;
            let writer = match Writer::start(writer_connection, Arc::clone(&healthy)) {
                Ok(writer) => writer,
                Err(error) => {
                    let (senders, handles) = readers.into_parts();
                    drop(senders);
                    for handle in handles {
                        handle.join().map_err(|_| {
                            anyhow::anyhow!("Un lecteur SQLite s'est terminé anormalement")
                        })?;
                    }
                    return Err(error);
                }
            };
            Ok(DatabaseInner {
                writer,
                readers,
                healthy,
                closing: AtomicBool::new(false),
            })
        })
        .await??;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Chaque écriture est exécutée dans une transaction IMMEDIATE.
    /// Le COMMIT appartient au gestionnaire, pas à l'appelant.
    pub(crate) async fn write<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        if self.inner.closing.load(Ordering::Acquire) {
            bail!("Le gestionnaire SQLite est en cours d'arrêt");
        }
        if !self.inner.healthy.load(Ordering::Acquire) {
            bail!("Le gestionnaire SQLite est indisponible");
        }
        self.inner.writer.execute(operation).await
    }

    /// Chaque lecture dispose d'un instantané transactionnel.
    /// La connexion utilisée reste exclusivement sur son thread.
    pub(crate) async fn read<T, F>(&self, operation: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T> + Send + 'static,
    {
        if self.inner.closing.load(Ordering::Acquire) {
            bail!("Le gestionnaire SQLite est en cours d'arrêt");
        }
        if !self.inner.healthy.load(Ordering::Acquire) {
            bail!("Le gestionnaire SQLite est indisponible");
        }
        self.inner.readers.execute(operation).await
    }

    /// Arrêt contrôlé : toutes les références clonées doivent
    /// avoir été libérées avant cet appel.
    ///
    /// Les opérations déjà engagées terminent leur transaction.
    /// Les demandes en attente dont le demandeur a disparu
    /// sont ignorées avant leur démarrage.
    pub(crate) async fn shutdown(self) -> Result<()> {
        let inner = self.inner;
        inner.closing.store(true, Ordering::Release);
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            while Arc::strong_count(&inner) != 1 {
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
        })
        .await
        .context("Des utilisateurs SQLite restent actifs")?;
        let inner = Arc::try_unwrap(inner).map_err(|_| {
            anyhow::anyhow!(
                "Impossible d'arrêter SQLite : des utilisateurs \
                possèdent encore une référence à la base"
            )
        })?;
        let (writer_sender, writer_handle) = inner.writer.into_parts();
        let (reader_senders, reader_handles) = inner.readers.into_parts();
        // La fermeture des canaux permet aux threads
        // de terminer après les opérations déjà en file.
        drop(writer_sender);
        drop(reader_senders);
        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut handles: Vec<thread::JoinHandle<()>> = reader_handles;
            handles.push(writer_handle);
            for handle in handles {
                handle
                    .join()
                    .map_err(|_| anyhow::anyhow!("Un thread SQLite s'est terminé anormalement"))?;
            }
            Ok(())
        })
        .await
        .context("Impossible d'attendre l'arrêt de SQLite")??;
        Ok(())
    }
}

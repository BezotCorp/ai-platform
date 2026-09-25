use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::params;

use crate::{
    memory::{
        memory_entry::MemoryEntry,
        revision::{checksum, project_scope},
        sqlite::{connection, search},
    },
};

#[derive(Clone)]
pub(crate) struct MemoryStore {
    database: PathBuf,
    project: String,
}

impl MemoryStore {
    pub(crate) async fn open(database: PathBuf, project_root: &Path) -> Result<Self> {
        if database.as_os_str().is_empty() || !database.is_absolute() {
            bail!("AI_PLATFORM_MEMORY_DB doit être un chemin absolu");
        }
        // Resolve the parent and any pre-existing target to prevent
        // symlink aliases from redirecting SQLite into the project.
        let parent = database
            .parent()
            .context("Chemin SQLite sans répertoire parent")?;
        let parent = tokio::fs::canonicalize(parent)
            .await
            .context("Répertoire parent SQLite inaccessible")?;
        if parent.starts_with(project_root) {
            bail!("La base mémoire doit être située hors du projet autorisé");
        }
        if let Ok(existing) = tokio::fs::canonicalize(&database).await
            && existing.starts_with(project_root)
        {
            bail!("La base mémoire ne peut pas pointer vers le projet");
        }

        let store = Self {
            database,
            project: project_scope(project_root),
        };
        let path = store.database.clone();
        tokio::task::spawn_blocking(move || -> Result<()> {
            connection::open(&path)?;
            Ok(())
        })
        .await??;

        Ok(store)
    }

    pub(crate) async fn recall(
        &self,
        query: &str,
        role: &str,
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        if limit > 8 {
            bail!("Limite de recherche mémoire excessive");
        }
        let database = self.database.clone();
        let project = self.project.clone();
        let query = query.to_owned();
        let role = role.to_owned();

        tokio::task::spawn_blocking(move || -> Result<Vec<MemoryEntry>> {
            let connection = connection::open(&database)?;
            let entries = search::find(&connection, &project, &query, &role, limit)?;
            if entries.iter().any(|entry| entry.project != project) {
                bail!("Erreur de cloisonnement de la mémoire");
            }
            Ok(entries
                .into_iter()
                .filter(|entry| {
                    checksum(&format!("{}\n{}", entry.query, entry.content))
                        == entry.checksum
                })
                .collect())
        })
        .await?
    }

    pub(crate) async fn remember(&self, query: &str, answer: &str) -> Result<()> {
        if query.trim().is_empty() || answer.trim().is_empty() {
            return Ok(());
        }

        // Les contenus complets des outils, les aperçus et les
        // approbations ne sont pas persistés dans la mémoire.
        let query = query.chars().take(1024).collect::<String>();
        let content = answer.chars().take(4096).collect::<String>();
        let checksum = checksum(&format!("{query}\n{content}"));
        let project = self.project.clone();
        let database = self.database.clone();

        tokio::task::spawn_blocking(move || -> Result<()> {
            let mut connection = connection::open(&database)?;
            let transaction = connection.transaction()?;
            transaction
                .execute(
                    "INSERT INTO memory_entries(project, query, content, source, checksum)
                     VALUES (?1, ?2, ?3, 'agent_final_answer', ?4)
                     ON CONFLICT(project, checksum) DO UPDATE SET
                         updated_at = unixepoch(),
                         revision = revision + 1",
                    params![project, query, content, checksum],
                )
                .context("Écriture de la mémoire SQLite impossible")?;
            transaction.commit()?;
            Ok(())
        })
        .await?
    }
}

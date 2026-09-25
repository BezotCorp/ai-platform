use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use rusqlite::params;

use crate::{
    agents::{MemoryEntry, apply, find, project_scope},
    sqlite::Database,
};

#[derive(Clone)]
pub(crate) struct MemoryStore {
    database: Database,
    project: String,
}

impl MemoryStore {
    pub(crate) async fn open(path: PathBuf, project_root: &Path) -> Result<Self> {
        if path.as_os_str().is_empty() || !path.is_absolute() {
            bail!("AI_PLATFORM_MEMORY_DB doit être un chemin absolu");
        }

        let parent = path
            .parent()
            .context("Chemin SQLite sans répertoire parent")?;

        let parent = tokio::fs::canonicalize(parent)
            .await
            .context("Répertoire parent SQLite inaccessible")?;

        if parent.starts_with(project_root) {
            bail!("La base mémoire doit être située hors du projet autorisé");
        }

        if let Ok(existing) = tokio::fs::canonicalize(&path).await
            && existing.starts_with(project_root)
        {
            bail!("La base mémoire ne peut pas pointer vers le projet");
        }

        let database = Database::open(path, 4, |connection| apply(connection)).await?;

        Ok(Self {
            database,
            project: project_scope(project_root),
        })
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

        let project = self.project.clone();
        let query = query.to_owned();
        let role = role.to_owned();

        self.database
            .read(move |connection| {
                let entries = find(connection, &project, &query, &role, limit)?;

                if entries.iter().any(|entry| entry.project() != project) {
                    bail!("Erreur de cloisonnement de la mémoire");
                }

                Ok(entries
                    .into_iter()
                    .filter(MemoryEntry::verify_checksum)
                    .collect())
            })
            .await
    }

    pub(crate) async fn remember(&self, query: &str, answer: &str) -> Result<()> {
        if query.trim().is_empty() || answer.trim().is_empty() {
            return Ok(());
        }

        let query = query.chars().take(1024).collect::<String>();
        let content = answer.chars().take(4096).collect::<String>();

        let checksum = MemoryEntry::checksum_for(&query, &content);

        let project = self.project.clone();

        self.database
            .write(move |connection| {
                connection
                    .execute(
                        "INSERT INTO memory_entries (
                            project,
                            query,
                            content,
                            source,
                            checksum
                        )
                        VALUES (
                            ?1,
                            ?2,
                            ?3,
                            'agent_final_answer',
                            ?4
                        )
                        ON CONFLICT(project, checksum)
                        DO UPDATE SET
                            updated_at = unixepoch(),
                            revision = revision + 1",
                        params![project, query, content, checksum],
                    )
                    .context("Écriture de la mémoire SQLite impossible")?;

                Ok(())
            })
            .await
    }
}

use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};

use crate::{
    agents::ExecutionMode,
    configurations::{
        ConfigurationSummary, SavedConfiguration, apply,
    },
    sqlite::Database,
};

#[derive(Clone)]
pub(crate) struct ConfigurationStore {
    database: Database,
    project: String,
}

impl ConfigurationStore {
    pub(crate) async fn open(
        database: Database,
        project: String,
    ) -> Result<Self> {
        database.write(apply).await?;

        Ok(Self { database, project })
    }

    fn validate_id(id: &str) -> Result<()> {
        if id.is_empty()
            || id.len() > 128
            || !id.bytes().all(|byte| {
                byte.is_ascii_alphanumeric()
                    || byte == b'-'
                    || byte == b'_'
            })
        {
            bail!("Identifiant de configuration invalide");
        }

        Ok(())
    }

    pub(crate) async fn save(
        &self,
        id: String,
        expected_revision: i64,
        mode: ExecutionMode,
    ) -> Result<ConfigurationSummary> {
        Self::validate_id(&id)?;

        if expected_revision < 0 {
            bail!("Révision de configuration invalide");
        }

        let mode = mode.validate()?;
        let encoded = serde_json::to_string(&mode)?;

        if encoded.len() > 64 * 1024 {
            bail!("Configuration trop volumineuse");
        }

        let project = self.project.clone();

        self.database
            .write(move |connection| {
                let changed = if expected_revision == 0 {
                    connection.execute(
                        "INSERT INTO configurations (
                            project,
                            id,
                            mode
                        )
                        VALUES (?1, ?2, ?3)
                        ON CONFLICT(project, id)
                        DO NOTHING",
                        params![project, id, encoded],
                    )?
                } else {
                    connection.execute(
                        "UPDATE configurations
                         SET mode = ?3,
                             revision = revision + 1,
                             updated_at = unixepoch()
                         WHERE project = ?1
                           AND id = ?2
                           AND revision = ?4",
                        params![
                            project,
                            id,
                            encoded,
                            expected_revision,
                        ],
                    )?
                };

                if changed != 1 {
                    bail!(
                        "Conflit de révision : configuration existante ou modifiée"
                    );
                }

                let summary = connection.query_row(
                    "SELECT
                        id,
                        revision,
                        created_at,
                        updated_at
                     FROM configurations
                     WHERE project = ?1
                       AND id = ?2",
                    params![project, id],
                    ConfigurationSummary::from_row,
                )?;

                Ok(summary)
            })
            .await
    }

    pub(crate) async fn load(
        &self,
        id: String,
    ) -> Result<Option<SavedConfiguration>> {
        Self::validate_id(&id)?;

        let project = self.project.clone();

        self.database
            .read(move |connection| {
                let stored = connection
                    .query_row(
                        "SELECT
                            id,
                            revision,
                            created_at,
                            updated_at,
                            mode
                         FROM configurations
                         WHERE project = ?1
                           AND id = ?2",
                        params![project, id],
                        |row| {
                            let summary =
                                ConfigurationSummary::from_row(row)?;

                            let encoded: String = row.get(4)?;

                            Ok((summary, encoded))
                        },
                    )
                    .optional()?;

                stored
                    .map(|(summary, encoded)| {
                        let mode: ExecutionMode =
                            serde_json::from_str(&encoded)?;

                        let mode = mode.validate()?;

                        Ok(SavedConfiguration {
                            id: summary.id,
                            revision: summary.revision,
                            created_at: summary.created_at,
                            updated_at: summary.updated_at,
                            mode,
                        })
                    })
                    .transpose()
            })
            .await
    }

    pub(crate) async fn list(
        &self,
    ) -> Result<Vec<ConfigurationSummary>> {
        let project = self.project.clone();

        self.database
            .read(move |connection| {
                let mut statement = connection.prepare(
                    "SELECT
                        id,
                        revision,
                        created_at,
                        updated_at
                     FROM configurations
                     WHERE project = ?1
                     ORDER BY updated_at DESC, id
                     LIMIT 50",
                )?;

                let rows = statement.query_map(
                    params![project],
                    ConfigurationSummary::from_row,
                )?;

                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .await
    }

    pub(crate) async fn delete(
        &self,
        id: String,
        expected_revision: i64,
    ) -> Result<bool> {
        Self::validate_id(&id)?;

        if expected_revision < 1 {
            bail!("Révision de configuration invalide");
        }

        let project = self.project.clone();

        self.database
            .write(move |connection| {
                let changed = connection.execute(
                    "DELETE FROM configurations
                     WHERE project = ?1
                       AND id = ?2
                       AND revision = ?3",
                    params![
                        project,
                        id,
                        expected_revision,
                    ],
                )?;

                Ok(changed == 1)
            })
            .await
    }
}

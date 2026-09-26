use anyhow::{Result, bail};
use rusqlite::{OptionalExtension, params};

use crate::{
    sessions::{History, Message, Session, apply},
    sqlite::Database,
};

#[derive(Clone)]
pub(crate) struct SessionStore {
    database: Database,
    project: String,
}

impl SessionStore {
    pub(crate) async fn open(database: Database, project: String) -> Result<Self> {
        database.write(apply).await?;

        Ok(Self { database, project })
    }

    fn validate_id(id: &str) -> Result<()> {
        if id.is_empty()
            || id.len() > 128
            || !id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
        {
            bail!("Identifiant de session invalide");
        }

        Ok(())
    }

    pub(crate) async fn save(
        &self,
        id: String,
        expected_revision: i64,
        messages: Vec<Message>,
    ) -> Result<Session> {
        Self::validate_id(&id)?;
        History::validate(&messages)?;

        if expected_revision < 0 {
            bail!("Révision de session invalide");
        }

        let project = self.project.clone();
        let encoded = serde_json::to_string(&messages)?;

        self.database
            .write(move |connection| {
                let changed = if expected_revision == 0 {
                    connection.execute(
                        "INSERT INTO sessions (
                            project,
                            id,
                            messages
                        )
                        VALUES (?1, ?2, ?3)
                        ON CONFLICT(project, id)
                        DO NOTHING",
                        params![project, id, encoded],
                    )?
                } else {
                    connection.execute(
                        "UPDATE sessions
                         SET messages = ?3,
                             revision = revision + 1,
                             updated_at = unixepoch()
                         WHERE project = ?1
                           AND id = ?2
                           AND revision = ?4",
                        params![project, id, encoded, expected_revision,],
                    )?
                };

                if changed != 1 {
                    bail!("Conflit de révision : session existante ou modifiée");
                }

                let session = connection.query_row(
                    "SELECT
                        id,
                        revision,
                        created_at,
                        updated_at
                     FROM sessions
                     WHERE project = ?1
                       AND id = ?2",
                    params![project, id],
                    |row| {
                        Ok(Session {
                            id: row.get(0)?,
                            revision: row.get(1)?,
                            created_at: row.get(2)?,
                            updated_at: row.get(3)?,
                        })
                    },
                )?;

                Ok(session)
            })
            .await
    }

    pub(crate) async fn load(&self, id: String) -> Result<Option<History>> {
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
                            messages
                         FROM sessions
                         WHERE project = ?1
                           AND id = ?2",
                        params![project, id],
                        |row| {
                            let session = Session {
                                id: row.get(0)?,
                                revision: row.get(1)?,
                                created_at: row.get(2)?,
                                updated_at: row.get(3)?,
                            };

                            let encoded: String = row.get(4)?;

                            Ok((session, encoded))
                        },
                    )
                    .optional()?;

                stored
                    .map(|(session, encoded)| {
                        let messages: Vec<Message> = serde_json::from_str(&encoded)?;

                        History::validate(&messages)?;

                        Ok(History { session, messages })
                    })
                    .transpose()
            })
            .await
    }

    pub(crate) async fn list(&self) -> Result<Vec<Session>> {
        let project = self.project.clone();

        self.database
            .read(move |connection| {
                let mut statement = connection.prepare(
                    "SELECT
                        id,
                        revision,
                        created_at,
                        updated_at
                     FROM sessions
                     WHERE project = ?1
                     ORDER BY updated_at DESC, id
                     LIMIT 50",
                )?;

                let rows = statement.query_map(params![project], |row| {
                    Ok(Session {
                        id: row.get(0)?,
                        revision: row.get(1)?,
                        created_at: row.get(2)?,
                        updated_at: row.get(3)?,
                    })
                })?;

                Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
            })
            .await
    }

    pub(crate) async fn delete(&self, id: String, expected_revision: i64) -> Result<bool> {
        Self::validate_id(&id)?;

        if expected_revision < 1 {
            bail!("Révision de session invalide");
        }

        let project = self.project.clone();

        self.database
            .write(move |connection| {
                let changed = connection.execute(
                    "DELETE FROM sessions
                     WHERE project = ?1
                       AND id = ?2
                       AND revision = ?3",
                    params![project, id, expected_revision,],
                )?;

                Ok(changed == 1)
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use crate::{
        sessions::{Message, SessionStore},
        sqlite::Database,
    };

    #[tokio::test]
    async fn persists_and_checks_revisions() -> anyhow::Result<()> {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();

        let path = std::env::temp_dir().join(format!(
            "ai-platform-sessions-{}-{stamp}.sqlite",
            std::process::id(),
        ));

        let database = Database::open(path.clone(), 2, |_| Ok(())).await?;

        let store = SessionStore::open(database.clone(), "project-a".into()).await?;

        let other = SessionStore::open(database.clone(), "project-b".into()).await?;

        let messages = vec![Message {
            role: "user".into(),
            content: "Première question".into(),
        }];

        let first = store.save("session_1".into(), 0, messages.clone()).await?;

        assert_eq!(first.revision, 1);

        assert!(
            store
                .save("session_1".into(), 0, messages.clone())
                .await
                .is_err()
        );

        assert!(
            store
                .save("session_1".into(), 3, messages.clone())
                .await
                .is_err()
        );

        let second = store.save("session_1".into(), 1, messages).await?;

        assert_eq!(second.revision, 2);

        let loaded = store
            .load("session_1".into())
            .await?
            .expect("La session doit exister");

        assert_eq!(loaded.session.revision, 2);
        assert_eq!(store.list().await?.len(), 1);

        assert!(other.load("session_1".into()).await?.is_none());

        assert!(!store.delete("session_1".into(), 1).await?);

        assert!(store.delete("session_1".into(), 2).await?);

        assert!(store.load("session_1".into()).await?.is_none());

        drop(store);
        drop(other);

        database.shutdown().await?;

        std::fs::remove_file(path)?;

        Ok(())
    }
}

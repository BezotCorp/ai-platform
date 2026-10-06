use rmcp::transport::auth::{AuthError, CredentialStore, StoredCredentials};
use serde::{Deserialize, Serialize};

use crate::config::Config;
#[derive(Serialize, Deserialize)]
struct PersistedCredentials {
    #[serde(flatten)]
    credentials: StoredCredentials,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    requested_scopes: Option<Vec<String>>,
}

/// Goose-specific credential store that uses the Config system
///
/// This implementation stores OAuth credentials in the goose configuration
/// system, which handles secure storage (e.g., keychain integration).

#[derive(Clone)]
pub struct GooseCredentialStore {
    name: String,
}

impl GooseCredentialStore {
    pub fn new(name: String) -> Self {
        Self { name }
    }

    fn secret_key(&self) -> String {
        format!("oauth_creds_{}", self.name)
    }

    fn load_persisted(&self) -> Result<Option<PersistedCredentials>, AuthError> {
        let config = Config::global();
        let key = self.secret_key();

        match config.get_secret::<PersistedCredentials>(&key) {
            Ok(credentials) => Ok(Some(credentials)),
            Err(_) => Ok(None),
        }
    }

    fn save_persisted(&self, credentials: PersistedCredentials) -> Result<(), AuthError> {
        let config = Config::global();
        let key = self.secret_key();

        config
            .set_secret(&key, &credentials)
            .map_err(|e| AuthError::InternalError(format!("Failed to save credentials: {}", e)))
    }

    pub fn load_requested_scopes(&self) -> Result<Option<Vec<String>>, AuthError> {
        Ok(self
            .load_persisted()?
            .and_then(|credentials| credentials.requested_scopes))
    }

    pub fn save_with_requested_scopes(
        &self,
        credentials: StoredCredentials,
        requested_scopes: Option<Vec<String>>,
    ) -> Result<(), AuthError> {
        self.save_persisted(PersistedCredentials {
            credentials,
            requested_scopes,
        })
    }
}

#[async_trait::async_trait]
impl CredentialStore for GooseCredentialStore {
    async fn load(&self) -> Result<Option<StoredCredentials>, AuthError> {
        Ok(self
            .load_persisted()?
            .map(|credentials| credentials.credentials))
    }

    async fn save(&self, credentials: StoredCredentials) -> Result<(), AuthError> {
        let requested_scopes = self.load_requested_scopes()?;
        self.save_with_requested_scopes(credentials, requested_scopes)
    }

    async fn clear(&self) -> Result<(), AuthError> {
        let config = Config::global();
        let key = self.secret_key();

        config
            .delete_secret(&key)
            .map_err(|e| AuthError::InternalError(format!("Failed to clear credentials: {}", e)))
    }
}

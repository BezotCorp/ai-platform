use super::{PairingState, PlatformUser};
use crate::config::{Config, base::SecretUpdate};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;

const PAIRINGS_CONFIG_KEY: &str = "gateway_pairings";
const PENDING_CODES_CONFIG_KEY: &str = "gateway_pending_codes";
const PENDING_CODES_SECRET_KEY: &str = "gateway_pending_codes";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPairing {
    platform: String,
    user_id: String,
    display_name: Option<String>,
    state: PairingState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPendingCode {
    code: String,
    gateway_type: String,
    expires_at: i64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoredPendingCodes {
    codes: Vec<StoredPendingCode>,
    #[serde(default)]
    legacy_import_complete: bool,
    #[serde(default)]
    imported_legacy_codes: Vec<String>,
}

impl StoredPendingCodes {
    fn revoke_imported_codes(&mut self) -> bool {
        let previous_len = self.codes.len();
        self.codes.retain(|pending| {
            !self
                .imported_legacy_codes
                .iter()
                .any(|code| code == &pending.code)
        });
        self.codes.len() != previous_len
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum StoredPendingCodesValue {
    State(StoredPendingCodes),
    Codes(Vec<StoredPendingCode>),
}

impl Default for StoredPendingCodesValue {
    fn default() -> Self {
        Self::State(StoredPendingCodes::default())
    }
}

impl StoredPendingCodesValue {
    fn into_state(self) -> StoredPendingCodes {
        match self {
            Self::State(state) => state,
            Self::Codes(codes) => StoredPendingCodes {
                codes,
                legacy_import_complete: false,
                imported_legacy_codes: Vec::new(),
            },
        }
    }
}

pub struct PairingStore {
    pairings: RwLock<HashMap<PlatformUser, PairingState>>,
}

impl PairingStore {
    pub fn new() -> anyhow::Result<Self> {
        let pairings = Self::load_pairings_from_config();
        Ok(Self {
            pairings: RwLock::new(pairings),
        })
    }

    fn load_pairings_from_config() -> HashMap<PlatformUser, PairingState> {
        let config = Config::global();
        let entries: Vec<StoredPairing> = config.get_param(PAIRINGS_CONFIG_KEY).unwrap_or_default();
        let mut map = HashMap::new();
        for entry in entries {
            let user = PlatformUser {
                platform: entry.platform,
                user_id: entry.user_id,
                display_name: entry.display_name,
            };
            map.insert(user, entry.state);
        }
        map
    }

    fn save_pairings_to_config(
        pairings: &HashMap<PlatformUser, PairingState>,
    ) -> anyhow::Result<()> {
        let entries: Vec<StoredPairing> = pairings
            .iter()
            .map(|(user, state)| StoredPairing {
                platform: user.platform.clone(),
                user_id: user.user_id.clone(),
                display_name: user.display_name.clone(),
                state: state.clone(),
            })
            .collect();
        Config::global()
            .set_param(PAIRINGS_CONFIG_KEY, &entries)
            .map_err(|e| anyhow::anyhow!("failed to save gateway pairings: {}", e))
    }

    fn migrate_pending_codes(config: &Config) -> anyhow::Result<()> {
        let legacy_codes: Vec<_> = config
            .get_param_source_values::<Vec<StoredPendingCode>>(PENDING_CODES_CONFIG_KEY)
            .map_err(|error| anyhow::anyhow!("failed to verify legacy pending codes: {error}"))?
            .into_iter()
            .flatten()
            .collect();
        if legacy_codes.is_empty() {
            return Ok(());
        }

        Self::revoke_legacy_pending_codes(config, legacy_codes)?;
        Err(anyhow::anyhow!(
            "legacy pending codes remain in ordinary configuration and have been revoked; stop older Goose processes, remove GATEWAY_PENDING_CODES and gateway_pending_codes from configured files, then retry and generate a new pairing code"
        ))
    }

    fn revoke_legacy_pending_codes(
        config: &Config,
        legacy_codes: Vec<StoredPendingCode>,
    ) -> anyhow::Result<()> {
        config
            .update_secret(
                PENDING_CODES_SECRET_KEY,
                |stored: StoredPendingCodesValue| {
                    let mut state = stored.into_state();
                    let mut changed = !state.legacy_import_complete;
                    for legacy_code in legacy_codes {
                        if state
                            .imported_legacy_codes
                            .iter()
                            .any(|code| code == &legacy_code.code)
                        {
                            continue;
                        }
                        state.imported_legacy_codes.push(legacy_code.code);
                        changed = true;
                    }
                    changed |= state.revoke_imported_codes();
                    state.legacy_import_complete = true;
                    if changed {
                        SecretUpdate::Write(state, ())
                    } else {
                        SecretUpdate::Unchanged(())
                    }
                },
            )
            .map_err(|error| anyhow::anyhow!("failed to migrate pending codes: {}", error))
    }

    fn store_pending_code_in(
        config: &Config,
        code: &str,
        gateway_type: &str,
        expires_at: i64,
    ) -> anyhow::Result<()> {
        Self::migrate_pending_codes(config)?;
        config
            .update_secret(
                PENDING_CODES_SECRET_KEY,
                |stored: StoredPendingCodesValue| {
                    let mut state = stored.into_state();
                    state.revoke_imported_codes();
                    state
                        .imported_legacy_codes
                        .retain(|legacy_code| legacy_code != code);
                    state.legacy_import_complete = true;
                    state.codes.retain(|pending| pending.code != code);
                    state.codes.push(StoredPendingCode {
                        code: code.to_string(),
                        gateway_type: gateway_type.to_string(),
                        expires_at,
                    });
                    SecretUpdate::Write(state, ())
                },
            )
            .map_err(|error| anyhow::anyhow!("failed to save pending codes: {}", error))
    }

    fn consume_pending_code_in(
        config: &Config,
        code: &str,
        now: i64,
    ) -> anyhow::Result<Option<String>> {
        Self::migrate_pending_codes(config)?;
        config
            .update_secret(
                PENDING_CODES_SECRET_KEY,
                |stored: StoredPendingCodesValue| {
                    let mut state = stored.into_state();
                    let mut needs_write = !state.legacy_import_complete;
                    state.legacy_import_complete = true;
                    needs_write |= state.revoke_imported_codes();
                    let Some(position) =
                        state.codes.iter().position(|pending| pending.code == code)
                    else {
                        return if needs_write {
                            SecretUpdate::Write(state, None)
                        } else {
                            SecretUpdate::Unchanged(None)
                        };
                    };
                    let consumed = Some(state.codes.remove(position))
                        .filter(|pending| now <= pending.expires_at)
                        .map(|pending| pending.gateway_type);
                    SecretUpdate::Write(state, consumed)
                },
            )
            .map_err(|error| anyhow::anyhow!("failed to consume pending code: {}", error))
    }

    pub async fn get(&self, user: &PlatformUser) -> anyhow::Result<PairingState> {
        let pairings = self.pairings.read().await;
        Ok(pairings
            .get(user)
            .cloned()
            .unwrap_or(PairingState::Unpaired))
    }

    pub async fn set(&self, user: &PlatformUser, state: PairingState) -> anyhow::Result<()> {
        let mut pairings = self.pairings.write().await;
        pairings.insert(user.clone(), state);
        Self::save_pairings_to_config(&pairings)
    }

    pub async fn remove(&self, user: &PlatformUser) -> anyhow::Result<()> {
        let mut pairings = self.pairings.write().await;
        pairings.remove(user);
        Self::save_pairings_to_config(&pairings)
    }

    pub async fn store_pending_code(
        &self,
        code: &str,
        gateway_type: &str,
        expires_at: i64,
    ) -> anyhow::Result<()> {
        Self::store_pending_code_in(Config::global(), code, gateway_type, expires_at)
    }

    pub async fn consume_pending_code(&self, code: &str) -> anyhow::Result<Option<String>> {
        let now = chrono::Utc::now().timestamp();
        Self::consume_pending_code_in(Config::global(), code, now)
    }

    pub fn generate_code() -> String {
        use rand::RngExt;
        let chars: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        let mut rng = rand::rng();
        (0..6)
            .map(|_| chars[rng.random_range(0..chars.len())] as char)
            .collect()
    }

    pub async fn remove_all_for_platform(&self, platform: &str) -> anyhow::Result<usize> {
        let mut pairings = self.pairings.write().await;
        let before = pairings.len();
        pairings.retain(|user, _| user.platform != platform);
        let removed = before - pairings.len();
        Self::save_pairings_to_config(&pairings)?;
        Ok(removed)
    }

    pub async fn list_paired_users(
        &self,
        gateway_type: &str,
    ) -> anyhow::Result<Vec<(PlatformUser, String, i64)>> {
        let pairings = self.pairings.read().await;
        let mut result = Vec::new();
        for (user, state) in pairings.iter() {
            if user.platform == gateway_type {
                if let PairingState::Paired {
                    session_id,
                    paired_at,
                } = state
                {
                    result.push((user.clone(), session_id.clone(), *paired_at));
                }
            }
        }
        Ok(result)
    }
}

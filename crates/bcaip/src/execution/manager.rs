use crate::agents::mcp_client::BcaipMcpHostInfo;
use crate::agents::{Agent, AgentConfig, BcaipPlatform, ExtensionLoadResult};
use crate::session::{SessionManager, SessionNameUpdate};
use crate::{
    config::{Config, permission::PermissionManager},
    scheduler_trait::SchedulerTrait,
};
use anyhow::Result;
use lru::LruCache;
use std::{collections::HashMap, num::NonZeroUsize, sync::Arc};
use tokio::sync::{Mutex, OnceCell, RwLock, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{debug, info};

const DEFAULT_MAX_SESSION: usize = 100;

static AGENT_MANAGER: OnceCell<Arc<AgentManager>> = OnceCell::const_new();

#[derive(Clone, Default)]
pub struct RuntimeContext {
    pub mcp_host_info: Option<BcaipMcpHostInfo>,
    pub use_login_shell_path: Option<bool>,
    pub session_name_update_tx: Option<mpsc::UnboundedSender<SessionNameUpdate>>,
}

pub struct AgentManagerGetResult {
    pub agent: Arc<Agent>,
    pub agent_created: bool,
    pub extension_results: Vec<ExtensionLoadResult>,
}

pub struct AgentManager {
    sessions: Arc<RwLock<LruCache<String, Arc<Agent>>>>,
    agent_config: AgentConfig,
    default_provider: Arc<RwLock<Option<Arc<dyn bcaip_provider_types::base::Provider>>>>,
    cancel_tokens: Arc<RwLock<HashMap<String, CancellationToken>>>,
    /// Per-session creation locks.  When `get_or_create_agent` misses the
    /// `sessions` cache it acquires the per-session lock before doing the
    /// expensive work (provider restore, MCP extension initialization) so
    /// concurrent callers for the same session never race into doing the
    /// work twice.  Entries are inserted on demand and pruned when the
    /// session is removed *or* evicted by the LRU; the underlying
    /// `Arc<Mutex<()>>` stays alive as long as any caller still holds it,
    /// even after the HashMap entry is removed.
    creation_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl AgentManager {
    pub async fn new(agent_config: AgentConfig, max_sessions: Option<usize>) -> Result<Self> {
        let capacity = NonZeroUsize::new(max_sessions.unwrap_or(DEFAULT_MAX_SESSION))
            .unwrap_or_else(|| NonZeroUsize::new(100).unwrap());

        let manager = Self {
            sessions: Arc::new(RwLock::new(LruCache::new(capacity))),
            agent_config,
            default_provider: Arc::new(RwLock::new(None)),
            cancel_tokens: Arc::new(RwLock::new(HashMap::new())),
            creation_locks: Arc::new(Mutex::new(HashMap::new())),
        };

        Ok(manager)
    }

    pub async fn instance() -> Result<Arc<Self>> {
        AGENT_MANAGER
            .get_or_try_init(|| async {
                let config = Config::global();
                let max_sessions = config
                    .get_bcaip_max_active_agents()
                    .unwrap_or(DEFAULT_MAX_SESSION);
                let default_mode = config.get_bcaip_mode().unwrap_or_default();
                let session_manager = Arc::new(SessionManager::instance());
                let agent_config = AgentConfig::new(
                    session_manager,
                    PermissionManager::instance(),
                    None,
                    default_mode,
                    config.get_bcaip_disable_session_naming().unwrap_or(false),
                    BcaipPlatform::BcaipDesktop,
                );
                let manager = Self::new(agent_config, Some(max_sessions)).await?;
                Ok(Arc::new(manager))
            })
            .await
            .cloned()
    }

    pub fn scheduler(&self) -> Option<Arc<dyn SchedulerTrait>> {
        self.agent_config.scheduler_service.as_ref().map(Arc::clone)
    }

    /// Get the shared SessionManager for session-only operations
    pub fn session_manager(&self) -> &SessionManager {
        self.agent_config.session_manager.as_ref()
    }

    pub async fn set_default_provider(
        &self,
        provider: Arc<dyn bcaip_provider_types::base::Provider>,
    ) {
        debug!("Setting default provider on AgentManager");
        *self.default_provider.write().await = Some(provider);
    }

    pub async fn get_or_create_agent(&self, session_id: String) -> Result<Arc<Agent>> {
        Ok(self
            .get_or_create_agent_with_runtime_context(session_id, RuntimeContext::default())
            .await?
            .agent)
    }

    pub async fn get_or_create_agent_with_runtime_context(
        &self,
        session_id: String,
        runtime_context: RuntimeContext,
    ) -> Result<AgentManagerGetResult> {
        // Fast path: agent already cached.
        {
            let mut sessions = self.sessions.write().await;
            if let Some(existing) = sessions.get(&session_id) {
                return Ok(AgentManagerGetResult {
                    agent: Arc::clone(existing),
                    agent_created: false,
                    extension_results: Vec::new(),
                });
            }
        }

        // Slow path: serialize creation per session so concurrent callers
        // (e.g. start_agent's background extension-loading task and a
        // resume_agent request racing through the frontend) cannot each
        // construct their own Agent and independently send `initialize` to
        // every MCP server.  See issue #9031.
        let creation_lock = {
            let mut locks = self.creation_locks.lock().await;
            Arc::clone(
                locks
                    .entry(session_id.clone())
                    .or_insert_with(|| Arc::new(Mutex::new(()))),
            )
        };
        let creation_guard = creation_lock.lock().await;

        // Funnel the fallible work through a helper so we can prune the
        // per-session creation lock on every error exit.  Without this
        // the provider-setup path (update_provider / update_mode) could
        // bail out via `?`, leaving a permanent `creation_locks` entry
        // for a session that never made it into the LRU cache and that
        // no one will ever call `remove_session` on.
        let result = self.create_agent_locked(&session_id, runtime_context).await;

        if result.is_err() {
            // Release BOTH the guard and our local Arc clone of the
            // creation lock before pruning.  `prune_creation_lock`
            // gates removal on `Arc::strong_count == 1`; if we kept
            // `creation_lock` alive the count would still be at least
            // two (HashMap + this local) and the failed session would
            // leak its lock entry forever.  In-flight waiters keep the
            // Arc alive on their own and prune correctly skips while
            // they hold it.
            drop(creation_guard);
            drop(creation_lock);
            self.prune_creation_lock(&session_id).await;
        }

        result
    }

    /// Slow-path body for `get_or_create_agent`.  Must be called with the
    /// per-session creation lock held by the caller.
    async fn create_agent_locked(
        &self,
        session_id: &str,
        runtime_context: RuntimeContext,
    ) -> Result<AgentManagerGetResult> {
        // Re-check under the creation lock: another caller may have
        // finished creating the agent while we were waiting.
        {
            let mut sessions = self.sessions.write().await;
            if let Some(existing) = sessions.get(session_id) {
                return Ok(AgentManagerGetResult {
                    agent: Arc::clone(existing),
                    agent_created: false,
                    extension_results: Vec::new(),
                });
            }
        }

        let mut mode = self.agent_config.bcaip_mode;
        if let Ok(session) = self
            .agent_config
            .session_manager
            .get_session(session_id, false)
            .await
        {
            mode = session.bcaip_mode;
            info!(bcaip_mode = %mode, session_id = %session_id, "Session loaded");
        }

        let mut config = self.agent_config.clone();
        config.bcaip_mode = mode;
        config.mcp_host_info = runtime_context.mcp_host_info;
        config.use_login_shell_path = runtime_context.use_login_shell_path;
        config.session_name_update_tx = runtime_context.session_name_update_tx;
        let agent = Arc::new(Agent::with_config(config));
        let mut extension_results = Vec::new();

        if let Ok(session) = self
            .agent_config
            .session_manager
            .get_session(session_id, false)
            .await
        {
            if session.provider_name.is_some() {
                info!(
                    "Restoring evicted session {} (provider: {:?})",
                    session_id, session.provider_name
                );
                if let Err(error) = agent.restore_provider_from_session(&session).await {
                    if crate::acp::is_auth_required(&error) {
                        return Err(error);
                    }
                    tracing::warn!(
                        "Failed to restore provider for session {}: {}",
                        session_id,
                        error
                    );
                }
            }
            extension_results = agent.load_extensions_from_session(&session).await;
            if let Some(recipe) = &session.recipe {
                agent
                    .apply_recipe_components(recipe.response.clone(), true)
                    .await?;
            }
        }

        if agent.provider().await.is_err() {
            if let Some(provider) = &*self.default_provider.read().await {
                let config = crate::config::Config::global();
                let model_config = config
                    .get_bcaip_provider()
                    .ok()
                    .zip(config.get_bcaip_model().ok())
                    .and_then(|(provider_name, model_name)| {
                        crate::model_config::model_config_from_user_config(
                            &provider_name,
                            &model_name,
                        )
                        .ok()
                    })
                    .unwrap_or_else(|| bcaip_provider_types::model::ModelConfig::new("unknown"));
                agent
                    .update_provider(Arc::clone(provider), model_config, session_id)
                    .await?;
                provider
                    .update_mode(session_id, mode)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to propagate mode to provider: {}", e))?;
            }
        }

        let mut sessions = self.sessions.write().await;
        if let Some(existing) = sessions.get(session_id) {
            return Ok(AgentManagerGetResult {
                agent: Arc::clone(existing),
                agent_created: false,
                extension_results: Vec::new(),
            });
        }
        // `push` returns the LRU-evicted entry when the cache is at
        // capacity, which `put` does not surface.  We need the evicted
        // key so we can also drop its creation lock below, otherwise the
        // `creation_locks` HashMap would grow without bound in long-lived
        // processes that churn through many sessions.
        let evicted = sessions
            .push(session_id.to_string(), agent.clone())
            .map(|(k, _)| k);
        drop(sessions);

        if let Some(evicted_id) = evicted {
            self.prune_creation_lock(&evicted_id).await;
        }

        Ok(AgentManagerGetResult {
            agent,
            agent_created: true,
            extension_results,
        })
    }

    /// Drop the per-session creation lock for `session_id` if no other
    /// caller is currently holding a clone of its `Arc`.  Holding the
    /// `creation_locks` mutex while we both check `Arc::strong_count` and
    /// remove guarantees no new waiter can race in between the check and
    /// the removal: any new caller would need to acquire the outer mutex
    /// first to clone the inner `Arc`.
    ///
    /// If a waiter is still in flight (strong_count > 1) we leave the
    /// entry in place so the in-flight callers continue to serialize
    /// through the same lock; a later removal or eviction will sweep it.
    async fn prune_creation_lock(&self, session_id: &str) {
        let mut locks = self.creation_locks.lock().await;
        let in_use = locks
            .get(session_id)
            .is_some_and(|lock| Arc::strong_count(lock) > 1);
        if !in_use {
            locks.remove(session_id);
        }
    }

    pub async fn remove_session(&self, session_id: &str) -> Result<()> {
        if let Some(token) = self.cancel_tokens.write().await.remove(session_id) {
            token.cancel();
        }
        let mut sessions = self.sessions.write().await;
        sessions
            .pop(session_id)
            .ok_or_else(|| anyhow::anyhow!("Session {} not found", session_id))?;
        drop(sessions);
        // Best-effort prune of the per-session creation lock so the
        // HashMap doesn't grow unbounded.  Any caller still holding a
        // clone of the Arc keeps the underlying Mutex alive until it
        // releases its guard.
        self.prune_creation_lock(session_id).await;
        info!("Removed session {}", session_id);
        Ok(())
    }

    /// Drops an in-memory agent when one is loaded for `session_id`.
    pub async fn remove_session_if_loaded(&self, session_id: &str) -> Result<()> {
        if let Some(token) = self.cancel_tokens.write().await.remove(session_id) {
            token.cancel();
        }
        let mut sessions = self.sessions.write().await;
        if sessions.pop(session_id).is_none() {
            return Ok(());
        }
        drop(sessions);
        self.prune_creation_lock(session_id).await;
        info!("Removed session {}", session_id);
        Ok(())
    }

    pub async fn has_session(&self, session_id: &str) -> bool {
        self.sessions.read().await.contains(session_id)
    }

    pub async fn session_count(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Atomically check if busy and register a cancel token. Returns Err if already busy.
    pub async fn try_register_cancel_token(
        &self,
        session_id: &str,
        token: CancellationToken,
    ) -> Result<()> {
        let mut tokens = self.cancel_tokens.write().await;
        if tokens.contains_key(session_id) {
            anyhow::bail!("Session '{}' is currently busy", session_id);
        }
        tokens.insert(session_id.to_string(), token);
        Ok(())
    }

    /// Remove the cancellation token for a session (called when reply finishes)
    pub async fn unregister_cancel_token(&self, session_id: &str) {
        self.cancel_tokens.write().await.remove(session_id);
    }

    /// Cancel a running agent by triggering its cancellation token
    pub async fn cancel_session(&self, session_id: &str) -> Result<()> {
        let tokens = self.cancel_tokens.read().await;
        let token = tokens
            .get(session_id)
            .ok_or_else(|| anyhow::anyhow!("No active operation for session {}", session_id))?;
        token.cancel();
        Ok(())
    }

    /// Check if a session has an active reply in progress
    pub async fn is_session_busy(&self, session_id: &str) -> bool {
        let tokens = self.cancel_tokens.read().await;
        tokens.contains_key(session_id)
    }

    /// List session IDs that currently have active agents loaded
    pub async fn list_active_session_ids(&self) -> Vec<String> {
        self.sessions
            .read()
            .await
            .iter()
            .map(|(id, _)| id.clone())
            .collect()
    }
}

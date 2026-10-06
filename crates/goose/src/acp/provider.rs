use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::schema::v1::{
    Annotations as AcpAnnotations, ClientCapabilities, CloseSessionRequest, ContentBlock,
    ContentChunk, EnvVariable, HttpHeader, ImageContent, InitializeRequest, InitializeResponse,
    LoadSessionRequest, McpCapabilities, McpServer, McpServerHttp, McpServerStdio,
    NewSessionRequest, NewSessionResponse, PromptRequest, PromptResponse, RequestPermissionOutcome,
    RequestPermissionRequest, RequestPermissionResponse, Role as AcpRole, SessionConfigKind,
    SessionConfigOption, SessionConfigOptionCategory, SessionConfigSelectOption,
    SessionConfigSelectOptions, SessionId, SessionModeState, SessionNotification, SessionUpdate,
    SetSessionConfigOptionRequest, SetSessionModeRequest, SetSessionModeResponse, StopReason,
    TextContent, ToolCallContent, ToolCallStatus, ToolKind,
};
use agent_client_protocol::{Agent, Client, ConnectionTo};
use agent_client_protocol_schema::v1::{AGENT_METHOD_NAMES, Usage as AcpUsage};
use anyhow::{Context, Result};
use async_stream::try_stream;
use futures::future::BoxFuture;
use bcaip_provider_types::conversations::{ProviderUsage, Usage};
use rmcp::model::{CallToolRequestParams, CallToolResult, ContentBlock as RmcpContent, Role, Tool};
use std::collections::{HashMap, HashSet};
use std::mem;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use std::thread::JoinHandle;
use std::{future::Future, path::PathBuf, process::Stdio};
use tokio::io::AsyncReadExt;
use tokio::process::{Child, Command};
use tokio::sync::{Mutex as TokioMutex, mpsc, oneshot, watch};
use tokio_util::compat::{TokioAsyncReadCompatExt as _, TokioAsyncWriteCompatExt as _};

use crate::acp::handoff::{build_handoff_context_memo, memo_token_budget, prompt_token_cost};
use crate::acp::{PermissionDecision, map_permission_response};
use crate::config::{Config, ExtensionConfig};
use crate::{
    subprocess::configure_subprocess, token_counter::create_token_counter,
    utils::sanitize_unicode_tags,
};
use bcaip_provider_types::base::{MessageStream, PermissionRouting, Provider};
use bcaip_provider_types::conversations::{
    Message, MessageContent, TOOL_META_EXTERNAL_DISPATCH_KEY,
};
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::goose_mode::GooseMode;
use bcaip_provider_types::model::ModelConfig;
use bcaip_provider_types::permission::PrincipalType;
use bcaip_provider_types::permission::{Permission, PermissionConfirmation};
use bcaip_provider_types::thinking::{
    ThinkingEffortCapability, ThinkingEffortOption, ThinkingEffortSupport,
};

/// Sentinel: resolved to the actual model name during connect().
pub const ACP_CURRENT_MODEL: &str = "current";

/// Config option id used by agents that advertise a thinking-effort selector
/// without categorizing it as `thought_level`.
const EFFORT_CONFIG_OPTION_ID: &str = "effort";

/// Session request param holding the selected thinking effort.
pub(crate) const THINKING_EFFORT_PARAM: &str = "thinking_effort";

pub struct AcpProviderConfig {
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub work_dir: PathBuf,
    pub mcp_servers: Vec<McpServer>,
    pub session_mode_id: Option<String>,
    pub session_config_options: Vec<(String, String)>,
    /// Config option id used to select the model (e.g. `"model"`). When set, the
    /// provider re-applies this option from the per-completion `ModelConfig`
    /// whenever the active session model changes.
    pub model_config_option_id: Option<String>,
    pub mode_mapping: HashMap<GooseMode, Vec<String>>,
    pub notification_callback: Option<Arc<dyn Fn(SessionNotification) + Send + Sync>>,
}

enum ClientRequest {
    NewSession {
        response_tx: oneshot::Sender<Result<NewSessionResponse>>,
    },
    LoadSession {
        session_id: SessionId,
        response_tx: oneshot::Sender<Result<NewSessionResponse>>,
    },
    CloseSession {
        session_id: SessionId,
    },
    SetMode {
        session_id: SessionId,
        mode_id: String,
        response_tx: oneshot::Sender<Result<()>>,
    },
    SetConfigOption {
        session_id: SessionId,
        config_id: String,
        value: String,
        response_tx: oneshot::Sender<Result<()>>,
    },
    Prompt {
        session_id: SessionId,
        content: Vec<ContentBlock>,
        response_tx: mpsc::Sender<AcpUpdate>,
    },
}

// tokio I/O handles can't move between runtimes, so the child process must be
// spawned inside the OS thread. This closure lets start() share all other logic.
type ClientLoopFn = Box<
    dyn FnOnce(
            AcpClientLoop,
            mpsc::Receiver<ClientRequest>,
            oneshot::Sender<Result<InitializeResponse>>,
            oneshot::Receiver<()>,
        ) -> BoxFuture<'static, ()>
        + Send,
>;

struct ClientLoopGuard {
    tx: Option<mpsc::Sender<ClientRequest>>,
    cancel_tx: Option<oneshot::Sender<()>>,
    thread: Option<JoinHandle<()>>,
}

impl Drop for ClientLoopGuard {
    fn drop(&mut self) {
        if let Some(cancel_tx) = self.cancel_tx.take() {
            let _ = cancel_tx.send(());
        }
        self.tx.take();
        if let Some(thread) = self.thread.take() {
            std::thread::spawn(move || {
                if let Err(error) = thread.join() {
                    tracing::debug!("AcpClientLoop thread panicked: {error:?}");
                }
            });
        }
    }
}

#[derive(Debug)]
enum AcpUpdate {
    Text(TextContent),
    Thought(String),
    ToolCallStart {
        id: String,
        name: String,
        kind: ToolKind,
        raw_input: Option<serde_json::Value>,
    },
    ToolCallComplete {
        id: String,
        raw_output: Option<serde_json::Value>,
        content: Option<Vec<ToolCallContent>>,
        is_error: bool,
    },
    PermissionRequest {
        request: Box<RequestPermissionRequest>,
        response_tx: oneshot::Sender<RequestPermissionResponse>,
    },
    Complete(StopReason, Option<AcpUsage>),
    Error(agent_client_protocol::Error),
}

/// Whether dropping the handoff memo could plausibly change the outcome. An agent that
/// rejected the very first update has told us nothing except that it disliked the prompt,
/// and the memo is the only part we added — but a spent account or a missing credential
/// says nothing about the prompt at all, so retrying would burn the single fallback the
/// session gets and consume a memo the agent never actually refused.
fn retry_without_memo_could_help(error: &agent_client_protocol::Error) -> bool {
    if error.code == agent_client_protocol::schema::v1::ErrorCode::AuthRequired {
        return false;
    }
    error
        .data
        .as_ref()
        .and_then(|data| data.get("reason"))
        .and_then(serde_json::Value::as_str)
        != Some(crate::acp::CREDITS_EXHAUSTED_REASON)
}

fn provider_error_from_acp(error: agent_client_protocol::Error) -> ProviderError {
    if error.code == agent_client_protocol::schema::v1::ErrorCode::AuthRequired {
        ProviderError::Authentication(error.to_string())
    } else {
        ProviderError::RequestFailed(error.to_string())
    }
}

/// Per-tool-call buffer for accumulating ACP ToolCallUpdate fields across
/// non-terminal updates, drained on the terminal status update.
#[derive(Debug, Default)]
struct AccumulatedToolCall {
    raw_output: Option<serde_json::Value>,
    content: Vec<ToolCallContent>,
}

/// The single ACP session backing this provider instance.
#[derive(Clone)]
struct AcpSession {
    id: SessionId,
    response: NewSessionResponse,
}

struct HandoffContextClaim {
    first_prompt: bool,
    include_context: bool,
}

struct HandoffContextClaimGuard {
    handoff_context_sent: Arc<AtomicBool>,
    pending: bool,
}

impl HandoffContextClaimGuard {
    fn new(handoff_context_sent: Arc<AtomicBool>, first_prompt: bool) -> Self {
        Self {
            handoff_context_sent,
            pending: first_prompt,
        }
    }

    fn commit(&mut self) {
        self.pending = false;
    }

    fn rollback(&mut self) {
        if self.pending {
            self.handoff_context_sent.store(false, Ordering::Release);
            self.pending = false;
        }
    }
}

impl Drop for HandoffContextClaimGuard {
    fn drop(&mut self) {
        if self.pending {
            self.handoff_context_sent.store(false, Ordering::Release);
        }
    }
}

#[derive(Clone)]
struct AcpEffortState {
    capability: Arc<Mutex<Option<ThinkingEffortCapability>>>,
    updates: watch::Sender<ThinkingEffortSupport>,
}

impl AcpEffortState {
    fn new() -> Self {
        let (updates, _) = watch::channel(ThinkingEffortSupport::Unsupported);
        Self {
            capability: Arc::new(Mutex::new(None)),
            updates,
        }
    }
}

#[derive(Clone)]
struct AcpSessionState {
    active_id: Arc<Mutex<Option<SessionId>>>,
    effort: AcpEffortState,
}

impl AcpSessionState {
    fn new(effort: AcpEffortState) -> Self {
        Self {
            active_id: Arc::new(Mutex::new(None)),
            effort,
        }
    }
}

pub struct AcpProvider {
    name: String,
    goose_mode: Arc<Mutex<GooseMode>>,
    mode_mapping: HashMap<GooseMode, Vec<String>>,

    session: Mutex<AcpSession>,

    pending_confirmations:
        Arc<TokioMutex<HashMap<String, oneshot::Sender<PermissionConfirmation>>>>,
    pending_tool_updates: Arc<Mutex<HashMap<String, AccumulatedToolCall>>>,
    /// True after the first ACP prompt completes with the handoff context committed.
    /// Failed or abandoned first prompts reset this so the next prompt can retry it.
    handoff_context_sent: Arc<AtomicBool>,
    /// Latest `size` reported by the ACP server in a `session/update` →
    /// `usage_update` notification. 0 means no real update has arrived yet.
    context_size: Arc<AtomicU64>,

    /// Config option id used to select the model, if this agent supports it.
    model_config_option_id: Option<String>,
    /// Model currently applied via `model_config_option_id`, used to avoid
    /// redundant `SetConfigOption` calls.
    applied_model: Arc<Mutex<Option<String>>>,

    /// The agent's thinking-effort config option, mirrored from every
    /// config-options payload it sends. `None` means the agent offers no effort
    /// selector for its current model. Its `current` value doubles as the
    /// redundant-send guard: unlike a goose-side cache of the last applied
    /// value, it tracks the agent resetting its own effort (e.g. on a model
    /// switch), so the persisted value is re-applied when that happens.
    effort: AcpEffortState,

    tx: Option<mpsc::Sender<ClientRequest>>,
    cancel_tx: Option<oneshot::Sender<()>>,
    loop_thread: Option<JoinHandle<()>>,
}

impl std::fmt::Debug for AcpProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AcpProvider")
            .field("name", &self.name)
            .finish()
    }
}

fn spawn_client_loop(fut: impl Future<Output = ()> + Send + 'static) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("failed to build ACP client runtime");
        rt.block_on(fut)
    })
}

impl AcpProvider {
    pub async fn connect(
        name: String,
        goose_mode: GooseMode,
        config: AcpProviderConfig,
    ) -> Result<Self> {
        Self::start(
            name,
            goose_mode,
            config,
            Box::new(|cl, rx, init_tx, mut cancel_rx| {
                Box::pin(async move {
                    tokio::select! {
                        biased;
                        _ = &mut cancel_rx => {}
                        _ = cl.spawn(rx, init_tx) => {}
                    }
                })
            }),
        )
        .await
    }

    #[doc(hidden)]
    pub async fn connect_with_transport(
        name: String,
        goose_mode: GooseMode,
        config: AcpProviderConfig,
        transport: impl agent_client_protocol::ConnectTo<Client> + 'static,
    ) -> Result<Self> {
        Self::start(
            name,
            goose_mode,
            config,
            Box::new(move |cl, mut rx, init_tx, mut cancel_rx| {
                Box::pin(async move {
                    tokio::select! {
                        biased;
                        _ = &mut cancel_rx => {}
                        result = cl.run(transport, &mut rx, init_tx) => {
                            if let Err(e) = result {
                                tracing::error!("ACP protocol error: {e}");
                            }
                        }
                    }
                })
            }),
        )
        .await
    }

    async fn start(
        name: String,
        goose_mode: GooseMode,
        config: AcpProviderConfig,
        run: ClientLoopFn,
    ) -> Result<Self> {
        let (tx, rx) = mpsc::channel(32);
        let (init_tx, init_rx) = oneshot::channel();
        let mode_mapping = config.mode_mapping.clone();
        let model_config_option_id = config.model_config_option_id.clone();
        let applied_model = config.model_config_option_id.as_ref().and_then(|id| {
            config
                .session_config_options
                .iter()
                .find(|(opt_id, _)| opt_id == id)
                .map(|(_, value)| value.clone())
        });
        let goose_mode_shared = Arc::new(Mutex::new(goose_mode));
        let pending_tool_updates: Arc<Mutex<HashMap<String, AccumulatedToolCall>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let context_size = Arc::new(AtomicU64::new(0));
        let effort = AcpEffortState::new();
        let client_loop = AcpClientLoop::new(
            config,
            goose_mode_shared.clone(),
            pending_tool_updates.clone(),
            context_size.clone(),
            effort.clone(),
        );
        let (cancel_tx, cancel_rx) = oneshot::channel();
        let loop_thread = spawn_client_loop(run(client_loop, rx, init_tx, cancel_rx));
        let mut client_loop_guard = ClientLoopGuard {
            tx: Some(tx),
            cancel_tx: Some(cancel_tx),
            thread: Some(loop_thread),
        };

        let _init_response = init_rx
            .await
            .context("ACP client initialization cancelled")??;

        // Create the ACP session eagerly during connect.
        let (session_tx, session_rx) = oneshot::channel();
        client_loop_guard
            .tx
            .as_ref()
            .unwrap()
            .send(ClientRequest::NewSession {
                response_tx: session_tx,
            })
            .await
            .context("ACP client is unavailable")?;
        let response = session_rx
            .await
            .context("ACP session creation cancelled")??;

        let session = AcpSession {
            id: response.session_id.clone(),
            response,
        };

        Ok(Self {
            name,
            goose_mode: goose_mode_shared,
            mode_mapping,
            session: Mutex::new(session),
            pending_confirmations: Arc::new(TokioMutex::new(HashMap::new())),
            pending_tool_updates,
            handoff_context_sent: Arc::new(AtomicBool::new(false)),
            context_size,
            model_config_option_id,
            applied_model: Arc::new(Mutex::new(applied_model)),
            effort,
            tx: client_loop_guard.tx.take(),
            cancel_tx: client_loop_guard.cancel_tx.take(),
            loop_thread: client_loop_guard.thread.take(),
        })
    }

    fn acp_session_id(&self) -> SessionId {
        self.session.lock().unwrap().id.clone()
    }

    async fn load_session(&self, session_id: SessionId) -> Result<AcpSession> {
        let (response_tx, response_rx) = oneshot::channel();
        self.tx
            .as_ref()
            .unwrap()
            .send(ClientRequest::LoadSession {
                session_id,
                response_tx,
            })
            .await
            .context("ACP client is unavailable")?;
        let response = response_rx.await.context("ACP session load cancelled")??;
        Ok(AcpSession {
            id: response.session_id.clone(),
            response,
        })
    }

    pub(crate) async fn send_set_mode(&self, _goose_id: &str, mode_id: String) -> Result<()> {
        let session_id = self.acp_session_id();
        let (response_tx, response_rx) = oneshot::channel();
        self.tx
            .as_ref()
            .unwrap()
            .send(ClientRequest::SetMode {
                session_id,
                mode_id,
                response_tx,
            })
            .await
            .context("ACP client is unavailable")?;
        response_rx.await.context("ACP request cancelled")?
    }

    pub(crate) async fn send_set_config_option(
        &self,
        _goose_id: &str,
        config_id: String,
        value: String,
    ) -> Result<()> {
        let session_id = self.acp_session_id();
        let (response_tx, response_rx) = oneshot::channel();
        self.tx
            .as_ref()
            .unwrap()
            .send(ClientRequest::SetConfigOption {
                session_id,
                config_id,
                value,
                response_tx,
            })
            .await
            .context("ACP client is unavailable")?;
        response_rx.await.context("ACP request cancelled")?
    }

    /// Re-apply the model selection config option when the active session model
    /// differs from what was last applied. ACP agents that select their model
    /// via a config option (e.g. Copilot) need this so resumed or switched
    /// sessions actually use the requested model instead of the agent default.
    async fn apply_model_if_changed(&self, model_name: &str) -> Result<()> {
        let Some(config_id) = self.model_config_option_id.clone() else {
            return Ok(());
        };
        if model_name == ACP_CURRENT_MODEL {
            return Ok(());
        }

        {
            let applied = self
                .applied_model
                .lock()
                .map_err(|_| anyhow::anyhow!("applied_model lock poisoned"))?;
            if applied.as_deref() == Some(model_name) {
                return Ok(());
            }
        }

        self.send_set_config_option("", config_id, model_name.to_string())
            .await?;

        let mut applied = self
            .applied_model
            .lock()
            .map_err(|_| anyhow::anyhow!("applied_model lock poisoned"))?;
        *applied = Some(model_name.to_string());
        Ok(())
    }

    fn effort_capability(&self) -> Result<Option<ThinkingEffortCapability>> {
        Ok(self
            .effort
            .capability
            .lock()
            .map_err(|_| anyhow::anyhow!("effort_state lock poisoned"))?
            .clone())
    }

    /// The redundant-send guard is the mirrored capability's `current`, not a
    /// goose-side record of the last sent value: the agent's `SetConfigOption`
    /// response refreshes `effort_state` before this call returns, and later
    /// refreshes track the agent resetting its own effort.
    async fn set_effort_option(
        &self,
        goose_id: &str,
        option_id: &str,
        value: String,
    ) -> Result<()> {
        {
            let state = self
                .effort
                .capability
                .lock()
                .map_err(|_| anyhow::anyhow!("effort_state lock poisoned"))?;
            let current = state
                .as_ref()
                .and_then(|capability| capability.current.as_deref());
            if current == Some(value.as_str()) {
                return Ok(());
            }
        }

        self.send_set_config_option(goose_id, option_id.to_string(), value)
            .await
    }

    /// Forward the session's thinking effort to the agent when it differs from
    /// the agent's mirrored current value. A recreated provider (model switch,
    /// provider switch, session reload) starts from the agent's own default, so
    /// the value has to be re-applied rather than assumed. It comes from
    /// `resolve_effort_value`, the same resolver the config menu advertises
    /// from, so the selection ACP clients see is the one that gets sent.
    async fn apply_effort_if_changed(&self, model_config: &ModelConfig) -> Result<()> {
        let Some(capability) = self.effort_capability()? else {
            return Ok(());
        };
        let Some(mapped) = resolve_effort_value(&capability, model_config) else {
            return Ok(());
        };

        self.set_effort_option("", &capability.option_id, mapped)
            .await
    }

    async fn prompt(
        &self,
        session_id: SessionId,
        content: Vec<ContentBlock>,
    ) -> Result<mpsc::Receiver<AcpUpdate>> {
        let (response_tx, response_rx) = mpsc::channel(64);
        self.tx
            .as_ref()
            .unwrap()
            .send(ClientRequest::Prompt {
                session_id,
                content,
                response_tx,
            })
            .await
            .context("ACP client is unavailable")?;
        Ok(response_rx)
    }

    fn session_has_config_option(&self, category: SessionConfigOptionCategory) -> bool {
        self.session
            .lock()
            .unwrap()
            .response
            .config_options
            .as_ref()
            .is_some_and(|opts| opts.iter().any(|o| o.category.as_ref() == Some(&category)))
    }

    fn claim_handoff_context(&self, messages: &[Message]) -> HandoffContextClaim {
        let first_prompt = !self.handoff_context_sent.swap(true, Ordering::AcqRel);
        HandoffContextClaim {
            first_prompt,
            include_context: first_prompt && has_handoff_context(messages),
        }
    }

    /// Prior conversation, bounded against the agent's context window. The agent's own
    /// system prompt and tool schemas are invisible to us, hence the conservative share.
    async fn bounded_handoff_memo(
        &self,
        model_config: &ModelConfig,
        messages: &[Message],
        current_prompt: &[ContentBlock],
    ) -> Option<String> {
        let last_user_index = last_user_message_index(messages)?;
        let counter = match create_token_counter().await {
            Ok(counter) => counter,
            Err(error) => {
                tracing::error!(%error, "no token counter, dropping ACP handoff context");
                return None;
            }
        };

        let context_limit = crate::context_limit::get_context_limit(self, &model_config.model_name)
            .await
            .ok()?;
        let budget = memo_token_budget(context_limit, prompt_token_cost(current_prompt, &counter));

        build_handoff_context_memo(&messages[..last_user_index], budget, &counter)
    }
}

fn fresh_text_run() -> (String, i64) {
    (
        uuid::Uuid::new_v4().to_string(),
        chrono::Utc::now().timestamp(),
    )
}

#[async_trait::async_trait]
impl Provider for AcpProvider {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn provider_session_id(&self) -> Option<String> {
        Some(self.acp_session_id().to_string())
    }

    async fn resume(&self, session_id: &str) -> Result<(), ProviderError> {
        if self.acp_session_id().0.as_ref() == session_id {
            return Ok(());
        }

        let previous_session_id = self.acp_session_id();
        let loaded = self
            .load_session(SessionId::new(session_id))
            .await
            .map_err(|error| ProviderError::RequestFailed(error.to_string()))?;
        *self.session.lock().unwrap() = loaded;
        self.handoff_context_sent.store(true, Ordering::Release);
        let _ = self
            .tx
            .as_ref()
            .unwrap()
            .send(ClientRequest::CloseSession {
                session_id: previous_session_id,
            })
            .await;
        Ok(())
    }

    async fn get_context_limit(&self, model: &str, override_limit: Option<usize>) -> usize {
        bcaip_provider_types::context_limit::ContextLimitResolver::new(self.get_name())
            .resolve(model, override_limit, || async {
                let size = self.context_size.load(Ordering::Relaxed);
                Ok((size > 0).then_some(size as usize))
            })
            .await
    }

    async fn update_mode(&self, session_id: &str, mode: GooseMode) -> Result<(), ProviderError> {
        if let Some(candidates) = self.mode_mapping.get(&mode) {
            let session = self.session.lock().unwrap().clone();
            let mode_str =
                select_mode_id(candidates, session.response.modes.as_ref()).ok_or_else(|| {
                    ProviderError::RequestFailed(format!(
                        "None of the mode ids [{}] are offered by the agent",
                        candidates.join(", ")
                    ))
                })?;
            if self.session_has_config_option(SessionConfigOptionCategory::Mode) {
                self.send_set_config_option(session_id, "mode".into(), mode_str)
                    .await
                    .map_err(|e| {
                        ProviderError::RequestFailed(format!("Failed to set mode: {e}"))
                    })?;
            } else {
                self.send_set_mode(session_id, mode_str)
                    .await
                    .map_err(|e| {
                        ProviderError::RequestFailed(format!("Failed to set mode: {e}"))
                    })?;
            }
        }

        if let Ok(mut guard) = self.goose_mode.lock() {
            *guard = mode;
        }
        Ok(())
    }

    fn thinking_effort_support(&self) -> ThinkingEffortSupport {
        match self.effort_capability().ok().flatten() {
            Some(capability) => ThinkingEffortSupport::Options(capability),
            None => ThinkingEffortSupport::Unsupported,
        }
    }

    fn subscribe_thinking_effort_support(&self) -> Option<watch::Receiver<ThinkingEffortSupport>> {
        Some(self.effort.updates.subscribe())
    }

    async fn set_thinking_effort(
        &self,
        session_id: &str,
        value: &str,
    ) -> Result<bool, ProviderError> {
        let capability = self
            .effort_capability()
            .map_err(|e| ProviderError::RequestFailed(e.to_string()))?;
        // No effort selector: the only advertised choice is `off`, which is a
        // no-op. Falling back to the caller's legacy path would respawn the
        // agent, while accepting any other value would persist a setting that
        // was never applied.
        let Some(capability) = capability else {
            return if value.eq_ignore_ascii_case("off") {
                Ok(true)
            } else {
                Err(ProviderError::InvalidValue(format!(
                    "Agent offers no thinking effort '{value}'"
                )))
            };
        };
        let mapped = map_effort_value(&capability, value).ok_or_else(|| {
            ProviderError::InvalidValue(format!("Agent offers no thinking effort '{value}'"))
        })?;

        self.set_effort_option(session_id, &capability.option_id, mapped)
            .await
            .map_err(|e| effort_option_error(value, e))?;
        Ok(true)
    }

    async fn apply_model_selection(&self, model_config: &ModelConfig) -> Result<(), ProviderError> {
        self.apply_model_if_changed(&model_config.model_name)
            .await
            .map_err(|e| {
                ProviderError::RequestFailed(format!("Failed to set ACP model option: {e}"))
            })
    }

    fn permission_routing(&self) -> PermissionRouting {
        PermissionRouting::ActionRequired
    }

    fn manages_own_context(&self) -> bool {
        true
    }

    async fn handle_permission_confirmation(
        &self,
        request_id: &str,
        confirmation: &PermissionConfirmation,
    ) -> bool {
        let mut pending = self.pending_confirmations.lock().await;
        if let Some(tx) = pending.remove(request_id) {
            let _ = tx.send(confirmation.clone());
            return true;
        }
        false
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        _system: &str,
        messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let session_id = self.acp_session_id();

        self.apply_model_if_changed(&model_config.model_name)
            .await
            .map_err(|e| {
                ProviderError::RequestFailed(format!("Failed to set ACP model option: {e}"))
            })?;

        self.apply_effort_if_changed(model_config)
            .await
            .map_err(|e| {
                ProviderError::RequestFailed(format!("Failed to set ACP effort option: {e}"))
            })?;

        let current_prompt_blocks = messages_to_prompt(messages, None);
        if current_prompt_blocks.is_empty() {
            return Ok(Box::pin(futures::stream::empty()));
        }

        let claim = self.claim_handoff_context(messages);
        let mut handoff_claim_guard =
            HandoffContextClaimGuard::new(self.handoff_context_sent.clone(), claim.first_prompt);
        let memo = if claim.include_context {
            self.bounded_handoff_memo(model_config, messages, &current_prompt_blocks)
                .await
        } else {
            None
        };
        if claim.include_context && memo.is_none() {
            // Nothing fit beside this turn's prompt, so the context never left goose. Give
            // it back rather than marking a handoff that never happened as done — a single
            // oversized turn would otherwise cost the session its whole history.
            handoff_claim_guard.rollback();
        }
        // A memo is only ever an estimate of what the agent will accept, so keep the bare
        // prompt to retry with. Without it a x estimate leaves the session unresumable.
        let (prompt_blocks, mut bare_retry_blocks) = match memo {
            Some(memo) => (
                messages_to_prompt(messages, Some(memo)),
                Some(current_prompt_blocks),
            ),
            None => (current_prompt_blocks, None),
        };
        // Drop any tool-call buffer state left over from a prior prompt
        // (e.g. cancelled or interrupted before its terminal status arrived).
        if let Ok(mut buffer) = self.pending_tool_updates.lock() {
            buffer.clear();
        }
        let mut rx = match self.prompt(session_id.clone(), prompt_blocks).await {
            Ok(rx) => rx,
            Err(e) => match bare_retry_blocks.take() {
                Some(blocks) => {
                    // Consume the handoff before retrying. The memo is the only thing this
                    // attempt added, so rebuilding it next time would reproduce the same
                    // rejection and leave the session permanently unresumable.
                    handoff_claim_guard.commit();
                    self.prompt(session_id.clone(), blocks)
                        .await
                        .map_err(|retry_error| {
                            ProviderError::RequestFailed(format!(
                                "Failed to send ACP prompt: {retry_error}"
                            ))
                        })?
                }
                // Nothing was added to this prompt, so the guard rolls the claim back as
                // it drops and the next attempt can still carry the context.
                None => {
                    return Err(ProviderError::RequestFailed(format!(
                        "Failed to send ACP prompt: {e}"
                    )));
                }
            },
        };
        let bare_retry =
            bare_retry_blocks.map(|blocks| (self.tx.as_ref().unwrap().clone(), session_id, blocks));

        let pending_confirmations = self.pending_confirmations.clone();
        let goose_mode = *self
            .goose_mode
            .lock()
            .map_err(|_| ProviderError::RequestFailed("goose_mode lock poisoned".into()))?;

        let reject_all_tools = goose_mode == GooseMode::Chat;
        let model_name = model_config.model_name.clone();

        Ok(Box::pin(try_stream! {
            let mut suppress_text = false;
            let mut bare_retry = bare_retry;
            let mut updates_seen = 0usize;
            let mut rejected_tool_calls: HashSet<String> = HashSet::new();
            // Stable id+timestamp per contiguous run so Desktop coalesces chunks into one bubble.
            let mut text_run: Option<(String, i64)> = None;
            let mut thought_run: Option<(String, i64)> = None;

            while let Some(update) = rx.recv().await {
                updates_seen += 1;
                match update {
                    AcpUpdate::Text(text) => {
                        if !suppress_text {
                            let (id, ts) = text_run
                                .get_or_insert_with(fresh_text_run)
                                .clone();
                            let message = acp_text_update_message(text, id, ts);
                            yield (Some(message), None);
                        }
                    }
                    AcpUpdate::Thought(text) => {
                        let (id, ts) = thought_run
                            .get_or_insert_with(fresh_text_run)
                            .clone();
                        let message = Message::new(Role::Assistant, ts, vec![])
                            .with_thinking(text, "")
                            .with_visibility(true, false)
                            .with_id(id);
                        yield (Some(message), None);
                    }
                    AcpUpdate::ToolCallStart { id, name, kind, raw_input } => {
                        text_run = None;
                        thought_run = None;
                        if reject_all_tools {
                            suppress_text = true;
                            rejected_tool_calls.insert(id);
                        } else {
                            let mut params = CallToolRequestParams::new(name);
                            if let Some(serde_json::Value::Object(map)) = raw_input {
                                params = params.with_arguments(map);
                            }
                            // external_dispatch tells the agent loop not to redispatch this
                            // call. goose.acp.kind preserves ACP's stable categorization for
                            // downstream consumers (metrics, observability, icon selection)
                            // independent of the display title we put in `name`.
                            let tool_meta = Some(serde_json::json!({
                                TOOL_META_EXTERNAL_DISPATCH_KEY: true,
                                "goose.acp.kind": kind,
                            }));
                            let message = Message::assistant().with_tool_request_with_metadata(
                                id,
                                Ok(params),
                                None,
                                tool_meta,
                            );
                            yield (Some(message), None);
                        }
                    }
                    AcpUpdate::ToolCallComplete {
                        id,
                        raw_output,
                        content,
                        is_error,
                    } => {
                        text_run = None;
                        thought_run = None;
                        if rejected_tool_calls.remove(&id) {
                            // In chat mode no tool_request was emitted (suppressed at
                            // ToolCallStart), so surface a plain text message. In other
                            // modes a tool_request WAS emitted, so pair it with an error
                            // tool_response so downstream consumers see the rejection.
                            if reject_all_tools {
                                let message = Message::assistant()
                                    .with_text("Tool call was denied.")
                                    .with_generated_id();
                                yield (Some(message), None);
                            } else {
                                let denial = vec![RmcpContent::text("Tool call was denied.")];
                                let result = CallToolResult::error(denial);
                                let message =
                                    Message::user().with_tool_response(id, Ok(result));
                                yield (Some(message), None);
                            }
                        } else {
                            let result_content =
                                acp_tool_call_content_to_rmcp(content, raw_output);
                            let result = if is_error {
                                CallToolResult::error(result_content)
                            } else {
                                CallToolResult::success(result_content)
                            };
                            let message = Message::user().with_tool_response(id, Ok(result));
                            yield (Some(message), None);
                        }
                    }
                    AcpUpdate::PermissionRequest { request, response_tx } => {
                        text_run = None;
                        thought_run = None;
                        if let Some(decision) = permission_decision_from_mode(goose_mode) {
                            if decision.should_record_rejection() {
                                rejected_tool_calls.insert(request.tool_call.tool_call_id.0.to_string());
                            }
                            let _ = response_tx.send(map_permission_response(&request, decision));
                            continue;
                        }

                        let request_id = request.tool_call.tool_call_id.0.to_string();
                        let (tx, rx) = oneshot::channel();

                        pending_confirmations
                            .lock()
                            .await
                            .insert(request_id.clone(), tx);

                        if let Some(action_required) = build_action_required_message(&request) {
                            yield (Some(action_required), None);
                        }

                        let confirmation = rx.await.unwrap_or(PermissionConfirmation {
                            principal_type: PrincipalType::Tool,
                            permission: Permission::Cancel,
                        });

                        pending_confirmations.lock().await.remove(&request_id);

                        let decision = PermissionDecision::from(confirmation.permission);
                        if decision.should_record_rejection() {
                            rejected_tool_calls.insert(request.tool_call.tool_call_id.0.to_string());
                        }
                        let _ = response_tx.send(map_permission_response(&request, decision));
                    }
                    AcpUpdate::Complete(reason, usage) => {
                        // Prefer retrying context over silently losing it. A harness may have
                        // ingested the memo before cancelling or refusing, so a retry can duplicate
                        // it, but treating an unprocessed handoff as delivered is unrecoverable.
                        if matches!(reason, StopReason::Cancelled | StopReason::Refusal) {
                            handoff_claim_guard.rollback();
                        } else {
                            handoff_claim_guard.commit();
                        }
                        if let Some(usage) = usage {
                            let provider_usage = ProviderUsage::new(
                                model_name.clone(),
                                Usage::new(
                                    Some(usage.input_tokens as i32),
                                    Some(usage.output_tokens as i32),
                                    Some(usage.total_tokens as i32),
                                ),
                            );
                            yield (None, Some(provider_usage));
                        }
                        break;
                    }
                    AcpUpdate::Error(e) => {
                        let retry_could_help = retry_without_memo_could_help(&e);
                        let error = provider_error_from_acp(e);
                        if updates_seen == 1 && retry_could_help
                            && let Some((tx, session_id, blocks)) = bare_retry.take() {
                                // Consume the handoff before retrying. The agent has already
                                // seen and rejected this memo, so rebuilding it on a later
                                // turn would reproduce the rejection forever.
                                handoff_claim_guard.commit();
                                let (response_tx, response_rx) = mpsc::channel(64);
                                let request = ClientRequest::Prompt {
                                    session_id,
                                    content: blocks,
                                    response_tx,
                                };
                                if tx.send(request).await.is_ok() {
                                    tracing::error!(
                                        %error,
                                        "ACP prompt with handoff context rejected, retrying without it"
                                    );
                                    rx = response_rx;
                                    updates_seen = 0;
                                    continue;
                                }
                            }
                        // Reset before yielding so an immediate retry can include the handoff even
                        // while the failed stream value is still alive.
                        handoff_claim_guard.rollback();
                        Err(error)?;
                    }
                }
            }
        }))
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        let session = self.session.lock().unwrap().clone();
        let (_, available) = resolve_model_info(&self.name, &session.response)?;
        Ok(available)
    }
}

impl Drop for AcpProvider {
    fn drop(&mut self) {
        self.tx.take();
        let _cancel_tx = self.cancel_tx.take();
        if let Some(h) = self.loop_thread.take()
            && let Err(e) = h.join()
        {
            tracing::debug!("AcpClientLoop thread panicked: {e:?}");
        }
    }
}

struct AcpClientLoop {
    config: AcpProviderConfig,
    goose_mode: Arc<Mutex<GooseMode>>,
    prompt_response_tx: Arc<Mutex<Option<mpsc::Sender<AcpUpdate>>>>,
    pending_tool_updates: Arc<Mutex<HashMap<String, AccumulatedToolCall>>>,
    context_size: Arc<AtomicU64>,
    effort: AcpEffortState,
}

impl AcpClientLoop {
    fn new(
        config: AcpProviderConfig,
        goose_mode: Arc<Mutex<GooseMode>>,
        pending_tool_updates: Arc<Mutex<HashMap<String, AccumulatedToolCall>>>,
        context_size: Arc<AtomicU64>,
        effort: AcpEffortState,
    ) -> Self {
        Self {
            config,
            goose_mode,
            prompt_response_tx: Arc::new(Mutex::new(None)),
            pending_tool_updates,
            context_size,
            effort,
        }
    }

    async fn spawn(
        self,
        mut rx: mpsc::Receiver<ClientRequest>,
        init_tx: oneshot::Sender<Result<InitializeResponse>>,
    ) {
        let child = match spawn_acp_process(&self.config).await {
            Ok(c) => c,
            Err(e) => {
                let _ = init_tx.send(Err(anyhow::anyhow!("{e}")));
                tracing::error!("failed to spawn ACP process: {e}");
                return;
            }
        };

        match self.run_with_child(child, &mut rx, init_tx).await {
            Ok(()) => tracing::debug!("ACP protocol loop exited cleanly"),
            Err(e) => tracing::error!(error = %e, "ACP protocol loop error"),
        }
    }

    async fn run_with_child(
        self,
        mut child: Child,
        rx: &mut mpsc::Receiver<ClientRequest>,
        init_tx: oneshot::Sender<Result<InitializeResponse>>,
    ) -> Result<()> {
        let stdin = child.stdin.take().context("no stdin")?;
        let stdout = child.stdout.take().context("no stdout")?;
        if let Some(stderr) = child.stderr.take() {
            tokio::spawn(forward_child_stderr(stderr));
        }
        let transport =
            agent_client_protocol::ByteStreams::new(stdin.compat_write(), stdout.compat());
        let result = self.run(transport, rx, init_tx).await;
        let _ = child.kill().await;
        let _ = child.wait().await;
        result
    }

    async fn run(
        self,
        transport: impl agent_client_protocol::ConnectTo<Client> + 'static,
        rx: &mut mpsc::Receiver<ClientRequest>,
        init_tx: oneshot::Sender<Result<InitializeResponse>>,
    ) -> Result<()> {
        let AcpClientLoop {
            config,
            goose_mode,
            prompt_response_tx,
            pending_tool_updates,
            context_size,
            effort,
        } = self;
        let notification_callback = config.notification_callback.clone();
        let reverse_modes = reverse_mode_mapping(&config.mode_mapping);
        let session_state = AcpSessionState::new(effort);

        Client
            .builder()
            .on_receive_notification(
                {
                    let prompt_response_tx = prompt_response_tx.clone();
                    let reverse_modes = reverse_modes.clone();
                    let goose_mode = goose_mode.clone();
                    let pending_tool_updates = pending_tool_updates.clone();
                    let context_size = context_size.clone();
                    let session_state = session_state.clone();
                    async move |notification: SessionNotification, _cx| {
                        let is_active_session =
                            session_state.active_id.lock().is_ok_and(|active| {
                                active
                                    .as_ref()
                                    .is_none_or(|id| id == &notification.session_id)
                            });
                        if !is_active_session {
                            return Ok(());
                        }
                        if let Some(ref cb) = notification_callback {
                            cb(notification.clone());
                        }
                        match &notification.update {
                            SessionUpdate::CurrentModeUpdate(update) => {
                                if let Some(mode) = resolve_mode(
                                    &reverse_modes,
                                    update.current_mode_id.0.as_ref(),
                                    &goose_mode,
                                ) && let Ok(mut guard) = goose_mode.lock()
                                {
                                    *guard = mode;
                                }
                            }
                            SessionUpdate::ConfigOptionUpdate(update) => {
                                publish_effort_state(&session_state.effort, &update.config_options);
                                for opt in &update.config_options {
                                    if opt.category == Some(SessionConfigOptionCategory::Mode)
                                        && let SessionConfigKind::Select(sel) = &opt.kind
                                        && let Some(mode) = resolve_mode(
                                            &reverse_modes,
                                            sel.current_value.0.as_ref(),
                                            &goose_mode,
                                        )
                                        && let Ok(mut guard) = goose_mode.lock()
                                    {
                                        *guard = mode;
                                    }
                                }
                            }
                            SessionUpdate::UsageUpdate(usage) => {
                                context_size.store(usage.size, Ordering::Relaxed);
                            }
                            _ => {}
                        }
                        if let Some(tx) = prompt_response_tx
                            .lock()
                            .ok()
                            .as_ref()
                            .and_then(|g| g.as_ref())
                        {
                            match notification.update {
                                SessionUpdate::AgentMessageChunk(ContentChunk {
                                    content: ContentBlock::Text(text),
                                    ..
                                }) => {
                                    let _ = tx.try_send(AcpUpdate::Text(text));
                                }
                                SessionUpdate::AgentThoughtChunk(ContentChunk {
                                    content: ContentBlock::Text(TextContent { text, .. }),
                                    ..
                                }) => {
                                    let _ = tx.try_send(AcpUpdate::Thought(text));
                                }
                                SessionUpdate::ToolCall(tool_call) => {
                                    let id = tool_call.tool_call_id.0.to_string();
                                    let initial_status = tool_call.status;
                                    let synchronous_terminal = matches!(
                                        initial_status,
                                        ToolCallStatus::Completed | ToolCallStatus::Failed
                                    );
                                    // Seed the buffer; drain immediately if the call is
                                    // already terminal (synchronous tool, no follow-up).
                                    let synchronous_accumulated =
                                        if let Ok(mut buffer) = pending_tool_updates.lock() {
                                            let entry = buffer.entry(id.clone()).or_default();
                                            if let Some(raw_output) = tool_call.raw_output.clone() {
                                                entry.raw_output = Some(raw_output);
                                            }
                                            entry.content.extend(tool_call.content.clone());
                                            if synchronous_terminal {
                                                buffer.remove(&id)
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        };
                                    // ACP carries no canonical tool name to clients — only
                                    // `title` (display) and `kind` (category). We pass `title`
                                    // for renderer affordance, surface `kind` separately via
                                    // tool_meta for stable categorization, and the
                                    // goose.external_dispatch marker keeps `name` off the
                                    // agent loop's routing/auth paths.
                                    let _ = tx.try_send(AcpUpdate::ToolCallStart {
                                        id: id.clone(),
                                        name: tool_call.title.clone(),
                                        kind: tool_call.kind,
                                        raw_input: tool_call.raw_input.clone(),
                                    });
                                    if let Some(accumulated) = synchronous_accumulated {
                                        let content = if accumulated.content.is_empty() {
                                            None
                                        } else {
                                            Some(accumulated.content)
                                        };
                                        let _ = tx.try_send(AcpUpdate::ToolCallComplete {
                                            id,
                                            raw_output: accumulated.raw_output,
                                            content,
                                            is_error: matches!(
                                                initial_status,
                                                ToolCallStatus::Failed
                                            ),
                                        });
                                    }
                                }
                                SessionUpdate::ToolCallUpdate(update) => {
                                    let id = update.tool_call_id.0.to_string();
                                    // Merge patch-like fields; only emit on terminal status.
                                    let terminal_status = update.fields.status.filter(|s| {
                                        matches!(
                                            s,
                                            ToolCallStatus::Completed | ToolCallStatus::Failed
                                        )
                                    });
                                    let accumulated = if let Ok(mut buffer) =
                                        pending_tool_updates.lock()
                                    {
                                        let entry = buffer.entry(id.clone()).or_default();
                                        if let Some(raw_output) = update.fields.raw_output.clone() {
                                            entry.raw_output = Some(raw_output);
                                        }
                                        if let Some(content) = update.fields.content.clone() {
                                            entry.content.extend(content);
                                        }
                                        if terminal_status.is_some() {
                                            buffer.remove(&id)
                                        } else {
                                            None
                                        }
                                    } else {
                                        None
                                    };
                                    if let (Some(accumulated), Some(status)) =
                                        (accumulated, terminal_status)
                                    {
                                        let content = if accumulated.content.is_empty() {
                                            None
                                        } else {
                                            Some(accumulated.content)
                                        };
                                        let _ = tx.try_send(AcpUpdate::ToolCallComplete {
                                            id,
                                            raw_output: accumulated.raw_output,
                                            content,
                                            is_error: matches!(status, ToolCallStatus::Failed),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                        Ok(())
                    }
                },
                agent_client_protocol::on_receive_notification!(),
            )
            .on_receive_request(
                {
                    let prompt_response_tx = prompt_response_tx.clone();
                    async move |request: RequestPermissionRequest, responder, _connection_cx| {
                        let (response_tx, response_rx) = oneshot::channel();

                        let handler = prompt_response_tx
                            .lock()
                            .ok()
                            .as_ref()
                            .and_then(|g| g.as_ref().cloned());
                        let tx =
                            handler.ok_or_else(agent_client_protocol::Error::internal_error)?;

                        if tx.is_closed() {
                            return Err(agent_client_protocol::Error::internal_error());
                        }

                        tx.try_send(AcpUpdate::PermissionRequest {
                            request: Box::new(request),
                            response_tx,
                        })
                        .map_err(|_| agent_client_protocol::Error::internal_error())?;

                        let response = response_rx.await.unwrap_or_else(|_| {
                            RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
                        });
                        responder.respond(response)
                    }
                },
                agent_client_protocol::on_receive_request!(),
            )
            .connect_with(transport, async move |cx: ConnectionTo<Agent>| {
                handle_requests(
                    config,
                    goose_mode,
                    cx,
                    rx,
                    prompt_response_tx,
                    session_state,
                    init_tx,
                )
                .await
            })
            .await?;

        Ok(())
    }
}

/// Forwards an ACP child's stderr to tracing line by line.
///
/// Lines longer than `MAX_LINE_LEN` are flushed in chunks so a child that
/// emits unbounded output without newlines (e.g. carriage-return progress
/// bars or binary data) cannot cause unbounded memory growth.
async fn forward_child_stderr(mut stderr: tokio::process::ChildStderr) {
    const MAX_LINE_LEN: usize = 8192;
    const READ_CHUNK: usize = 1024;

    let mut line: Vec<u8> = Vec::with_capacity(256);
    let mut chunk = [0u8; READ_CHUNK];
    loop {
        match stderr.read(&mut chunk).await {
            Ok(0) => break,
            Ok(n) => {
                for &b in &chunk[..n] {
                    if b == b'\n' {
                        emit_stderr_line(&mut line);
                    } else {
                        line.push(b);
                        if line.len() >= MAX_LINE_LEN {
                            emit_stderr_line(&mut line);
                        }
                    }
                }
            }
            Err(e) => {
                tracing::debug!(target: "goose::acp::child::stderr", error = %e, "stderr read error");
                break;
            }
        }
    }
    emit_stderr_line(&mut line);
}

fn emit_stderr_line(line: &mut Vec<u8>) {
    if line.is_empty() {
        return;
    }
    let trimmed = line.strip_suffix(b"\r").unwrap_or(line);
    tracing::info!(target: "goose::acp::child::stderr", "{}", String::from_utf8_lossy(trimmed));
    line.clear();
}

async fn spawn_acp_process(config: &AcpProviderConfig) -> Result<Child> {
    let mut cmd = Command::new(&config.command);
    cmd.args(&config.args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);

    if let Some(command_dir) = config.command.parent() {
        // npm adapters commonly use `/usr/bin/env node`, while desktop PATH may omit their bin dir.
        let path = std::env::join_paths(
            std::iter::once(command_dir.to_path_buf()).chain(
                std::env::var_os("PATH")
                    .as_ref()
                    .map(std::env::split_paths)
                    .into_iter()
                    .flatten(),
            ),
        )?;
        cmd.env("PATH", path);
    }

    for key in &config.env_remove {
        cmd.env_remove(key);
    }

    for (key, value) in &config.env {
        cmd.env(key, value);
    }

    configure_subprocess(&mut cmd);
    cmd.spawn().context("failed to spawn ACP process")
}

fn log_undelivered<E: std::fmt::Debug>(result: Result<(), E>, method: &str) {
    if let Err(e) = result {
        tracing::debug!(method, error = ?e, "response not delivered");
    }
}

fn acp_method_error(method: &str, error: agent_client_protocol::Error) -> anyhow::Error {
    let message = format!("ACP {method} failed: {error}");
    anyhow::Error::new(error).context(message)
}

async fn handle_requests(
    config: AcpProviderConfig,
    goose_mode: Arc<Mutex<GooseMode>>,
    cx: ConnectionTo<Agent>,
    rx: &mut mpsc::Receiver<ClientRequest>,
    prompt_response_tx: Arc<Mutex<Option<mpsc::Sender<AcpUpdate>>>>,
    session_state: AcpSessionState,
    init_tx: oneshot::Sender<Result<InitializeResponse>>,
) -> Result<(), agent_client_protocol::Error> {
    let mut init_tx = Some(init_tx);

    let client_capabilities = ClientCapabilities::new();
    let init_response: InitializeResponse = cx
        .send_request(
            InitializeRequest::new(ProtocolVersion::V1).client_capabilities(client_capabilities),
        )
        .block_task()
        .await
        .map_err(|err| {
            let message = format!("ACP {} failed: {err}", AGENT_METHOD_NAMES.initialize);
            if let Some(tx) = init_tx.take() {
                let _ = tx.send(Err(anyhow::anyhow!(message.clone())));
            }
            agent_client_protocol::Error::internal_error().data(message)
        })?;

    let supports_close = init_response
        .agent_capabilities
        .session_capabilities
        .close
        .is_some();
    let supports_load = init_response.agent_capabilities.load_session;
    let mcp_capabilities = init_response.agent_capabilities.mcp_capabilities.clone();
    if let Some(tx) = init_tx.take() {
        log_undelivered(tx.send(Ok(init_response)), AGENT_METHOD_NAMES.initialize);
    }

    let mut session_ids: Vec<SessionId> = Vec::new();

    while let Some(request) = rx.recv().await {
        match request {
            ClientRequest::NewSession { response_tx } => {
                let mcp_servers = filter_supported_servers(&config.mcp_servers, &mcp_capabilities);
                let session = cx
                    .send_request(
                        NewSessionRequest::new(config.work_dir.clone()).mcp_servers(mcp_servers),
                    )
                    .block_task()
                    .await;
                let result = match session {
                    Ok(session) => {
                        *session_state.active_id.lock().unwrap() = Some(session.session_id.clone());
                        session_ids.push(session.session_id.clone());
                        if let Some(config_options) = session.config_options.as_deref() {
                            publish_effort_state(&session_state.effort, config_options);
                        }
                        apply_session_config_options(
                            &config,
                            &cx,
                            session.session_id.clone(),
                            &session_state.effort,
                        )
                        .await?;
                        apply_session_mode(&config, &goose_mode, &cx, session).await
                    }
                    Err(error) => Err(acp_method_error(AGENT_METHOD_NAMES.session_new, error)),
                };
                log_undelivered(response_tx.send(result), AGENT_METHOD_NAMES.session_new);
            }
            ClientRequest::LoadSession {
                session_id,
                response_tx,
            } => {
                let previous_session_id = session_state
                    .active_id
                    .lock()
                    .unwrap()
                    .replace(session_id.clone());
                let previous_effort_state = replace_effort_state(&session_state.effort, None);
                let result = if supports_load {
                    let mcp_servers =
                        filter_supported_servers(&config.mcp_servers, &mcp_capabilities);
                    cx.send_request(
                        LoadSessionRequest::new(session_id.clone(), config.work_dir.clone())
                            .mcp_servers(mcp_servers),
                    )
                    .block_task()
                    .await
                    .map(|response| {
                        NewSessionResponse::new(session_id.clone())
                            .modes(response.modes)
                            .config_options(response.config_options)
                            .meta(response.meta)
                    })
                    .map_err(anyhow::Error::from)
                } else {
                    Err(anyhow::anyhow!("ACP agent does not support session/load"))
                };
                let result = match result {
                    Ok(session) => {
                        session_ids.push(session.session_id.clone());
                        if let Some(config_options) = session.config_options.as_deref() {
                            publish_effort_state(&session_state.effort, config_options);
                        }
                        apply_session_config_options(
                            &config,
                            &cx,
                            session.session_id.clone(),
                            &session_state.effort,
                        )
                        .await?;
                        apply_session_mode(&config, &goose_mode, &cx, session).await
                    }
                    Err(error) => {
                        *session_state.active_id.lock().unwrap() = previous_session_id;
                        replace_effort_state(&session_state.effort, previous_effort_state);
                        Err(error)
                    }
                };
                log_undelivered(response_tx.send(result), AGENT_METHOD_NAMES.session_load);
            }
            ClientRequest::CloseSession { session_id } => {
                if supports_close
                    && let Err(error) = cx
                        .send_request(CloseSessionRequest::new(session_id.clone()))
                        .block_task()
                        .await
                {
                    tracing::debug!(method = AGENT_METHOD_NAMES.session_close, session_id = %session_id, %error, "failed to close replaced ACP session");
                }
                session_ids.retain(|id| id != &session_id);
            }
            ClientRequest::SetMode {
                session_id,
                mode_id,
                response_tx,
            } => {
                let result: Result<()> = cx
                    .send_request(SetSessionModeRequest::new(session_id, mode_id))
                    .block_task()
                    .await
                    .map(|_| ())
                    .map_err(anyhow::Error::from);
                log_undelivered(
                    response_tx.send(result),
                    AGENT_METHOD_NAMES.session_set_mode,
                );
            }
            ClientRequest::SetConfigOption {
                session_id,
                config_id,
                value,
                response_tx,
            } => {
                let value_id = agent_client_protocol::schema::v1::SessionConfigValueId::new(value);
                let req = SetSessionConfigOptionRequest::new(session_id, config_id, value_id);
                // The agent rebuilds per-model effort levels in this response,
                // so it is the freshest source after a goose-initiated switch.
                let result: Result<()> = cx
                    .send_request(req)
                    .block_task()
                    .await
                    .map(|response| {
                        publish_effort_state(&session_state.effort, &response.config_options)
                    })
                    .map_err(anyhow::Error::from);
                log_undelivered(
                    response_tx.send(result),
                    AGENT_METHOD_NAMES.session_set_config_option,
                );
            }
            ClientRequest::Prompt {
                session_id,
                content,
                response_tx,
            } => {
                *prompt_response_tx.lock().unwrap() = Some(response_tx.clone());

                let response: Result<PromptResponse, _> = cx
                    .send_request(PromptRequest::new(session_id, content))
                    .block_task()
                    .await;

                match response {
                    Ok(r) => {
                        log_undelivered(
                            response_tx.try_send(AcpUpdate::Complete(r.stop_reason, r.usage)),
                            AGENT_METHOD_NAMES.session_prompt,
                        );
                    }
                    Err(e) => {
                        log_undelivered(
                            response_tx.try_send(AcpUpdate::Error(e)),
                            AGENT_METHOD_NAMES.session_prompt,
                        );
                    }
                }

                *prompt_response_tx.lock().unwrap() = None;
            }
        }
    }

    if supports_close && !supports_load {
        for session_id in session_ids {
            if let Err(e) = cx
                .send_request(CloseSessionRequest::new(session_id.clone()))
                .block_task()
                .await
            {
                tracing::debug!(method = AGENT_METHOD_NAMES.session_close, session_id = %session_id, error = %e, "failed on shutdown");
            }
        }
    }

    Ok(())
}

async fn apply_session_config_options(
    config: &AcpProviderConfig,
    cx: &ConnectionTo<Agent>,
    session_id: SessionId,
    effort: &AcpEffortState,
) -> Result<()> {
    for (config_id, value) in &config.session_config_options {
        let value_id = agent_client_protocol::schema::v1::SessionConfigValueId::new(value.clone());
        let response = cx
            .send_request(SetSessionConfigOptionRequest::new(
                session_id.clone(),
                config_id.clone(),
                value_id,
            ))
            .block_task()
            .await
            .map_err(|err| {
                anyhow::anyhow!(
                    "ACP agent rejected {} for '{}': {err}",
                    AGENT_METHOD_NAMES.session_set_config_option,
                    config_id
                )
            })?;
        // Pinning the model here makes the agent rebuild its per-model effort
        // levels, so this response supersedes the session/new snapshot.
        publish_effort_state(effort, &response.config_options);
    }
    Ok(())
}

async fn apply_session_mode(
    config: &AcpProviderConfig,
    goose_mode: &Arc<Mutex<GooseMode>>,
    cx: &ConnectionTo<Agent>,
    session: NewSessionResponse,
) -> Result<NewSessionResponse> {
    let current_mode = goose_mode.lock().ok().map(|mode| *mode);
    let candidates = initial_mode_candidates(config, current_mode);

    if let Some(modes) = session.modes.as_ref()
        && !candidates.is_empty()
    {
        let Some(mode_id) = select_mode_id(&candidates, Some(modes)) else {
            let available: Vec<String> = modes
                .available_modes
                .iter()
                .map(|mode| mode.id.0.to_string())
                .collect();
            return Err(anyhow::anyhow!(
                "Requested mode(s) [{}] not offered by agent. Available modes: {}",
                candidates.join(", "),
                available.join(", ")
            ));
        };
        if modes.current_mode_id.0.as_ref() != mode_id.as_str() {
            let _: SetSessionModeResponse = cx
                .send_request(SetSessionModeRequest::new(
                    session.session_id.clone(),
                    mode_id,
                ))
                .block_task()
                .await
                .map_err(|err| {
                    anyhow::anyhow!(
                        "ACP agent rejected {}: {err}",
                        AGENT_METHOD_NAMES.session_set_mode
                    )
                })?;
        }
    }

    Ok(session)
}

fn initial_mode_candidates(
    config: &AcpProviderConfig,
    current_mode: Option<GooseMode>,
) -> Vec<String> {
    current_mode
        .and_then(|mode| config.mode_mapping.get(&mode).cloned())
        .or_else(|| config.session_mode_id.clone().map(|id| vec![id]))
        .unwrap_or_default()
}

fn select_mode_id(candidates: &[String], modes: Option<&SessionModeState>) -> Option<String> {
    match modes {
        Some(state) => candidates
            .iter()
            .find(|candidate| {
                state
                    .available_modes
                    .iter()
                    .any(|mode| mode.id.0.as_ref() == candidate.as_str())
            })
            .cloned(),
        None => candidates.first().cloned(),
    }
}

pub fn extension_configs_to_mcp_servers(configs: &[ExtensionConfig]) -> Vec<McpServer> {
    let mut servers = Vec::new();

    for config in configs {
        match config {
            ExtensionConfig::StreamableHttp {
                name,
                socket: Some(_),
                ..
            } => {
                tracing::debug!(
                    name,
                    "skipping socket-backed HTTP extension, unsupported by ACP"
                );
            }
            ExtensionConfig::StreamableHttp {
                name,
                uri,
                headers,
                socket: None,
                ..
            } => {
                let http_headers = headers
                    .iter()
                    .map(|(key, value)| HttpHeader::new(key, value))
                    .collect();
                servers.push(McpServer::Http(
                    McpServerHttp::new(name, uri).headers(http_headers),
                ));
            }
            ExtensionConfig::Stdio {
                name,
                cmd,
                args,
                envs,
                ..
            } => {
                let env_vars = envs
                    .get_env()
                    .into_iter()
                    .map(|(key, value)| EnvVariable::new(key, value))
                    .collect();

                servers.push(McpServer::Stdio(
                    McpServerStdio::new(name, cmd)
                        .args(args.clone())
                        .env(env_vars),
                ));
            }
            _ => {}
        }
    }

    servers
}

fn filter_supported_servers(
    servers: &[McpServer],
    capabilities: &McpCapabilities,
) -> Vec<McpServer> {
    servers
        .iter()
        .filter(|server| match server {
            McpServer::Http(http) => {
                if !capabilities.http {
                    tracing::debug!(
                        name = http.name,
                        "skipping HTTP server, agent lacks capability"
                    );
                    false
                } else {
                    true
                }
            }
            McpServer::Sse(sse) => {
                tracing::debug!(name = sse.name, "skipping SSE server, unsupported");
                false
            }
            _ => true,
        })
        .cloned()
        .collect()
}

fn messages_to_prompt(messages: &[Message], handoff_memo: Option<String>) -> Vec<ContentBlock> {
    let Some(last_user_index) = last_user_message_index(messages) else {
        return Vec::new();
    };

    let message = messages[last_user_index].agent_visible_content();
    let mut current_prompt_blocks = Vec::new();
    for content in &message.content {
        match content {
            MessageContent::Text(text) => {
                current_prompt_blocks.push(ContentBlock::Text(TextContent::new(text.text.clone())));
            }
            MessageContent::Image(image) => {
                current_prompt_blocks.push(ContentBlock::Image(ImageContent::new(
                    &image.data,
                    &image.mime_type,
                )));
            }
            _ => {}
        }
    }

    let Some(memo) = handoff_memo else {
        return current_prompt_blocks;
    };
    if current_prompt_blocks.is_empty() {
        return current_prompt_blocks;
    }

    let mut content_blocks = vec![ContentBlock::Text(TextContent::new(memo))];
    content_blocks.extend(current_prompt_blocks);
    content_blocks
}

fn last_user_message_index(messages: &[Message]) -> Option<usize> {
    messages
        .iter()
        .rposition(|m| m.role == Role::User && m.is_agent_visible() && !m.is_turn_context())
}

fn has_handoff_context(messages: &[Message]) -> bool {
    last_user_message_index(messages).is_some_and(|last_user_index| {
        messages[..last_user_index]
            .iter()
            .any(|m| m.is_agent_visible() && !m.is_turn_context())
    })
}

fn acp_audience_to_rmcp(annotations: Option<&AcpAnnotations>) -> Option<Vec<Role>> {
    let audience = annotations?.audience.as_ref()?;
    let audience = audience
        .iter()
        .filter_map(|role| match role {
            AcpRole::Assistant => Some(Role::Assistant),
            AcpRole::User => Some(Role::User),
            _ => None,
        })
        .collect::<Vec<_>>();

    if audience.is_empty() {
        None
    } else {
        Some(audience)
    }
}

fn acp_annotations_to_rmcp(annotations: Option<&AcpAnnotations>) -> rmcp::model::Annotations {
    let mut rmcp_annotations = rmcp::model::Annotations::default().with_priority(0.0);
    if let Some(audience) = acp_audience_to_rmcp(annotations) {
        rmcp_annotations = rmcp_annotations.with_audience(audience);
    }
    rmcp_annotations
}

fn acp_text_content_to_rmcp(text: TextContent) -> RmcpContent {
    RmcpContent::Text(
        rmcp::model::TextContent::new(sanitize_unicode_tags(&text.text))
            .with_annotations(acp_annotations_to_rmcp(text.annotations.as_ref())),
    )
}

fn acp_image_content_to_rmcp(image: ImageContent) -> RmcpContent {
    RmcpContent::Image(
        rmcp::model::ImageContent::new(image.data, image.mime_type)
            .with_annotations(acp_annotations_to_rmcp(image.annotations.as_ref())),
    )
}

fn visible_rmcp_text(text: impl Into<String>) -> RmcpContent {
    visible_rmcp_text_with_annotations(text, None)
}

fn visible_rmcp_text_with_annotations(
    text: impl Into<String>,
    annotations: Option<&AcpAnnotations>,
) -> RmcpContent {
    RmcpContent::Text(
        rmcp::model::TextContent::new(text).with_annotations(acp_annotations_to_rmcp(annotations)),
    )
}

fn acp_content_annotations(content: &ContentBlock) -> Option<&AcpAnnotations> {
    match content {
        ContentBlock::Text(content) => content.annotations.as_ref(),
        ContentBlock::Image(content) => content.annotations.as_ref(),
        ContentBlock::Audio(content) => content.annotations.as_ref(),
        ContentBlock::ResourceLink(content) => content.annotations.as_ref(),
        ContentBlock::Resource(content) => content.annotations.as_ref(),
        _ => None,
    }
}

fn acp_text_update_message(text: TextContent, id: String, created: i64) -> Message {
    Message::new(Role::Assistant, created, vec![])
        .with_content(acp_text_content_to_rmcp(text).into())
        .with_id(id)
}

/// Convert ACP `ToolCallContent` blocks into the rmcp `Content` shape goose's
/// `Message::with_tool_response` consumes. Handles `Content` (text/image/other),
/// `Diff`, and `Terminal` variants; falls back to a JSON serialization of
/// `raw_output` when no blocks are present so the renderer always has something.
fn acp_tool_call_content_to_rmcp(
    content: Option<Vec<ToolCallContent>>,
    raw_output: Option<serde_json::Value>,
) -> Vec<RmcpContent> {
    let mut out = Vec::new();
    if let Some(blocks) = content {
        for block in blocks {
            match block {
                ToolCallContent::Content(val) => match val.content {
                    ContentBlock::Text(text) => {
                        out.push(acp_text_content_to_rmcp(text));
                    }
                    ContentBlock::Image(image) => {
                        out.push(acp_image_content_to_rmcp(image));
                    }
                    other => {
                        if let Ok(json) = serde_json::to_string(&other) {
                            out.push(visible_rmcp_text_with_annotations(
                                json,
                                acp_content_annotations(&other),
                            ));
                        }
                    }
                },
                ToolCallContent::Diff(diff) => {
                    let path = diff.path.display();
                    let body = match diff.old_text.as_deref() {
                        Some(old) => {
                            format!("--- {path}\n{old}\n+++ {path}\n{}", diff.new_text)
                        }
                        None => format!("+++ {path}\n{}", diff.new_text),
                    };
                    out.push(visible_rmcp_text(body));
                }
                ToolCallContent::Terminal(_) => {}
                _ => {}
            }
        }
    }
    if out.is_empty()
        && let Some(raw) = raw_output
    {
        let text = match raw {
            serde_json::Value::String(s) => s,
            other => other.to_string(),
        };
        out.push(visible_rmcp_text(text));
    }
    out
}

fn build_action_required_message(request: &RequestPermissionRequest) -> Option<Message> {
    let tool_title = request
        .tool_call
        .fields
        .title
        .clone()
        .unwrap_or_else(|| "Tool".to_string());

    let arguments = request
        .tool_call
        .fields
        .raw_input
        .as_ref()
        .and_then(|v| v.as_object().cloned())
        .unwrap_or_default();

    let prompt = request
        .tool_call
        .fields
        .content
        .as_ref()
        .and_then(|content| {
            content.iter().find_map(|c| match c {
                ToolCallContent::Content(val) => match &val.content {
                    ContentBlock::Text(text) => Some(text.text.clone()),
                    _ => None,
                },
                _ => None,
            })
        });

    Some(
        Message::assistant()
            .with_action_required(
                request.tool_call.tool_call_id.0.to_string(),
                tool_title,
                arguments,
                prompt,
            )
            .user_only(),
    )
}

fn extract_model_info_from_config_options(
    config_options: &[SessionConfigOption],
) -> Option<(String, Vec<String>)> {
    let select = config_options.iter().find_map(|opt| {
        if opt.category.as_ref() != Some(&SessionConfigOptionCategory::Model) {
            return None;
        }
        match &opt.kind {
            SessionConfigKind::Select(select) => Some(select),
            _ => None,
        }
    })?;

    let current = select.current_value.0.to_string();
    let available = select_option_values(&select.options)
        .into_iter()
        .map(|option| option.value)
        .collect();
    Some((current, available))
}

fn resolve_model_info(
    provider_name: &str,
    response: &NewSessionResponse,
) -> Result<(String, Vec<String>), ProviderError> {
    if let Some(opts) = &response.config_options
        && let Some((current, available)) = extract_model_info_from_config_options(opts)
    {
        return Ok((current, available));
    }

    Err(ProviderError::RequestFailed(format!(
        "{provider_name}: agent returned no model config_options"
    )))
}

fn select_option_values(options: &SessionConfigSelectOptions) -> Vec<ThinkingEffortOption> {
    let effort_option = |option: &SessionConfigSelectOption| ThinkingEffortOption {
        value: option.value.0.to_string(),
        label: option.name.clone(),
    };
    match options {
        SessionConfigSelectOptions::Ungrouped(options) => {
            options.iter().map(effort_option).collect()
        }
        SessionConfigSelectOptions::Grouped(groups) => groups
            .iter()
            .flat_map(|group| group.options.iter().map(effort_option))
            .collect(),
        _ => Vec::new(),
    }
}

/// Find the agent's thinking-effort selector. Detection is by category so any
/// ACP agent advertising `thought_level` is supported without per-provider
/// config, with the well-known option id as a fallback.
fn extract_effort_capability(
    config_options: &[SessionConfigOption],
) -> Option<ThinkingEffortCapability> {
    let option = config_options
        .iter()
        .find(|opt| opt.category.as_ref() == Some(&SessionConfigOptionCategory::ThoughtLevel))
        .or_else(|| {
            config_options
                .iter()
                .find(|opt| opt.id.0.as_ref() == EFFORT_CONFIG_OPTION_ID)
        })?;
    let SessionConfigKind::Select(select) = &option.kind else {
        return None;
    };

    let values = select_option_values(&select.options);
    if values.is_empty() {
        return None;
    }
    Some(ThinkingEffortCapability {
        option_id: option.id.0.to_string(),
        values,
        current: Some(select.current_value.0.to_string()),
    })
}

/// Config-options payloads carry the agent's full set, so a payload without an
/// effort selector means the current model has none and the mirrored capability
/// must be dropped.
fn publish_effort_support(
    effort_updates: &watch::Sender<ThinkingEffortSupport>,
    support: ThinkingEffortSupport,
) {
    effort_updates.send_if_modified(|current| {
        if *current == support {
            false
        } else {
            *current = support;
            true
        }
    });
}

fn publish_effort_state(effort: &AcpEffortState, config_options: &[SessionConfigOption]) {
    let capability = extract_effort_capability(config_options);
    replace_effort_state(effort, capability);
}

fn replace_effort_state(
    effort: &AcpEffortState,
    capability: Option<ThinkingEffortCapability>,
) -> Option<ThinkingEffortCapability> {
    let mut state = effort.capability.lock().unwrap();
    let previous = mem::replace(&mut *state, capability.clone());
    publish_effort_support(
        &effort.updates,
        capability.map_or(
            ThinkingEffortSupport::Unsupported,
            ThinkingEffortSupport::Options,
        ),
    );
    previous
}

/// Map a goose effort value onto the agent's advertised vocabulary. Values goose
/// and the agent share pass through; goose's own enum values map onto their
/// closest agent equivalent. Anything else yields `None` so we never send a
/// value the agent would reject.
pub(super) fn map_effort_value(
    capability: &ThinkingEffortCapability,
    value: &str,
) -> Option<String> {
    let offered = |candidate: &str| {
        capability
            .values
            .iter()
            .find(|option| option.value.eq_ignore_ascii_case(candidate))
            .map(|option| option.value.clone())
    };

    offered(value).or_else(|| {
        let synonyms: &[&str] = match value.to_lowercase().as_str() {
            // Harnesses that always reason have no "off"; their default is the
            // closest thing to not forcing an effort level.
            "off" => &["default"],
            "max" => &["xhigh"],
            "xhigh" => &["max"],
            _ => &[],
        };
        synonyms.iter().find_map(|candidate| offered(candidate))
    })
}

/// Resolve the goose-side thinking effort to send to the agent: the session's
/// persisted pick wins, then the global default, each mapped into the agent's
/// vocabulary. `None` leaves the agent on its own current value. Shared with the
/// config menu so the advertised selection is the applied one — a persisted pick
/// the agent no longer offers (its selector was rebuilt by a model switch) falls
/// back to the global on both sides. The unmappable pick is deliberately left
/// persisted: it becomes honorable again if the user switches back.
pub(super) fn resolve_effort_value(
    capability: &ThinkingEffortCapability,
    model_config: &ModelConfig,
) -> Option<String> {
    model_config
        .request_param::<String>(THINKING_EFFORT_PARAM)
        .and_then(|value| map_effort_value(capability, &value))
        .or_else(|| {
            Config::global()
                .get_goose_thinking_effort()
                .and_then(|effort| map_effort_value(capability, &effort.to_string()))
        })
}

/// Separate a value the agent evaluated and refused from an operational failure.
/// Only an agent that actually processed the request answers with the JSON-RPC
/// `invalid_params` code; the client library synthesizes `internal_error` for a
/// dead subprocess or dropped connection, and goose's own send failures carry no
/// ACP error at all.
fn effort_option_error(value: &str, error: anyhow::Error) -> ProviderError {
    match error.downcast_ref::<agent_client_protocol::Error>() {
        Some(acp_error) if acp_error.code == agent_client_protocol::ErrorCode::InvalidParams => {
            ProviderError::InvalidValue(format!(
                "Agent rejected thinking effort '{value}': {acp_error}"
            ))
        }
        _ => ProviderError::RequestFailed(format!("Failed to set ACP effort option: {error}")),
    }
}

fn reverse_mode_mapping(
    mode_mapping: &HashMap<GooseMode, Vec<String>>,
) -> HashMap<String, Vec<GooseMode>> {
    let mut reverse: HashMap<String, Vec<GooseMode>> = HashMap::new();
    for (mode, ids) in mode_mapping {
        for id in ids {
            reverse.entry(id.clone()).or_default().push(*mode);
        }
    }
    reverse
}

fn resolve_mode(
    reverse_modes: &HashMap<String, Vec<GooseMode>>,
    mode_id: &str,
    current: &Arc<Mutex<GooseMode>>,
) -> Option<GooseMode> {
    let candidates = reverse_modes.get(mode_id)?;
    if candidates.len() == 1 {
        return Some(candidates[0]);
    }
    let current = current.lock().ok()?;
    if candidates.contains(&*current) {
        Some(*current)
    } else {
        Some(candidates[0])
    }
}

fn permission_decision_from_mode(goose_mode: GooseMode) -> Option<PermissionDecision> {
    match goose_mode {
        GooseMode::Auto => Some(PermissionDecision::AllowOnce),
        GooseMode::Chat => Some(PermissionDecision::RejectOnce),
        GooseMode::Approve | GooseMode::SmartApprove => None,
    }
}

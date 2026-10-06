use std::sync::atomic::{AtomicBool, Ordering};
use std::{collections::HashMap, sync::Arc};
use std::{fmt, time};

use super::tool_confirmation_coordinator::{
    ActiveTurnGuard, ConfirmationAnswer, ToolConfirmationCoordinator,
};
use super::tool_confirmation_router::ToolConfirmationRouter;
use super::tool_execution::{
    CHAT_MODE_TOOL_SKIPPED_RESPONSE, DECLINED_RESPONSE, ToolCallResult, ToolStream, ToolStreamItem,
    tool_stream,
};
use super::{
    container::Container, final_output_tool::FinalOutputTool, gen_ai_telemetry,
    mcp_client::GooseMcpHostInfo,
};
use crate::action_required_manager::ElicitationOutcome;
use crate::agents;
use crate::agents::extension::{ExtensionConfig, ExtensionResult};
use crate::agents::extension_manager::{ExtensionManager, ExtensionManagerCapabilities};
use crate::agents::final_output_tool::{
    FINAL_OUTPUT_CONTINUATION_MESSAGE, FINAL_OUTPUT_TOOL_NAME,
    structured_output_unsupported_message,
};
use crate::agents::retry::{RetryManager, RetryResult};
use crate::agents::state_machine::{
    BangShellOperation, CompactionOperation, DoctorOperation, EntryHookOperation,
    ExitOnErrorOperation, GooseEffect, GooseInferenceProvider, GooseInferenceRequestPreparer,
    MAX_TURNS_MESSAGE, MaxTurnsOperation, ProjectOperation, RecipeOperation, RetryOperation,
    SkillOperation, SlashCommandOperation, StatusOperation, SteerOperation, SteerQueue,
    StopHookOperation, ToolApprovalOperation, ToolExecutionOperation, ToolPairCompactionOperation,
    UnknownToolOperation, has_unapplied_tool_confirmation_response, pending_tool_confirmations,
    persist_tool_confirmation_decision, run_goose,
};
use crate::agents::types::{
    DEFAULT_ON_FAILURE_TIMEOUT_SECONDS, DEFAULT_RETRY_TIMEOUT_SECONDS, SessionConfig,
    SharedProvider,
};
use crate::agents::{large_response_handler, moim, tool_execution};
use crate::agents::{
    platform_extensions::MANAGE_EXTENSIONS_TOOL_NAME_COMPLETE, prompt_manager::PromptManager,
};
use crate::config::Config;
use crate::config::{extensions::name_to_key, permission::PermissionManager};
use crate::context_mgmt;
use crate::context_mgmt::{check_if_compaction_needed, compact_messages};
use crate::permission::{
    permission_inspector::PermissionInspector, permission_judge::PermissionCheckResult,
};
use crate::session::{EnabledExtensionsState, ExtensionState};
use crate::session::{Session, SessionManager, SessionNameUpdate};
use crate::{acp, context_limit, hooks, model_config, providers, session_context, sources};
use crate::{
    recipe::Response,
    scheduler_trait::SchedulerTrait,
    security::{
        adversary_inspector::AdversaryInspector, egress_inspector::EgressInspector,
        security_inspector::SecurityInspector,
    },
};
use crate::{
    tool_inspection::ToolInspectionManager, tool_monitor::RepetitionInspector,
    utils::is_token_cancelled,
};
use anyhow::{Context, Result, anyhow};
use bcaip_agent::events::AgentEvent;
use bcaip_agent::inference::InferenceRunner;
use bcaip_agent::inference::ends_with_successful_tool_response;
use bcaip_agent::machine::{StateMachine, Step};
use bcaip_agent::operation::{Emitter, Operation};
use bcaip_provider_types::base::{PermissionRouting, Provider};
use bcaip_provider_types::conversations::{
    ActionRequiredData, InferenceMetadata, Message, MessageContent, MessageUsage, ProviderMetadata,
    SystemNotificationType,
};
use bcaip_provider_types::conversations::{Conversation, debug_conversation_fix, fix_conversation};
use bcaip_provider_types::conversations::{ProviderUsage, Usage};
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::goose_mode::GooseMode;
use bcaip_provider_types::permission::{Permission, PermissionConfirmation};
use bcaip_provider_types::thinking::{ThinkingEffort, ThinkingEffortSupport};
use futures::stream::BoxStream;
use futures::{FutureExt, StreamExt, TryStreamExt, stream};
use goose_context_management::DEFAULT_COMPACTION_THRESHOLD;
use rmcp::model::{
    CallToolRequestParams, CallToolResult, ContentBlock, ElicitationAction, ErrorCode, ErrorData,
    GetPromptResult, Prompt, Tool,
};
use serde_json::Value;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, instrument, warn};
use tracing_futures::Instrument;

const DEFAULT_MAX_TURNS: u32 = 1000;
const DEFAULT_STOP_HOOK_BLOCK_CAP: u32 = 8;
const COMPACTION_PROGRESS_TEXT: &str = "goose is compacting the conversation...";
const MAX_EMPTY_TURN_RETRIES: u32 = 3;
const EMPTY_TURN_MESSAGE: &str =
    "The model returned an empty response. Please resend your message to continue.";

fn provider_creation_error(error: anyhow::Error, context: impl fmt::Display) -> anyhow::Error {
    let message = format!("{context}: {error}");
    error.context(message)
}

fn normalize_legacy_provider_thinking_effort(
    mut model_config: bcaip_provider_types::model::ModelConfig,
    effort_support: &ThinkingEffortSupport,
) -> bcaip_provider_types::model::ModelConfig {
    let has_raw_effort = model_config
        .request_params
        .as_ref()
        .is_some_and(|params| params.contains_key("thinking_effort"));
    if !matches!(effort_support, ThinkingEffortSupport::Unspecified)
        || !has_raw_effort
        || model_config.thinking_effort().is_some()
    {
        return model_config;
    }

    if let Some(params) = model_config.request_params.as_mut() {
        params.remove("thinking_effort");
    }
    model_config.with_default_thinking_effort(Config::global().get_goose_thinking_effort())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ToolCategory {
    Shell,
    Read,
    Write,
    Other,
}

fn categorize_tool(tool_name: &str) -> ToolCategory {
    let local = tool_name.rsplit("__").next().unwrap_or(tool_name);
    match local {
        "shell" | "bash" | "exec" | "run" => ToolCategory::Shell,
        "read" | "view" | "cat" | "read_file" => ToolCategory::Read,
        "write" | "edit" | "patch" | "write_file" | "edit_file" => ToolCategory::Write,
        _ => ToolCategory::Other,
    }
}

fn extract_string_arg(input: &Value, keys: &[&str]) -> Option<String> {
    let obj = input.as_object()?;
    for k in keys {
        if let Some(s) = obj.get(*k).and_then(|v| v.as_str())
            && !s.is_empty()
        {
            return Some(s.to_string());
        }
    }
    None
}

pub(crate) fn stop_hook_denial_context_message(plugin: &str, reason: &str) -> Message {
    let nudge = format!(
        "Stop hook `{plugin}` blocked ending this turn:

{reason}

Address this policy hook denial before trying to stop again."
    );
    Message::user()
        .with_text(nudge)
        .with_visibility(false, true)
}

pub(crate) fn stop_hook_denial_notification(plugin: &str) -> Message {
    Message::assistant().with_system_notification(
        SystemNotificationType::InlineMessage,
        format!("Stop hook `{plugin}` blocked ending this turn."),
    )
}

pub(crate) fn stop_hook_block_cap_warning(plugin: &str, cap: u32) -> Message {
    Message::assistant().with_system_notification(
        SystemNotificationType::InlineMessage,
        format!(
            "Stop hook `{plugin}` blocked the turn from ending more than {cap} consecutive times — overriding and ending turn to avoid an infinite loop. Set GOOSE_STOP_HOOK_BLOCK_CAP to raise this limit."
        ),
    )
}

/// Context needed for the reply function
pub struct ReplyContext {
    pub conversation: Conversation,
    pub tools: Vec<Tool>,
    pub toolshim_tools: Vec<Tool>,
    pub system_prompt: String,
    pub goose_mode: GooseMode,
    pub tool_call_cut_off: usize,
    pub model_config: bcaip_provider_types::model::ModelConfig,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct ExtensionLoadResult {
    pub name: String,
    pub success: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub enum GoosePlatform {
    GooseDesktop,
    GooseCli,
}

impl fmt::Display for GoosePlatform {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            GoosePlatform::GooseCli => write!(f, "goose-cli"),
            GoosePlatform::GooseDesktop => write!(f, "goose-desktop"),
        }
    }
}

#[derive(Clone)]
pub struct AgentConfig {
    pub session_manager: Arc<SessionManager>,
    pub permission_manager: Arc<PermissionManager>,
    pub scheduler_service: Option<Arc<dyn SchedulerTrait>>,
    pub goose_mode: GooseMode,
    pub disable_session_naming: bool,
    pub goose_platform: GoosePlatform,
    pub mcp_host_info: Option<GooseMcpHostInfo>,
    pub elicitation_handler: Option<agents::mcp_client::ElicitationHandler>,
    pub mcp_protocol_version: Option<rmcp::model::ProtocolVersion>,
    pub session_name_update_tx: Option<mpsc::UnboundedSender<SessionNameUpdate>>,
    pub use_login_shell_path: Option<bool>,
    pub is_subagent: bool,
}

impl AgentConfig {
    pub fn new(
        session_manager: Arc<SessionManager>,
        permission_manager: Arc<PermissionManager>,
        scheduler_service: Option<Arc<dyn SchedulerTrait>>,
        goose_mode: GooseMode,
        disable_session_naming: bool,
        goose_platform: GoosePlatform,
    ) -> Self {
        Self {
            session_manager,
            permission_manager,
            scheduler_service,
            goose_mode,
            disable_session_naming,
            goose_platform,
            mcp_host_info: None,
            elicitation_handler: None,
            mcp_protocol_version: None,
            session_name_update_tx: None,
            use_login_shell_path: None,
            is_subagent: false,
        }
    }

    pub fn with_mcp_host_info(mut self, mcp_host_info: Option<GooseMcpHostInfo>) -> Self {
        self.mcp_host_info = mcp_host_info;
        self
    }

    pub fn with_session_name_update_tx(
        mut self,
        tx: Option<mpsc::UnboundedSender<SessionNameUpdate>>,
    ) -> Self {
        self.session_name_update_tx = tx;
        self
    }

    pub fn with_use_login_shell_path(mut self, use_login_shell_path: bool) -> Self {
        self.use_login_shell_path = Some(use_login_shell_path);
        self
    }

    fn resolve_use_login_shell_path(&self) -> bool {
        resolve_use_login_shell_path(self.use_login_shell_path, &self.goose_platform)
    }
}

fn resolve_use_login_shell_path(explicit: Option<bool>, platform: &GoosePlatform) -> bool {
    explicit.unwrap_or(matches!(platform, GoosePlatform::GooseDesktop))
}

/// The main goose Agent
pub struct Agent {
    pub(super) provider: SharedProvider,
    pub config: AgentConfig,
    pub(super) current_goose_mode: Mutex<GooseMode>,

    pub extension_manager: Arc<ExtensionManager>,
    pub(super) final_output_tool: Arc<Mutex<Option<FinalOutputTool>>>,
    pub(super) prompt_manager: Mutex<PromptManager>,
    pub(super) tool_confirmation_router: ToolConfirmationRouter,
    tool_confirmation_coordinator: ToolConfirmationCoordinator,

    pub(super) retry_manager: RetryManager,
    pub(super) tool_inspection_manager: ToolInspectionManager,
    pub(super) hook_manager: crate::hooks::HookManager,
    session_start_emitted: AtomicBool,
    container: Mutex<Option<Container>>,
    pub(super) goal: Mutex<Option<String>>,
    pub(super) grind: Mutex<Option<String>>,
    steer_queues: Mutex<HashMap<String, SteerQueue>>,
}

fn ensure_message_event_id(event: AgentEvent) -> AgentEvent {
    match event {
        AgentEvent::Message(message) => AgentEvent::Message(message.with_generated_id_if_missing()),
        other => other,
    }
}

fn push_message_with_id(messages: &mut Conversation, message: Message) -> Message {
    let message = message.with_generated_id_if_missing();
    messages.push(message.clone());
    message
}

async fn persist_message_with_id(
    session_manager: &SessionManager,
    session_id: &str,
    message: Message,
) -> Result<Message> {
    let message = message.with_generated_id_if_missing();
    session_manager.add_message(session_id, &message).await?;
    Ok(message)
}

async fn persist_and_push_message_with_id(
    session_manager: &SessionManager,
    session_id: &str,
    conversation: &mut Conversation,
    message: Message,
) -> Result<Message> {
    let message = persist_message_with_id(session_manager, session_id, message).await?;
    conversation.push(message.clone());
    Ok(message)
}

fn project_message_for_user_event(message: &Message) -> Message {
    message.user_visible_content()
}

fn agent_visible_message_text(message: &Message) -> String {
    message.agent_visible_content().as_concat_text()
}

fn attach_turn_usage(
    messages: &mut Conversation,
    usage: &ProviderUsage,
    preferred_message_id: Option<&str>,
) -> Option<(Option<String>, MessageUsage)> {
    let message_index = preferred_message_id
        .and_then(|preferred_message_id| {
            messages.messages().iter().rposition(|message| {
                message.role == rmcp::model::Role::Assistant
                    && message.id.as_deref() == Some(preferred_message_id)
            })
        })
        .or_else(|| {
            messages
                .messages()
                .iter()
                .rposition(|message| message.role == rmcp::model::Role::Assistant)
        })?;
    let message = &mut messages.messages_mut()[message_index];
    let has_user_visible_content = !message.user_visible_content().content.is_empty();
    let message_usage = MessageUsage::from_provider_usage(usage, false);
    message.metadata.usage = Some(Box::new(message_usage.clone()));
    has_user_visible_content.then(|| (message.id.clone(), message_usage))
}

impl Default for Agent {
    fn default() -> Self {
        Self::new()
    }
}

fn has_unique_persisted_extension(configs: &[ExtensionConfig], key: &str) -> Result<bool> {
    match configs
        .iter()
        .filter(|config| config.key() == key)
        .take(2)
        .count()
    {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(anyhow!("Duplicate session extension key '{key}'")),
    }
}

impl Agent {
    pub fn new() -> Self {
        let config = Config::global();
        Self::with_config(AgentConfig::new(
            Arc::new(SessionManager::instance()),
            PermissionManager::instance(),
            None,
            config.get_goose_mode().unwrap_or_default(),
            config.get_goose_disable_session_naming().unwrap_or(false),
            GoosePlatform::GooseCli,
        ))
    }

    pub fn with_config(config: AgentConfig) -> Self {
        let provider = Arc::new(Mutex::new(None));

        let goose_platform = config.goose_platform.clone();
        let initial_mode = config.goose_mode;
        let explicit_mcp_host_info = config.mcp_host_info.clone();
        let mcpui = explicit_mcp_host_info
            .as_ref()
            .filter(|host_info| host_info.explicit_extensions)
            .map(GooseMcpHostInfo::mcpui_enabled)
            .unwrap_or_else(|| match config.goose_platform {
                GoosePlatform::GooseDesktop => true,
                GoosePlatform::GooseCli => false,
            });
        let capabilities = ExtensionManagerCapabilities {
            mcpui,
            host_info: explicit_mcp_host_info.clone(),
            elicitation_handler: config.elicitation_handler.clone(),
            protocol_version: config.mcp_protocol_version.clone(),
        };
        let client_name = explicit_mcp_host_info
            .as_ref()
            .and_then(|host_info| host_info.client_name.clone())
            .unwrap_or_else(|| goose_platform.to_string());
        let session_manager = Arc::clone(&config.session_manager);
        let scheduler = config.scheduler_service.clone();
        let inspection_session_manager = Arc::clone(&config.session_manager);
        let permission_manager = Arc::clone(&config.permission_manager);
        let use_login_shell_path = config.resolve_use_login_shell_path();
        let is_subagent = config.is_subagent;
        Self {
            provider: provider.clone(),
            config,
            current_goose_mode: Mutex::new(initial_mode),
            extension_manager: Arc::new(ExtensionManager::new(
                provider.clone(),
                session_manager,
                scheduler,
                client_name,
                capabilities,
                use_login_shell_path,
            )),
            final_output_tool: Arc::new(Mutex::new(None)),
            prompt_manager: Mutex::new(PromptManager::new()),
            tool_confirmation_router: ToolConfirmationRouter::new(),
            tool_confirmation_coordinator: ToolConfirmationCoordinator::new(),
            retry_manager: RetryManager::new(),
            tool_inspection_manager: Self::create_tool_inspection_manager(
                permission_manager,
                provider.clone(),
                inspection_session_manager,
            ),
            hook_manager: if is_subagent {
                hooks::HookManager::default()
            } else {
                hooks::HookManager::load(
                    std::env::current_dir().ok().as_deref(),
                    use_login_shell_path,
                )
            },
            session_start_emitted: AtomicBool::new(false),
            container: Mutex::new(None),
            goal: Mutex::new(None),
            grind: Mutex::new(None),
            steer_queues: Mutex::new(HashMap::new()),
        }
    }

    /// Emit a lifecycle hook event with no extra context. Useful for events that have no matcher (e.g. `SessionStart`, `SessionEnd`).
    pub(crate) fn stop_hook_block_cap(&self) -> u32 {
        Config::global()
            .get_param::<u32>("GOOSE_STOP_HOOK_BLOCK_CAP")
            .unwrap_or(DEFAULT_STOP_HOOK_BLOCK_CAP)
    }

    pub async fn emit_hook(&self, event: hooks::HookEvent, session_id: &str) {
        if !self.hook_manager.has_hooks(event) {
            return;
        }
        self.hook_manager
            .emit(event, hooks::HookContext::new(event, session_id))
            .await;
    }

    pub async fn emit_hook_with_banners(
        &self,
        event: hooks::HookEvent,
        session_id: &str,
    ) -> Vec<String> {
        if event == hooks::HookEvent::SessionStart {
            self.session_start_emitted.store(true, Ordering::Release);
        }
        if !self.hook_manager.has_hooks(event) {
            return Vec::new();
        }
        self.hook_manager
            .emit_collecting_banners(event, hooks::HookContext::new(event, session_id))
            .await
    }

    fn stop_hook_context(
        session_id: &str,
        last_assistant_message: &str,
        working_dir: &str,
    ) -> hooks::HookContext {
        hooks::HookContext::new(hooks::HookEvent::Stop, session_id)
            .with_last_assistant_message(last_assistant_message.to_string())
            .with_working_dir(working_dir.to_string())
    }

    pub(crate) async fn emit_stop_hook(
        &self,
        session_id: &str,
        last_assistant_message: &str,
        working_dir: &str,
    ) {
        if !self.hook_manager.has_hooks(hooks::HookEvent::Stop) {
            return;
        }
        self.hook_manager
            .emit(
                hooks::HookEvent::Stop,
                Self::stop_hook_context(session_id, last_assistant_message, working_dir),
            )
            .await;
    }

    pub(crate) async fn emit_stop_hook_blocking(
        &self,
        session_id: &str,
        last_assistant_message: &str,
        working_dir: &str,
    ) -> crate::hooks::HookDecision {
        self.hook_manager
            .emit_blocking(
                hooks::HookEvent::Stop,
                Self::stop_hook_context(session_id, last_assistant_message, working_dir),
            )
            .await
    }

    pub async fn steer(&self, session_id: &str, message: Message) {
        self.steer_queue(session_id)
            .await
            .lock()
            .await
            .push_back(message);
    }

    pub async fn discard_pending_steers(&self, session_id: &str) {
        self.steer_queues.lock().await.remove(session_id);
    }

    pub(crate) async fn has_pending_steers(&self, session_id: &str) -> bool {
        let queue = self.steer_queues.lock().await.get(session_id).cloned();
        match queue {
            Some(queue) => !queue.lock().await.is_empty(),
            None => false,
        }
    }

    pub(crate) async fn drain_pending_steers(&self, session_id: &str) -> Vec<Message> {
        let queue = self.steer_queues.lock().await.get(session_id).cloned();
        match queue {
            Some(queue) => queue
                .lock()
                .await
                .drain(..)
                .map(Message::with_steer)
                .collect(),
            None => Vec::new(),
        }
    }

    async fn steer_queue(&self, session_id: &str) -> SteerQueue {
        self.steer_queues
            .lock()
            .await
            .entry(session_id.to_string())
            .or_default()
            .clone()
    }

    async fn emit_pre_tool_extended_hooks(
        &self,
        tool_name: &str,
        tool_input: Option<&Value>,
        session: &Session,
    ) {
        let working_dir = session.working_dir.to_string_lossy().to_string();
        match categorize_tool(tool_name) {
            ToolCategory::Shell => {
                if let Some(cmd) = tool_input.and_then(|v| extract_string_arg(v, &["command"])) {
                    self.emit_with_matcher(
                        hooks::HookEvent::BeforeShellExecution,
                        &session.id,
                        &cmd,
                        tool_name,
                        tool_input.cloned(),
                        &working_dir,
                    )
                    .await;
                }
            }
            ToolCategory::Read => {
                if let Some(path) =
                    tool_input.and_then(|v| extract_string_arg(v, &["path", "file", "file_path"]))
                {
                    self.emit_with_matcher(
                        hooks::HookEvent::BeforeReadFile,
                        &session.id,
                        &path,
                        tool_name,
                        tool_input.cloned(),
                        &working_dir,
                    )
                    .await;
                }
            }
            ToolCategory::Write | ToolCategory::Other => {}
        }
    }

    async fn emit_with_matcher(
        &self,
        event: hooks::HookEvent,
        session_id: &str,
        matcher_context: &str,
        tool_name: &str,
        tool_input: Option<Value>,
        working_dir: &str,
    ) {
        if !self.hook_manager.has_hooks(event) {
            return;
        }
        let mut ctx = crate::hooks::HookContext::new(event, session_id)
            .with_tool(tool_name.to_string(), tool_input)
            .with_working_dir(working_dir.to_string());
        ctx.matcher_context = Some(matcher_context.to_string());
        self.hook_manager.emit(event, ctx).await;
    }

    /// Observation-only record of what the `PreToolUse` chain decided. Carries
    /// no veto: the decision has already been made by the time this runs.
    async fn emit_pre_tool_use_result(
        &self,
        session: &Session,
        tool_call_id: &str,
        tool_name: &str,
        tool_input: Option<&Value>,
        outcome: &hooks::HookChainOutcome,
    ) {
        if !self
            .hook_manager
            .has_hooks(hooks::HookEvent::PreToolUseResult)
        {
            return;
        }
        let ctx = hooks::HookContext::new(hooks::HookEvent::PreToolUseResult, &session.id)
            .with_tool(tool_name.to_string(), tool_input.cloned())
            .with_tool_call_id(tool_call_id)
            .with_working_dir(session.working_dir.to_string_lossy().to_string())
            .with_pre_tool_use_outcome(outcome);
        self.hook_manager.emit_pre_tool_use_result(ctx).await;
    }

    fn with_post_tool_hook(
        &self,
        result: ToolCallResult,
        tool_call: &CallToolRequestParams,
        session: &Session,
        tool_call_id: &str,
    ) -> ToolCallResult {
        let hook_manager = self.hook_manager.clone();
        let session_id = session.id.clone();
        let working_dir = session.working_dir.to_string_lossy().to_string();
        let tool_name = tool_call.name.to_string();
        let tool_call_id = tool_call_id.to_string();
        let tool_input = tool_call
            .arguments
            .as_ref()
            .map(|a| serde_json::Value::Object(a.clone()));
        let category = categorize_tool(&tool_name);
        let span = tracing::Span::current();
        let capture_message_content = gen_ai_telemetry::capture_message_content();

        let fut = async move {
            let processed_result =
                large_response_handler::process_tool_response(result.result.await);
            if capture_message_content {
                let output = gen_ai_telemetry::tool_result_json(&processed_result);
                span.record("output", output.as_str());
            }
            gen_ai_telemetry::record_tool_result(&span, &processed_result);
            let event = match &processed_result {
                Ok(call_result) if call_result.is_error != Some(true) => {
                    hooks::HookEvent::PostToolUse
                }
                _ => hooks::HookEvent::PostToolUseFailure,
            };

            if hook_manager.has_hooks(event) {
                let ctx = crate::hooks::HookContext::new(event, &session_id)
                    .with_tool(tool_name.clone(), tool_input.clone())
                    .with_tool_call_id(tool_call_id.as_str())
                    .with_working_dir(working_dir.clone());
                hook_manager.emit(event, ctx).await;
            }

            if event == hooks::HookEvent::PostToolUse {
                let extended = match category {
                    ToolCategory::Shell => Some((
                        hooks::HookEvent::AfterShellExecution,
                        tool_input
                            .as_ref()
                            .and_then(|v| extract_string_arg(v, &["command"])),
                    )),
                    ToolCategory::Write => Some((
                        hooks::HookEvent::AfterFileEdit,
                        tool_input
                            .as_ref()
                            .and_then(|v| extract_string_arg(v, &["path", "file", "file_path"])),
                    )),
                    _ => None,
                };
                if let Some((ext_event, Some(matcher))) = extended
                    && hook_manager.has_hooks(ext_event)
                {
                    let mut ctx = hooks::HookContext::new(ext_event, &session_id)
                        .with_tool(tool_name, tool_input)
                        .with_working_dir(working_dir);
                    ctx.matcher_context = Some(matcher);
                    hook_manager.emit(ext_event, ctx).await;
                }
            }

            processed_result
        };

        ToolCallResult {
            notification_stream: result.notification_stream,
            action_required_stream: result.action_required_stream,
            result: Box::new(fut.boxed()),
        }
    }

    /// Create a tool inspection manager with default inspectors
    fn create_tool_inspection_manager(
        permission_manager: Arc<PermissionManager>,
        provider: SharedProvider,
        session_manager: Arc<SessionManager>,
    ) -> ToolInspectionManager {
        let mut tool_inspection_manager = ToolInspectionManager::new();

        // Add security inspector (highest priority - runs first)
        tool_inspection_manager.add_inspector(Box::new(SecurityInspector::new()));
        tool_inspection_manager.add_inspector(Box::new(EgressInspector::new()));

        // Add adversary inspector (LLM-based review, enabled by ~/.config/goose/adversary.md)
        tool_inspection_manager.add_inspector(Box::new(AdversaryInspector::new(
            provider.clone(),
            session_manager.clone(),
        )));

        // Add permission inspector (medium-high priority)
        tool_inspection_manager.add_inspector(Box::new(PermissionInspector::new(
            permission_manager,
            provider,
            session_manager,
        )));

        // Add repetition inspector (lower priority - basic repetition checking)
        tool_inspection_manager.add_inspector(Box::new(RepetitionInspector::new(None)));

        tool_inspection_manager
    }

    /// Reset the retry attempts counter to 0
    pub async fn reset_retry_attempts(&self) {
        self.retry_manager.reset_attempts().await;
    }

    /// Increment the retry attempts counter and return the new value
    pub async fn increment_retry_attempts(&self) -> u32 {
        self.retry_manager.increment_attempts().await
    }

    /// Get the current retry attempts count
    pub async fn get_retry_attempts(&self) -> u32 {
        self.retry_manager.get_attempts().await
    }

    async fn handle_retry_logic(
        &self,
        messages: &mut Conversation,
        session_config: &SessionConfig,
        initial_messages: &[Message],
    ) -> Result<RetryResult> {
        let result = self
            .retry_manager
            .handle_retry_logic(messages, session_config, initial_messages)
            .await?;
        if matches!(result, RetryResult::Retried)
            && let Some(tool) = self.final_output_tool.lock().await.as_mut()
        {
            tool.final_output = None;
        }
        Ok(result)
    }
    async fn load_project_instructions(&self, session: &Session) -> Option<String> {
        let project_id = session.project_id.as_deref()?;
        let entry = sources::read_project(project_id).ok()?;
        let mut parts = Vec::new();
        parts.push(format!("# Project: {}", entry.name));
        if !entry.description.is_empty() {
            parts.push(entry.description.clone());
        }
        if !entry.content.is_empty() {
            parts.push(entry.content.clone());
        }
        Some(parts.join("\n\n"))
    }

    async fn prepare_reply_context(
        &self,
        session_id: &str,
        unfixed_conversation: Conversation,
        working_dir: &std::path::Path,
    ) -> Result<ReplyContext> {
        let unfixed_messages = unfixed_conversation.messages().clone();
        let (conversation, issues) = fix_conversation(unfixed_conversation.clone());
        if !issues.is_empty() {
            debug!(
                "Conversation issue fixed: {}",
                debug_conversation_fix(
                    unfixed_messages.as_slice(),
                    conversation.messages(),
                    &issues
                )
            );
        }
        let (tools, toolshim_tools, system_prompt, model_config) = self
            .prepare_tools_and_prompt(session_id, working_dir)
            .await?;

        let goose_mode = *self.current_goose_mode.lock().await;

        let tool_call_cut_off = match Config::global().get_param::<usize>("GOOSE_TOOL_CALL_CUTOFF")
        {
            Ok(v) => v,
            Err(_) => {
                let context_limit = match self.provider().await {
                    Ok(provider) => context_limit::get_context_limit(
                        provider.as_ref(),
                        &model_config.model_name,
                    )
                    .await
                    .unwrap_or(bcaip_provider_types::model::DEFAULT_CONTEXT_LIMIT),
                    Err(_) => bcaip_provider_types::model::DEFAULT_CONTEXT_LIMIT,
                };
                let compaction_threshold = Config::global()
                    .get_param::<f64>("GOOSE_AUTO_COMPACT_THRESHOLD")
                    .unwrap_or(goose_context_management::DEFAULT_COMPACTION_THRESHOLD);
                context_mgmt::compute_tool_call_cutoff(context_limit, compaction_threshold)
            }
        };

        Ok(ReplyContext {
            conversation,
            tools,
            toolshim_tools,
            system_prompt,
            goose_mode,
            tool_call_cut_off,
            model_config,
        })
    }

    async fn handle_approved_and_denied_tools(
        &self,
        permission_check_result: &PermissionCheckResult,
        request_to_response_map: &mut HashMap<String, Message>,
        cancel_token: Option<tokio_util::sync::CancellationToken>,
        session: &Session,
    ) -> Result<Vec<(String, ToolStream)>> {
        let mut tool_futures: Vec<(String, ToolStream)> = Vec::new();

        // Handle pre-approved and read-only tools
        for request in &permission_check_result.approved {
            if let Ok(tool_call) = request.tool_call.clone() {
                let (req_id, tool_result) = self
                    .dispatch_tool_call(
                        tool_call,
                        request.id.clone(),
                        cancel_token.clone(),
                        session,
                    )
                    .await;

                tool_futures.push((
                    req_id,
                    match tool_result {
                        Ok(result) => tool_stream(
                            result
                                .notification_stream
                                .unwrap_or_else(|| Box::new(stream::empty())),
                            result
                                .action_required_stream
                                .unwrap_or_else(|| Box::new(stream::empty())),
                            result.result,
                        ),
                        Err(e) => tool_stream(
                            Box::new(stream::empty()),
                            Box::new(stream::empty()),
                            futures::future::ready(Err(e)),
                        ),
                    },
                ));
            }
        }

        Self::handle_denied_tools(permission_check_result, request_to_response_map);
        Ok(tool_futures)
    }

    fn handle_denied_tools(
        permission_check_result: &PermissionCheckResult,
        request_to_response_map: &mut HashMap<String, Message>,
    ) {
        for request in &permission_check_result.denied {
            if let Some(response) = request_to_response_map.get_mut(&request.id) {
                response.add_tool_response_with_metadata(
                    request.id.clone(),
                    Ok(CallToolResult::error(vec![
                        rmcp::model::ContentBlock::text(DECLINED_RESPONSE),
                    ])),
                    request.metadata.as_ref(),
                );
            }
        }
    }

    /// Get a reference count clone to the provider
    pub async fn provider(&self) -> Result<Arc<dyn Provider>, anyhow::Error> {
        match &*self.provider.lock().await {
            Some(provider) => Ok(Arc::clone(provider)),
            None => Err(anyhow!("Provider not set")),
        }
    }

    /// Resolve the active model config for a session.
    ///
    /// The session is the source of truth for the selected model and its
    /// settings. When the session has no stored config (e.g. before the
    /// provider has been persisted), fall back to the configured provider
    /// defaults.
    pub async fn model_config_for_session(
        &self,
        session_id: &str,
    ) -> Result<bcaip_provider_types::model::ModelConfig> {
        if let Ok(session) = self
            .config
            .session_manager
            .get_session(session_id, false)
            .await
            && let Some(model_config) = session.model_config
        {
            return Ok(model_config);
        }

        let config = Config::global();
        let provider_name = config
            .get_goose_provider()
            .map_err(|_| anyhow!("Could not resolve model config: missing provider"))?;
        let model_name = config
            .get_goose_model()
            .map_err(|_| anyhow!("Could not resolve model config: missing model"))?;
        model_config::model_config_from_user_config(&provider_name, &model_name)
            .map_err(|e| anyhow!("Could not resolve model config: {e}"))
    }

    pub(super) async fn effective_model_config_for_session(
        &self,
        session_id: &str,
    ) -> Result<bcaip_provider_types::model::ModelConfig> {
        let model_config = self.model_config_for_session(session_id).await?;
        let provider_name = self.provider().await?.get_name().to_string();
        match providers::get_from_registry(&provider_name).await {
            Ok(entry) => Ok(entry
                .normalize_model_config(model_config.clone())
                .unwrap_or(model_config)),
            Err(_) => Ok(model_config),
        }
    }

    /// When set, all stdio extensions will be started via `docker exec` in the specified container.
    pub async fn set_container(&self, container: Option<Container>) {
        *self.container.lock().await = container.clone();
    }

    pub async fn container(&self) -> Option<Container> {
        self.container.lock().await.clone()
    }

    pub async fn add_final_output_tool(&self, response: Response) -> Result<()> {
        let mut final_output_tool = self.final_output_tool.lock().await;
        let created_final_output_tool =
            FinalOutputTool::try_new(response).map_err(anyhow::Error::msg)?;
        let final_output_system_prompt = created_final_output_tool.system_prompt();
        *final_output_tool = Some(created_final_output_tool);
        self.extend_system_prompt("final_output".to_string(), final_output_system_prompt)
            .await;
        Ok(())
    }

    pub async fn apply_recipe_components(
        &self,
        response: Option<Response>,
        include_final_output: bool,
    ) -> Result<()> {
        if include_final_output && let Some(response) = response {
            self.add_final_output_tool(response).await?;
        }
        Ok(())
    }

    /// Dispatch a single tool call to the appropriate client
    #[instrument(
        skip(self, tool_call, request_id, cancellation_token, session),
        fields(
            input,
            output,
            session.id = %session.id,
            gen_ai.conversation.id = %session.id,
            gen_ai.operation.name = "execute_tool",
            gen_ai.tool.name = %tool_call.name,
            gen_ai.tool.call.id = %request_id,
            gen_ai.tool.call.arguments = tracing::field::Empty,
            gen_ai.tool.call.result = tracing::field::Empty,
            error.type = tracing::field::Empty,
        )
    )]
    pub async fn dispatch_tool_call(
        &self,
        tool_call: CallToolRequestParams,
        request_id: String,
        cancellation_token: Option<CancellationToken>,
        session: &Session,
    ) -> (String, Result<ToolCallResult, ErrorData>) {
        if gen_ai_telemetry::capture_message_content() {
            let input_summary = serde_json::json!({
                "tool": tool_call.name,
                "arguments": tool_call.arguments,
            });
            tracing::Span::current().record("input", tracing::field::display(&input_summary));
        }
        gen_ai_telemetry::record_tool_arguments(&tracing::Span::current(), &tool_call);

        self.prompt_manager
            .lock()
            .await
            .record_tool_arguments(&tool_call.arguments, &session.working_dir);

        let tool_input_for_hooks = tool_call
            .arguments
            .as_ref()
            .map(|a| serde_json::Value::Object(a.clone()));

        let pre_tool_outcome = if self.hook_manager.has_hooks(hooks::HookEvent::PreToolUse) {
            let ctx = hooks::HookContext::new(hooks::HookEvent::PreToolUse, &session.id)
                .with_tool(tool_call.name.to_string(), tool_input_for_hooks.clone())
                .with_tool_call_id(request_id.as_str())
                .with_working_dir(session.working_dir.to_string_lossy().to_string());
            self.hook_manager
                .emit_blocking_with_outcome(hooks::HookEvent::PreToolUse, ctx)
                .await
        } else {
            crate::hooks::HookChainOutcome::allow(false)
        };

        // Emitted before the denial returns, so an observer sees the denial
        // before the model receives the refusal. Best effort, like every other
        // hook emission: a subscriber that fails or is absent changes nothing.
        self.emit_pre_tool_use_result(
            session,
            request_id.as_str(),
            &tool_call.name,
            tool_input_for_hooks.as_ref(),
            &pre_tool_outcome,
        )
        .await;

        if let Some(denial) = pre_tool_outcome.denial() {
            tracing::Span::current().record("error.type", denial.error_type);
            return (
                request_id,
                Err(ErrorData::new(
                    ErrorCode::INTERNAL_ERROR,
                    denial.message,
                    None,
                )),
            );
        }

        self.emit_pre_tool_extended_hooks(&tool_call.name, tool_input_for_hooks.as_ref(), session)
            .await;

        if tool_call.name == FINAL_OUTPUT_TOOL_NAME {
            return if let Some(final_output_tool) = self.final_output_tool.lock().await.as_mut() {
                let result = final_output_tool.execute_tool_call(tool_call.clone()).await;
                let result = self.with_post_tool_hook(result, &tool_call, session, &request_id);
                (request_id, Ok(result))
            } else {
                // This method has always reported a missing final-output tool as
                // the outer error. Keep that contract and emit the failure
                // observation directly, the same event the wrapper would emit.
                let error = ErrorData::new(
                    ErrorCode::INTERNAL_ERROR,
                    "Final output tool not defined".to_string(),
                    None,
                );
                let failure = hooks::HookEvent::PostToolUseFailure;
                if self.hook_manager.has_hooks(failure) {
                    let ctx = hooks::HookContext::new(failure, &session.id)
                        .with_tool(tool_call.name.to_string(), tool_input_for_hooks.clone())
                        .with_tool_call_id(request_id.as_str())
                        .with_working_dir(session.working_dir.to_string_lossy().to_string());
                    self.hook_manager.emit(failure, ctx).await;
                }
                (request_id, Err(error))
            };
        }

        let ctx = tool_execution::ToolCallContext::new(
            session.id.clone(),
            Some(session.working_dir.clone()),
            Some(request_id.clone()),
        );

        debug!("WAITING_TOOL_START: {}", tool_call.name);
        let result = self
            .extension_manager
            .dispatch_tool_call(
                &ctx,
                tool_call.clone(),
                cancellation_token.unwrap_or_default(),
            )
            .await;
        let result = result.unwrap_or_else(|error_data| {
            #[cfg(feature = "telemetry")]
            crate::posthog::emit_error(
                "tool_execution_failed",
                &format!("{}: {}", tool_call.name, error_data),
            );
            ToolCallResult::from(Err(error_data))
        });

        debug!("WAITING_TOOL_END: {}", tool_call.name);

        let result = self.with_post_tool_hook(result, &tool_call, session, &request_id);
        (request_id, Ok(result))
    }

    /// Save current extension state to session metadata
    /// Should be called after any extension add/remove operation
    pub async fn save_extension_state(&self, session: &SessionConfig) -> Result<()> {
        let extensions_state =
            EnabledExtensionsState::new(self.extension_manager.get_extension_configs().await);

        let session_manager = self.config.session_manager.clone();
        let mut session_data = session_manager.get_session(&session.id, false).await?;

        if let Err(e) = extensions_state.to_extension_data(&mut session_data.extension_data) {
            warn!("Failed to serialize extension state: {}", e);
            return Err(anyhow!("Extension state serialization failed: {}", e));
        }

        session_manager
            .update(&session.id)
            .extension_data(session_data.extension_data)
            .apply()
            .await?;

        Ok(())
    }

    /// Save current extension state to session by session_id
    pub async fn persist_extension_state(&self, session_id: &str) -> Result<()> {
        self.persist_extension_configs(
            session_id,
            self.extension_manager.get_extension_configs().await,
        )
        .await
    }

    /// Save the provided extension configuration to session metadata.
    pub async fn persist_extension_configs(
        &self,
        session_id: &str,
        extensions: Vec<ExtensionConfig>,
    ) -> Result<()> {
        let extensions_state = EnabledExtensionsState::new(extensions);

        let session_manager = self.config.session_manager.clone();
        let session = session_manager.get_session(session_id, false).await?;
        let mut extension_data = session.extension_data.clone();

        extensions_state
            .to_extension_data(&mut extension_data)
            .map_err(|e| anyhow!("Failed to serialize extension state: {}", e))?;

        session_manager
            .update(session_id)
            .extension_data(extension_data)
            .apply()
            .await?;

        Ok(())
    }

    /// Load extensions from session into the agent
    /// Skips extensions that are already loaded
    /// Uses the session's working_dir for extension initialization
    pub async fn load_extensions_from_session(
        self: &Arc<Self>,
        session: &Session,
    ) -> Vec<ExtensionLoadResult> {
        let session_extensions =
            EnabledExtensionsState::from_extension_data(&session.extension_data);
        let enabled_configs = match session_extensions {
            Some(state) => state.extensions,
            None => {
                tracing::warn!(
                    "No extensions found in session {}. This is unexpected.",
                    session.id
                );
                return vec![];
            }
        };

        let manages_own_context = self
            .provider()
            .await
            .map(|p| p.manages_own_context())
            .unwrap_or(false);
        let (skipped_configs, enabled_configs): (Vec<_>, Vec<_>) =
            enabled_configs.into_iter().partition(|config| {
                manages_own_context
                    && matches!(
                        config,
                        ExtensionConfig::Stdio { .. } | ExtensionConfig::StreamableHttp { .. }
                    )
            });

        let session_id = session.id.clone();

        let extension_futures = enabled_configs
            .into_iter()
            .map(|config| {
                let config_clone = config.clone();
                let agent_ref = self.clone();
                let session_id_clone = session_id.clone();

                async move {
                    let name = config_clone.name().to_string();

                    if agent_ref
                        .extension_manager
                        .is_extension_enabled(&name)
                        .await
                    {
                        tracing::debug!("Extension {} already loaded, skipping", name);
                        return ExtensionLoadResult {
                            name,
                            success: true,
                            error: None,
                        };
                    }

                    match agent_ref
                        .add_extension_inner(config_clone, &session_id_clone)
                        .await
                    {
                        Ok(_) => ExtensionLoadResult {
                            name,
                            success: true,
                            error: None,
                        },
                        Err(e) => {
                            let error_msg = e.to_string();
                            warn!("Failed to load extension {}: {}", name, error_msg);
                            ExtensionLoadResult {
                                name,
                                success: false,
                                error: Some(error_msg),
                            }
                        }
                    }
                }
            })
            .collect::<Vec<_>>();

        let results = futures::future::join_all(extension_futures).await;

        if results.iter().any(|r| r.success)
            && skipped_configs.is_empty()
            && let Err(e) = self.persist_extension_state(&session_id).await
        {
            warn!("Failed to persist extension state after bulk load: {}", e);
        }

        results
    }

    pub async fn add_extension(
        &self,
        extension: ExtensionConfig,
        session_id: &str,
    ) -> ExtensionResult<()> {
        self.add_extension_inner(extension, session_id).await?;

        // Persist extension state after successful add
        self.persist_extension_state(session_id)
            .await
            .map_err(|e| {
                error!("Failed to persist extension state: {}", e);
                agents::extension::ExtensionError::SetupError(format!(
                    "Failed to persist extension state: {}",
                    e
                ))
            })?;

        Ok(())
    }

    /// Load multiple extensions in parallel, persisting state once at the end.
    ///
    /// Unlike `add_extension`, this avoids per-extension persistence and acquires
    /// the container lock once upfront to prevent serialisation of the parallel futures.
    ///
    /// State is persisted once every extension has settled, even when all of them
    /// fail: the session's enabled list records what actually loaded, so failed
    /// extensions are dropped instead of staying marked as enabled and being
    /// retried on every subsequent resume.
    pub async fn add_extensions_bulk(
        self: &Arc<Self>,
        extensions: Vec<ExtensionConfig>,
        session_id: &str,
    ) -> anyhow::Result<Vec<ExtensionLoadResult>> {
        let working_dir = match self
            .config
            .session_manager
            .get_session(session_id, false)
            .await
        {
            Ok(session) => Some(session.working_dir),
            Err(e) => {
                warn!("Failed to get session for bulk load: {}", e);
                None
            }
        };
        let container = self.container.lock().await.clone();

        let extension_futures = extensions
            .into_iter()
            .map(|config| {
                let ext_manager = Arc::clone(&self.extension_manager);
                let working_dir = working_dir.clone();
                let container = container.clone();
                let sid = session_id.to_string();

                async move {
                    let name = config.name().to_string();
                    match ext_manager
                        .add_extension(config, working_dir, container.as_ref(), Some(&sid))
                        .await
                    {
                        Ok(_) => ExtensionLoadResult {
                            name,
                            success: true,
                            error: None,
                        },
                        Err(e) => {
                            let error = e.to_string();
                            warn!("Failed to load extension {}: {}", name, error);
                            ExtensionLoadResult {
                                name,
                                success: false,
                                error: Some(error),
                            }
                        }
                    }
                }
            })
            .collect::<Vec<_>>();

        let results = futures::future::join_all(extension_futures).await;

        self.persist_extension_state(session_id).await?;

        Ok(results)
    }

    async fn add_extension_inner(
        &self,
        extension: ExtensionConfig,
        session_id: &str,
    ) -> ExtensionResult<()> {
        let session = self
            .config
            .session_manager
            .get_session(session_id, false)
            .await
            .map_err(|e| {
                agents::extension::ExtensionError::SetupError(format!(
                    "Failed to get session '{}': {}",
                    session_id, e
                ))
            })?;
        let working_dir = Some(session.working_dir);

        let container = self.container.lock().await;
        self.extension_manager
            .add_extension(extension, working_dir, container.as_ref(), Some(session_id))
            .await?;

        Ok(())
    }

    pub async fn list_tools(&self, session_id: &str, extension_name: Option<String>) -> Vec<Tool> {
        let include_final_output = extension_name.is_none();
        let mut prefixed_tools = self
            .extension_manager
            .get_prefixed_tools(session_id, extension_name)
            .await
            .unwrap_or_default();

        if include_final_output
            && let Some(final_output_tool) = self.final_output_tool.lock().await.as_ref()
        {
            prefixed_tools.push(final_output_tool.tool());
        }

        prefixed_tools
    }

    pub async fn remove_extension(&self, name: &str, session_id: &str) -> Result<()> {
        self.remove_extension_by_key(&name_to_key(name), session_id)
            .await?;
        Ok(())
    }

    pub async fn remove_extension_by_key(&self, key: &str, session_id: &str) -> Result<bool> {
        let session = self
            .config
            .session_manager
            .get_session(session_id, false)
            .await?;
        let persisted_extensions = EnabledExtensionsState::extensions_or_default(
            Some(&session.extension_data),
            Config::global(),
        );
        if !has_unique_persisted_extension(&persisted_extensions, key)? {
            return Ok(false);
        }

        self.extension_manager.remove_extension_by_key(key).await?;

        // Persist extension state after successful removal
        self.persist_extension_state(session_id)
            .await
            .map_err(|e| {
                error!("Failed to persist extension state: {}", e);
                anyhow!("Failed to persist extension state: {}", e)
            })?;

        Ok(true)
    }

    pub async fn list_extensions(&self) -> Vec<String> {
        self.extension_manager
            .list_extensions()
            .await
            .expect("Failed to list extensions")
    }

    pub async fn get_extension_configs(&self) -> Vec<ExtensionConfig> {
        self.extension_manager.get_extension_configs().await
    }

    pub async fn submit_tool_confirmation(
        &self,
        session_id: &str,
        request_id: &str,
        permission: Permission,
    ) -> Result<()> {
        self.config
            .session_manager
            .get_session(session_id, false)
            .await?;

        let state = self.tool_confirmation_coordinator.session(session_id);
        let _confirmation_submission_guard = state.confirmation_submission_lock.lock().await;
        let state_machine_permission = if permission == Permission::Cancel {
            Permission::DenyOnce
        } else {
            permission.clone()
        };

        if let Some(answer) = state.answer(request_id) {
            return match answer {
                ConfirmationAnswer::LiveHandled => {
                    Err(anyhow!("tool confirmation request was already answered"))
                }
                ConfirmationAnswer::StateMachine(previous)
                    if previous == state_machine_permission =>
                {
                    Ok(())
                }
                ConfirmationAnswer::StateMachine(_) => Err(anyhow!(
                    "tool confirmation request already has a different decision"
                )),
            };
        }

        let confirmation = PermissionConfirmation {
            principal_type: bcaip_provider_types::permission::PrincipalType::Tool,
            permission: permission.clone(),
        };
        if self
            .try_route_tool_confirmation_to_provider(request_id, &confirmation)
            .await
        {
            if state.contains_request(request_id) {
                state.record_answer(request_id, ConfirmationAnswer::LiveHandled)?;
            }
            return Ok(());
        }

        if self
            .tool_confirmation_router
            .deliver(session_id, request_id, confirmation)
            .await
        {
            if state.contains_request(request_id) {
                state.record_answer(request_id, ConfirmationAnswer::LiveHandled)?;
            }
            return Ok(());
        }

        if state.contains_request(request_id) {
            persist_tool_confirmation_decision(
                self.config.session_manager.as_ref(),
                session_id,
                request_id,
                &state_machine_permission,
            )
            .await?;
            state.record_answer(
                request_id,
                ConfirmationAnswer::StateMachine(state_machine_permission),
            )?;
            return Ok(());
        }

        Err(anyhow!(
            "unknown or stale tool confirmation request {request_id} for session {session_id}"
        ))
    }

    async fn try_route_tool_confirmation_to_provider(
        &self,
        request_id: &str,
        confirmation: &PermissionConfirmation,
    ) -> bool {
        let provider = self.provider.lock().await.clone();
        if let Some(provider) = provider.as_ref()
            && provider.permission_routing() == PermissionRouting::ActionRequired
            && provider
                .handle_permission_confirmation(request_id, confirmation)
                .await
        {
            return true;
        }
        false
    }

    pub async fn handle_confirmation(
        &self,
        session_id: &str,
        request_id: String,
        confirmation: PermissionConfirmation,
    ) {
        if self
            .try_route_tool_confirmation_to_provider(&request_id, &confirmation)
            .await
        {
            return;
        }
        if !self
            .tool_confirmation_router
            .deliver(session_id, &request_id, confirmation)
            .await
        {
            error!("Failed to deliver confirmation");
        }
    }

    pub async fn supports_action_required_permissions(&self) -> bool {
        if let Some(provider) = self.provider.lock().await.as_ref() {
            return provider.permission_routing() == PermissionRouting::ActionRequired;
        }
        false
    }

    pub(super) fn create_state_machine(
        &self,
        provider: Arc<dyn Provider>,
        model_config: bcaip_provider_types::model::ModelConfig,
        context_limit: usize,
        max_turns: Option<u32>,
        cancel: CancellationToken,
        steer_queue: SteerQueue,
    ) -> StateMachine<'_, Session, GooseEffect> {
        let max_turns = max_turns.unwrap_or_else(|| {
            Config::global()
                .get_param::<u32>("GOOSE_MAX_TURNS")
                .unwrap_or(DEFAULT_MAX_TURNS)
        });
        let retry_timeout = Config::global()
            .get_param::<u64>("GOOSE_RECIPE_RETRY_TIMEOUT_SECONDS")
            .unwrap_or(DEFAULT_RETRY_TIMEOUT_SECONDS);
        let on_failure_timeout = Config::global()
            .get_param::<u64>("GOOSE_RECIPE_ON_FAILURE_TIMEOUT_SECONDS")
            .unwrap_or(DEFAULT_ON_FAILURE_TIMEOUT_SECONDS);
        let stop_hook_block_cap = Config::global()
            .get_param::<u32>("GOOSE_STOP_HOOK_BLOCK_CAP")
            .unwrap_or(DEFAULT_STOP_HOOK_BLOCK_CAP);
        let compaction_threshold = Config::global()
            .get_param::<f64>("GOOSE_AUTO_COMPACT_THRESHOLD")
            .unwrap_or(DEFAULT_COMPACTION_THRESHOLD);
        let tool_call_cutoff = Config::global()
            .get_param::<usize>("GOOSE_TOOL_CALL_CUTOFF")
            .unwrap_or_else(|_| {
                context_mgmt::compute_tool_call_cutoff(context_limit, compaction_threshold)
            });
        let manages_own_context = provider.manages_own_context();
        let tool_pair_compaction_enabled =
            context_mgmt::tool_pair_summarization_enabled() && !manages_own_context;

        let mut operations: Vec<Arc<dyn Operation<Session, GooseEffect> + '_>> = vec![
            Arc::new(SteerOperation::new(steer_queue, self.hook_manager.clone())),
            Arc::new(MaxTurnsOperation::new(max_turns)),
            Arc::new(BangShellOperation::new()),
        ];
        if !manages_own_context {
            operations.push(Arc::new(CompactionOperation::new(
                provider.clone(),
                model_config.clone(),
                context_limit,
                compaction_threshold,
            )));
        }
        let remaining_operations: Vec<Arc<dyn Operation<Session, GooseEffect> + '_>> = vec![
            Arc::new(ToolPairCompactionOperation::new(
                provider.clone(),
                model_config.clone(),
                tool_call_cutoff,
                tool_pair_compaction_enabled,
            )),
            Arc::new(ToolApprovalOperation::new(
                &self.current_goose_mode,
                &self.tool_inspection_manager,
            )),
            Arc::new(DoctorOperation),
            Arc::new(ProjectOperation),
            Arc::new(SkillOperation::new(self.hook_manager.clone())),
            Arc::new(RecipeOperation::new(
                provider.clone(),
                self.hook_manager.clone(),
            )),
            Arc::new(ToolExecutionOperation::new(
                &self.current_goose_mode,
                self.extension_manager.clone(),
                self.hook_manager.clone(),
            )),
            Arc::new(UnknownToolOperation::new(self.hook_manager.clone())),
            Arc::new(RetryOperation::new(
                &self.goal,
                &self.grind,
                std::time::Duration::from_secs(retry_timeout),
                std::time::Duration::from_secs(on_failure_timeout),
            )),
            Arc::new(StopHookOperation::new(
                self.hook_manager.clone(),
                stop_hook_block_cap,
            )),
            Arc::new(ExitOnErrorOperation),
        ];
        operations.extend(remaining_operations);
        let request_preparer = GooseInferenceRequestPreparer {
            #[cfg(feature = "code-mode")]
            extension_manager: self.extension_manager.clone(),
            goose_mode: &self.current_goose_mode,
            prompt_manager: &self.prompt_manager,
            tool_inspection_manager: &self.tool_inspection_manager,
            context_limit,
        };
        let status_operation =
            Arc::new(StatusOperation::new(provider.clone(), model_config.clone()));
        let inference_provider = Arc::new(GooseInferenceProvider::new(provider));
        let inference = Arc::new(
            InferenceRunner::new(inference_provider, model_config)
                .with_request_preparer(Arc::new(request_preparer)),
        );
        let mut command_handlers = operations.clone();
        command_handlers.push(status_operation);
        let command_operation: Arc<dyn Operation<Session, GooseEffect> + '_> =
            Arc::new(SlashCommandOperation::new(command_handlers));
        let operations: Vec<_> =
            std::iter::once(Arc::new(EntryHookOperation::new(self.hook_manager.clone()))
                as Arc<dyn Operation<Session, GooseEffect> + '_>)
            .chain(std::iter::once(command_operation))
            .chain(operations)
            .collect();

        let steps = operations
            .into_iter()
            .map(Step::Operation)
            .chain(std::iter::once(Step::Inference(inference)))
            .collect();

        StateMachine::new(steps, cancel)
    }

    pub(crate) async fn reply_with_state_machine(
        &self,
        user_message: Message,
        session_config: SessionConfig,
        cancel_token: Option<CancellationToken>,
    ) -> Result<BoxStream<'_, Result<AgentEvent>>> {
        let session_id = session_config.id.clone();
        let events = crate::session_context::with_session_id(
            Some(session_id.clone()),
            self.reply_with_state_machine_inner(user_message, session_config, cancel_token),
        )
        .await?;
        Ok(crate::session_context::with_session_id_stream(
            Some(session_id),
            events,
        ))
    }

    async fn reply_with_state_machine_inner(
        &self,
        user_message: Message,
        session_config: SessionConfig,
        cancel_token: Option<CancellationToken>,
    ) -> Result<BoxStream<'_, Result<AgentEvent>>> {
        let session_manager = self.config.session_manager.clone();
        let session_id = session_config.id.clone();
        let turn_guard = self
            .tool_confirmation_coordinator
            .session(&session_id)
            .try_start_turn()?;

        if let Some(schedule_id) = session_config.schedule_id.clone() {
            session_manager
                .update(&session_id)
                .schedule_id(Some(schedule_id))
                .apply()
                .await?;
        }
        session_manager
            .add_message(&session_config.id, &user_message)
            .await?;

        if !self.config.disable_session_naming {
            let provider = self
                .provider
                .lock()
                .await
                .clone()
                .ok_or_else(|| anyhow!("Provider not set"))?;
            let manager = session_manager.clone();
            let tx = self.config.session_name_update_tx.clone();
            let id = session_id.clone();
            let provider = provider.clone();
            tokio::spawn(async move {
                match manager.maybe_update_name(&id, provider).await {
                    Ok(Some(update)) => {
                        if let Some(tx) = tx
                            && tx.send(update).is_err()
                        {
                            tracing::warn!("Failed to publish generated session name");
                        }
                    }
                    Ok(None) => {}
                    Err(e) => tracing::warn!("Failed to generate session description: {}", e),
                }
            });
        }

        let cancel = cancel_token.unwrap_or_default();
        let initial_stream = self
            .stream_state_machine_session(session_config.clone(), cancel.clone())
            .await?;
        Ok(
            self.stream_state_machine_turn(
                session_config,
                cancel,
                turn_guard,
                Some(initial_stream),
            ),
        )
    }

    pub(crate) async fn resume_state_machine_turn(
        self: &Arc<Self>,
        session_config: SessionConfig,
        cancel: CancellationToken,
    ) -> Result<Option<BoxStream<'static, Result<AgentEvent>>>> {
        let session_id = session_config.id.clone();
        let stream = session_context::with_session_id(
            Some(session_id.clone()),
            self.resume_state_machine_turn_inner(session_config, cancel),
        )
        .await?;
        Ok(stream.map(|stream| session_context::with_session_id_stream(Some(session_id), stream)))
    }

    async fn resume_state_machine_turn_inner(
        self: &Arc<Self>,
        session_config: SessionConfig,
        cancel: CancellationToken,
    ) -> Result<Option<BoxStream<'static, Result<AgentEvent>>>> {
        if !super::state_machine::enabled() {
            return Ok(None);
        }

        let session = self
            .config
            .session_manager
            .get_session(&session_config.id, true)
            .await?;
        let conversation = session
            .conversation
            .as_ref()
            .ok_or_else(|| anyhow!("Session {} has no conversation", session_config.id))?;
        let pending_confirmations = pending_tool_confirmations(conversation);
        let resume_from_persisted_response = pending_confirmations.is_empty()
            && has_unapplied_tool_confirmation_response(conversation);
        if pending_confirmations.is_empty() && !resume_from_persisted_response {
            return Ok(None);
        }

        let turn_guard = self
            .tool_confirmation_coordinator
            .session(&session_config.id)
            .try_start_turn()?;
        for request in pending_confirmations {
            turn_guard.state().register_request(request.id);
        }

        let agent = Arc::clone(self);
        let stream = Box::pin(async_stream::try_stream! {
            let initial_stream = if resume_from_persisted_response {
                Some(
                    agent
                        .stream_state_machine_session(
                            session_config.clone(),
                            cancel.clone(),
                        )
                        .await?,
                )
            } else {
                None
            };
            let mut stream = agent.stream_state_machine_turn(
                session_config,
                cancel,
                turn_guard,
                initial_stream,
            );
            while let Some(event) = stream.next().await {
                yield event?;
            }
        });
        Ok(Some(stream))
    }

    fn tool_confirmation_request_ids(event: &AgentEvent) -> Vec<String> {
        let AgentEvent::Message(message) = event else {
            return Vec::new();
        };

        message
            .content
            .iter()
            .filter_map(|content| {
                let MessageContent::ActionRequired(action) = content else {
                    return None;
                };
                let ActionRequiredData::ToolConfirmation { id, .. } = &action.data else {
                    return None;
                };
                Some(id.clone())
            })
            .collect()
    }

    fn stream_state_machine_turn<'a>(
        &'a self,
        session_config: SessionConfig,
        cancel: CancellationToken,
        turn_guard: ActiveTurnGuard,
        initial_stream: Option<BoxStream<'a, Result<AgentEvent>>>,
    ) -> BoxStream<'a, Result<AgentEvent>> {
        Box::pin(async_stream::try_stream! {
            let mut stream = initial_stream;
            loop {
                if let Some(active_stream) = stream.as_mut() {
                    let mut has_confirmations = false;
                    while let Some(event) = active_stream.next().await {
                        let event = event?;
                        for request_id in Self::tool_confirmation_request_ids(&event) {
                            turn_guard.state().register_request(request_id);
                            has_confirmations = true;
                        }
                        yield event;
                    }

                    if !has_confirmations {
                        return;
                    }
                }

                let has_state_machine_answer = turn_guard
                    .state()
                    .wait_for_all_confirmation_answers(&cancel)
                    .await?;
                if !has_state_machine_answer {
                    return;
                }
                turn_guard.state().clear_confirmations();
                stream = Some(
                    self.stream_state_machine_session(
                        session_config.clone(),
                        cancel.clone(),
                    )
                    .await?,
                );
            }
        })
    }

    async fn stream_state_machine_session(
        &self,
        session_config: SessionConfig,
        cancel: CancellationToken,
    ) -> Result<BoxStream<'_, Result<AgentEvent>>> {
        let session_manager = self.config.session_manager.clone();
        let session_id = session_config.id.clone();
        let provider = self
            .provider
            .lock()
            .await
            .clone()
            .ok_or_else(|| anyhow!("Provider not set"))?;
        let model_config = self.effective_model_config_for_session(&session_id).await?;

        let context_limit =
            context_limit::get_context_limit(provider.as_ref(), &model_config.model_name).await?;
        let steer_queue = self.steer_queue(&session_id).await;
        let machine = self.create_state_machine(
            provider,
            model_config,
            context_limit,
            session_config.max_turns,
            cancel.clone(),
            steer_queue,
        );
        let reply_span = tracing::Span::current();

        Ok(Box::pin(
            async_stream::try_stream! {
                let (tx, mut rx) = mpsc::channel::<AgentEvent>(32);
                let emit = Emitter::new(tx, cancel.clone());
                let result = {
                    let run = session_context::with_session_id(
                        Some(session_id.clone()),
                        run_goose(&machine, session_manager.as_ref(), &session_id, &emit),
                    );
                    tokio::pin!(run);
                    loop {
                        tokio::select! {
                            biased;
                            Some(event) = rx.recv() => yield event,
                            result = &mut run => break result,
                        }
                    }
                };
                result?;
                // Without this the drain below never ends: `run` only borrows the emitter.
                drop(emit);
                while let Some(event) = rx.recv().await {
                    yield event;
                }
            }
            .instrument(reply_span),
        ))
    }

    pub(crate) async fn reply_live_delegation(
        &self,
        user_message: Message,
        session_config: SessionConfig,
        cancel_token: CancellationToken,
    ) -> Result<BoxStream<'_, Result<AgentEvent>>> {
        let user_message = user_message.agent_only();
        let events = self
            .reply_with_state_machine(user_message, session_config, Some(cancel_token))
            .await?;
        Ok(Box::pin(events.map_ok(ensure_message_event_id)))
    }

    #[instrument(
        skip(self, user_message, session_config, use_state_machine, cancel_token),
        fields(
            user_message,
            trace_input,
            trace_output = tracing::field::Empty,
            session.id = %session_config.id,
            gen_ai.operation.name = "invoke_agent",
            gen_ai.agent.name = tracing::field::Empty,
            gen_ai.input.messages = tracing::field::Empty,
            gen_ai.output.messages = tracing::field::Empty,
            gen_ai.usage.input_tokens = tracing::field::Empty,
            gen_ai.usage.output_tokens = tracing::field::Empty,
        )
    )]
    pub async fn reply(
        &self,
        user_message: Message,
        session_config: SessionConfig,
        use_state_machine: bool,
        cancel_token: Option<CancellationToken>,
    ) -> Result<BoxStream<'_, Result<AgentEvent>>> {
        let reply_span = tracing::Span::current();
        let session_id = session_config.id.clone();
        let events = session_context::with_session_id(
            Some(session_id.clone()),
            self.reply_impl(
                user_message,
                session_config,
                use_state_machine,
                cancel_token,
            ),
        )
        .await?;
        let events = session_context::with_session_id_stream(Some(session_id), events);

        // This is the single live-event identity boundary. Callers that intentionally stream
        // multiple events for one logical message must assign their shared ID before this point.
        Ok(Box::pin(
            events
                .map_ok(ensure_message_event_id)
                .instrument(reply_span),
        ))
    }

    async fn reply_impl(
        &self,
        user_message: Message,
        session_config: SessionConfig,
        use_state_machine: bool,
        cancel_token: Option<CancellationToken>,
    ) -> Result<BoxStream<'_, Result<AgentEvent>>> {
        let user_message = user_message.with_generated_id_if_missing();
        let session_manager = self.config.session_manager.clone();

        let message_text_for_trace = agent_visible_message_text(&user_message);
        if gen_ai_telemetry::capture_message_content() {
            tracing::Span::current().record("user_message", message_text_for_trace.as_str());
            tracing::Span::current().record("trace_input", message_text_for_trace.as_str());
            tracing::Span::current().record(
                "gen_ai.input.messages",
                gen_ai_telemetry::simple_input_json(&message_text_for_trace).as_str(),
            );
        }

        for content in &user_message.content {
            if let MessageContent::ActionRequired(action_required) = content
                && let ActionRequiredData::ElicitationResponse {
                    id,
                    user_data,
                    action,
                } = &action_required.data
            {
                // Surface stale/cancelled/timed-out elicitations as a hard
                // error so callers (e.g. the HTTP handler) can propagate
                // failure to the client instead of silently reporting
                // success while the blocked tool call stays unblocked.
                // The success path returns an empty stream after the MCP
                // server receives the user's accept/decline/cancel action.
                let response = match action {
                    ElicitationAction::Accept => ElicitationOutcome::Accept(user_data.clone()),
                    ElicitationAction::Decline => ElicitationOutcome::Decline,
                    ElicitationAction::Cancel => ElicitationOutcome::Cancel,
                    _ => ElicitationOutcome::Cancel,
                };
                crate::elicitation::complete_elicitation_with_message(
                    &session_manager,
                    &session_config.id,
                    id,
                    response,
                    &user_message,
                )
                .await
                .map_err(|e| {
                    error!("Failed to submit elicitation response: {}", e);
                    anyhow!("Failed to submit elicitation response: {}", e)
                })?;
                return Ok(Box::pin(futures::stream::empty()));
            }
        }

        if use_state_machine {
            tracing::info!("dispatching reply via experimental state machine");
            return self
                .reply_with_state_machine(user_message, session_config, cancel_token)
                .await;
        }

        let message_text = message_text_for_trace;

        let session = session_manager
            .get_session(&session_config.id, true)
            .await?;
        tracing::Span::current()
            .record("gen_ai.agent.name", gen_ai_telemetry::agent_name(&session));
        let is_first_agent_turn = session
            .conversation
            .as_ref()
            .map(|conversation| {
                conversation.messages().iter().all(|message| {
                    !message.is_agent_visible()
                        || message.agent_visible_content().content.is_empty()
                })
            })
            .unwrap_or(true);

        if !user_message.is_agent_visible()
            || user_message.agent_visible_content().content.is_empty()
        {
            let user_visibility = user_message.is_user_visible();
            let user_message = user_message.with_visibility(user_visibility, false);
            session_manager
                .add_message(&session_config.id, &user_message)
                .await?;
            return Ok(Box::pin(futures::stream::empty()));
        }

        if is_first_agent_turn && !self.session_start_emitted.swap(true, Ordering::AcqRel) {
            self.emit_hook(hooks::HookEvent::SessionStart, &session_config.id)
                .await;
        }

        if self
            .hook_manager
            .has_hooks(hooks::HookEvent::UserPromptSubmit)
        {
            let ctx =
                hooks::HookContext::new(hooks::HookEvent::UserPromptSubmit, &session_config.id)
                    .with_message(message_text.clone());
            self.hook_manager
                .emit(hooks::HookEvent::UserPromptSubmit, ctx)
                .await;
        }

        let command_result = self
            .execute_command(&message_text, &session_config.id)
            .await;

        let mut command_preamble: Vec<AgentEvent> = Vec::new();

        match command_result {
            Err(e) => {
                let error_message = Message::assistant()
                    .with_text(e.to_string())
                    .with_visibility(true, false);
                return Ok(Box::pin(stream::once(async move {
                    Ok(AgentEvent::Message(error_message))
                })));
            }
            Ok(Some(response))
                if response.role == rmcp::model::Role::Assistant
                    && agents::execute_commands::command_starts_turn(&message_text) =>
            {
                let response = response.with_generated_id_if_missing();

                // Setting a goal/grind should immediately start a turn so the
                // agent begins pursuing it, rather than waiting for the next
                // user prompt. Record the command and its confirmation as
                // user-visible only, then inject an agent-visible kickoff and
                // fall through into the reply loop.
                session_manager
                    .add_message(
                        &session_config.id,
                        &user_message.clone().with_visibility(true, false),
                    )
                    .await?;
                session_manager
                    .add_message(
                        &session_config.id,
                        &response.clone().with_visibility(true, false),
                    )
                    .await?;
                let goal_text = agents::execute_commands::parse_slash_command(&message_text)
                    .map(|parsed| parsed.params_str.to_string())
                    .unwrap_or_default();
                let kickoff = Message::user()
                    .with_text(format!(
                        "Start working toward this goal now:\n\n**Goal:** {goal_text}"
                    ))
                    .with_visibility(false, true);
                session_manager
                    .add_message(&session_config.id, &kickoff)
                    .await?;

                command_preamble = vec![
                    AgentEvent::Message(user_message.clone()),
                    AgentEvent::Message(response.clone()),
                ];
            }
            Ok(Some(response)) if response.role == rmcp::model::Role::Assistant => {
                let response = response.with_generated_id_if_missing();

                session_manager
                    .add_message(
                        &session_config.id,
                        &user_message.clone().with_visibility(true, false),
                    )
                    .await?;
                session_manager
                    .add_message(
                        &session_config.id,
                        &response.clone().with_visibility(true, false),
                    )
                    .await?;

                // Check if this was a command that modifies conversation history
                let modifies_history = agents::execute_commands::COMPACT_TRIGGERS
                    .contains(&message_text.trim())
                    || message_text.trim() == "/clear";

                return Ok(Box::pin(async_stream::try_stream! {
                    yield AgentEvent::Message(user_message);
                    yield AgentEvent::Message(response);

                    // After commands that modify history, notify UI that history was replaced
                    if modifies_history {
                        let updated_session = session_manager.get_session(&session_config.id, true)
                            .await
                            .map_err(|e| anyhow!("Failed to fetch updated session: {}", e))?;
                        let updated_conversation = updated_session
                            .conversation
                            .ok_or_else(|| anyhow!("Session has no conversation after history modification"))?;
                        yield AgentEvent::HistoryReplaced(updated_conversation);
                    }
                }));
            }
            Ok(Some(resolved_message)) => {
                session_manager
                    .add_message(
                        &session_config.id,
                        &user_message.clone().with_visibility(true, false),
                    )
                    .await?;
                session_manager
                    .add_message(
                        &session_config.id,
                        &resolved_message.clone().with_visibility(false, true),
                    )
                    .await?;
            }
            Ok(None) => {
                session_manager
                    .add_message(&session_config.id, &user_message)
                    .await?;
            }
        }
        let session = session_manager
            .get_session(&session_config.id, true)
            .await?;
        let conversation = session
            .conversation
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Session {} has no conversation", session_config.id))?;

        if self.final_output_tool.lock().await.is_some() {
            let provider = self.provider().await?;
            if !provider.supports_builtin_tools() {
                let provider_name = provider.get_name();
                warn!(
                    provider = %provider_name,
                    "Recipe declares structured response, but this provider can't receive the final_output tool; failing before inference"
                );
                let message = Message::assistant()
                    .with_text(structured_output_unsupported_message(provider_name))
                    .with_generated_id_if_missing();
                session_manager
                    .add_message(&session_config.id, &message)
                    .await?;

                return Ok(Box::pin(async_stream::try_stream! {
                    for event in command_preamble {
                        yield event;
                    }
                    yield AgentEvent::Message(message);
                }));
            }
        }

        let needs_auto_compact = check_if_compaction_needed(
            self.provider().await?.as_ref(),
            &conversation,
            None,
            &session,
        )
        .await?;

        let conversation_to_compact = conversation.clone();
        let reply_span = tracing::Span::current();
        reply_span.record("gen_ai.agent.name", gen_ai_telemetry::agent_name(&session));

        Ok(Box::pin(async_stream::try_stream! {
            for event in command_preamble {
                yield event;
            }

            let final_conversation = if !needs_auto_compact {
                conversation
            } else {
                let config = Config::global();
                let threshold = config
                    .get_param::<f64>("GOOSE_AUTO_COMPACT_THRESHOLD")
                    .unwrap_or(DEFAULT_COMPACTION_THRESHOLD);
                let threshold_percentage = (threshold * 100.0) as u32;

                let inline_msg = format!(
                    "Exceeded auto-compact threshold of {}%. Performing auto-compaction...",
                    threshold_percentage
                );

                yield AgentEvent::Message(
                    Message::assistant().with_system_notification(
                        SystemNotificationType::InlineMessage,
                        inline_msg,
                    )
                );

                yield AgentEvent::Message(
                    Message::assistant().with_system_notification(
                        SystemNotificationType::ProgressMessage,
                        COMPACTION_PROGRESS_TEXT,
                    )
                );

                let compact_model_config = self.model_config_for_session(&session_config.id).await?;
                match compact_messages(
                    self.provider().await?.as_ref(),
                    &compact_model_config,
                    &session_config.id,
                    &conversation_to_compact,
                    false,
                )
                .await
                {
                    Ok(compaction) => {
                        let compacted_conversation = compaction.conversation;
                        session_manager.replace_conversation(&session_config.id, &compacted_conversation).await?;
                        self.update_session_metrics(&session_config.id, session_config.schedule_id.clone(), &compaction.usage, Some(compaction.retained_context_tokens)).await?;

                        yield AgentEvent::HistoryReplaced(compacted_conversation.clone());

                        yield AgentEvent::Message(
                            Message::assistant().with_system_notification(
                                SystemNotificationType::InlineMessage,
                                "Compaction complete",
                            )
                        );

                        compacted_conversation
                    }
                    Err(e) => {
                        yield AgentEvent::Message(
                            Message::assistant().with_text(
                                format!("Ran into this error trying to compact: {e}.\n\nPlease try again or create a new session")
                            )
                        );
                        return;
                    }
                }
            };

            let parent_span = tracing::Span::current();
            let mut reply_stream = self.reply_internal(final_conversation, session_config, session, cancel_token, parent_span.clone()).await?;
            while let Some(event) = reply_stream.next().await {
                yield event?;
            }
        }))
    }

    async fn reply_internal(
        &self,
        conversation: Conversation,
        session_config: SessionConfig,
        session: Session,
        cancel_token: Option<CancellationToken>,
        reply_span: tracing::Span,
    ) -> Result<BoxStream<'_, Result<AgentEvent>>> {
        let context = self
            .prepare_reply_context(&session.id, conversation, session.working_dir.as_path())
            .await?;
        let ReplyContext {
            mut conversation,
            mut tools,
            mut toolshim_tools,
            mut system_prompt,
            tool_call_cut_off,
            goose_mode,
            model_config,
        } = context;

        if let Some(project_addendum) = self.load_project_instructions(&session).await {
            system_prompt = format!("{system_prompt}\n\n{project_addendum}");
        }

        self.reset_retry_attempts().await;

        let provider = self.provider().await?;
        let provider_name = provider.get_name().to_string();
        let saved_provider_session_id =
            super::latest_provider_session_id(conversation.messages(), &provider_name);
        if let Some(saved_provider_session_id) = saved_provider_session_id
            && let Err(error) = provider.resume(saved_provider_session_id).await
        {
            warn!(
                provider = provider_name,
                %error,
                "Could not resume provider session; continuing with a handoff"
            );
        }

        let requested_model = model_config.model_name.clone();
        let resolved_model = provider
            .fetch_model_info(&requested_model)
            .await
            .ok()
            .and_then(|model_info| model_info.resolved_model);
        let provider_session_id = provider.provider_session_id();
        let inference = Some(InferenceMetadata {
            provider: provider_name.clone(),
            requested_model,
            resolved_model,
            provider_session_id,
        });
        let session_manager = self.config.session_manager.clone();
        let session_id = session_config.id.clone();
        if !self.config.disable_session_naming {
            let provider = provider.clone();
            let manager_for_spawn = session_manager.clone();
            let session_name_update_tx = self.config.session_name_update_tx.clone();
            tokio::spawn(async move {
                match manager_for_spawn
                    .maybe_update_name(&session_id, provider)
                    .await
                {
                    Ok(Some(update)) => {
                        if let Some(tx) = session_name_update_tx
                            && tx.send(update).is_err()
                        {
                            warn!("Failed to publish generated session name");
                        }
                    }
                    Ok(None) => {}
                    Err(e) => warn!("Failed to generate session description: {}", e),
                }
            });
        }

        // Count tool calls present before this reply — everything added during
        // the reply loop is part of the current turn and should not be summarized.
        let pre_turn_tool_count = conversation
            .messages()
            .iter()
            .flat_map(|m| m.content.iter())
            .filter(|c| matches!(c, MessageContent::ToolRequest(_)))
            .count();

        let working_dir = session.working_dir.clone();
        let reply_stream_span = tracing::info_span!(
            parent: &reply_span,
            "reply_stream",
            trace_output = tracing::field::Empty,
            session.id = %session_config.id,
            session.user = %session_context::session_user(),
            session.host = %session_context::session_host(),
            session.agent_type = "goose",
            gen_ai.operation.name = "invoke_agent",
            gen_ai.agent.name = tracing::field::Empty,
            gen_ai.conversation.id = %session_config.id,
            gen_ai.request.model = %model_config.model_name,
            gen_ai.request.temperature = tracing::field::Empty,
            gen_ai.request.max_tokens = tracing::field::Empty,
            gen_ai.provider.name = %provider_name,
            gen_ai.input.messages = tracing::field::Empty,
            gen_ai.output.messages = tracing::field::Empty,
            gen_ai.response.finish_reasons = tracing::field::Empty,
            gen_ai.response.id = tracing::field::Empty,
            gen_ai.usage.input_tokens = tracing::field::Empty,
            gen_ai.usage.output_tokens = tracing::field::Empty,
        );
        gen_ai_telemetry::record_request_params(&reply_stream_span, &model_config);
        reply_stream_span.record("gen_ai.agent.name", gen_ai_telemetry::agent_name(&session));
        if gen_ai_telemetry::capture_message_content()
            && let Some(last_user_msg) = conversation
                .messages()
                .iter()
                .rev()
                .find(|m| m.role == rmcp::model::Role::User)
        {
            reply_stream_span.record(
                "gen_ai.input.messages",
                gen_ai_telemetry::simple_input_json(&last_user_msg.as_concat_text()).as_str(),
            );
        }
        let inner = Box::pin(async_stream::try_stream! {
            let mut turns_taken = 0u32;
            let max_turns = session_config.max_turns.unwrap_or_else(|| {
                Config::global()
                    .get_param::<u32>("GOOSE_MAX_TURNS")
                    .unwrap_or(DEFAULT_MAX_TURNS)
            });
            let mut compaction_attempts = 0;
            let mut empty_turn_retries = 0u32;
            let mut retrying_after_empty_turn = false;
            let mut last_assistant_text = String::new();
            let mut turn_total_usage = Usage::default();
            let mut goal_check_pending = false;
            let mut tool_pair_summarization_done = false;
            let mut stop_hook_handled_for_exit = false;
            let mut retrying_after_stop_hook_denial = false;
            let mut consecutive_stop_hook_blocks = 0u32;
            let stop_hook_block_cap = self.stop_hook_block_cap();
            let mut can_drain_pending_steers = false;
            let turn_start = chrono::Local::now();
            let turn_start_compaction_info =
                moim::compute_compaction_info(&session_config.id, &self.extension_manager)
                    .await;

            if let Some(turn_context) = moim::turn_context_message(
                &session_config.id,
                &self.extension_manager,
                turns_taken,
                max_turns,
                turn_start,
                turn_start_compaction_info,
            )
            .await
            {
                persist_and_push_message_with_id(
                    &session_manager,
                    &session_config.id,
                    &mut conversation,
                    turn_context,
                )
                .await?;
            }
            // Snapshot after the turn-context append so a retry keeps the sent prefix.
            let initial_messages = conversation.messages().clone();

            loop {
                if is_token_cancelled(&cancel_token) {
                    break;
                }

                if can_drain_pending_steers {
                    for message in self.drain_pending_steers(&session_config.id).await {
                        let message_text = agent_visible_message_text(&message);
                        if self
                            .hook_manager
                            .has_hooks(hooks::HookEvent::UserPromptSubmit)
                        {
                            let ctx = hooks::HookContext::new(
                                hooks::HookEvent::UserPromptSubmit,
                                &session_config.id,
                            )
                            .with_message(message_text);
                            self.hook_manager
                                .emit(hooks::HookEvent::UserPromptSubmit, ctx)
                                .await;
                        }
                        let message = persist_and_push_message_with_id(
                            &session_manager,
                            &session_config.id,
                            &mut conversation,
                            message,
                        )
                        .await?;
                        yield AgentEvent::Message(message);
                    }
                }

                let final_output = {
                    let mut guard = self.final_output_tool.lock().await;
                    guard.as_mut().and_then(|fot| fot.final_output.take())
                };
                if let Some(output) = final_output {
                    last_assistant_text = output.clone();
                    let message = Message::assistant()
                        .with_text(output)
                        .with_generated_id_if_missing();
                    yield AgentEvent::Message(message.clone());
                    session_manager.add_message(&session_config.id, &message).await?;
                    conversation.push(message);

                    match self
                        .emit_stop_hook_blocking(&session_config.id, &last_assistant_text, &session.working_dir.to_string_lossy())
                        .await
                    {
                        hooks::HookDecision::Allow => {
                            stop_hook_handled_for_exit = true;
                            break;
                        }
                        hooks::HookDecision::Deny { reason, plugin } => {
                            consecutive_stop_hook_blocks += 1;
                            if consecutive_stop_hook_blocks > stop_hook_block_cap {
                                let message = persist_message_with_id(
                                    &session_manager,
                                    &session_config.id,
                                    stop_hook_block_cap_warning(&plugin, stop_hook_block_cap),
                                )
                                .await?;
                                yield AgentEvent::Message(message);
                                stop_hook_handled_for_exit = true;
                                break;
                            }
                            persist_and_push_message_with_id(
                                &session_manager,
                                &session_config.id,
                                &mut conversation,
                                stop_hook_denial_context_message(&plugin, &reason),
                            )
                            .await?;
                            yield AgentEvent::Message(stop_hook_denial_notification(&plugin));
                            retrying_after_stop_hook_denial = true;
                            continue;
                        }
                    }
                }

                if retrying_after_stop_hook_denial {
                    retrying_after_stop_hook_denial = false;
                } else if retrying_after_empty_turn {
                    retrying_after_empty_turn = false;
                } else {
                    turns_taken += 1;
                }
                if turns_taken > max_turns {
                    last_assistant_text = MAX_TURNS_MESSAGE.to_string();
                    yield AgentEvent::Message(Message::assistant().with_text(last_assistant_text.clone()));
                    break;
                }

                let mut stream = agents::reply_parts::stream_response_from_provider(
                    self.provider().await?,
                    model_config.clone(),
                    &session_config.id,
                    &system_prompt,
                    conversation.messages(),
                    &tools,
                    &toolshim_tools,
                ).await?;
                last_assistant_text.clear();

                let current_turn_tool_count = conversation.messages().iter()
                    .flat_map(|m| m.content.iter())
                    .filter(|c| matches!(c, MessageContent::ToolRequest(_)))
                    .count()
                    .saturating_sub(pre_turn_tool_count);

                let tool_pair_summarization_task = if tool_pair_summarization_done {
                    None
                } else {
                    context_mgmt::maybe_summarize_tool_pairs(
                        self.provider().await?,
                        model_config.clone(),
                        session_config.id.clone(),
                        conversation.clone(),
                        tool_call_cut_off,
                        current_turn_tool_count,
                    )
                };

                let mut no_tools_called = true;
                let mut messages_to_add = Conversation::default();
                let mut tools_updated = false;
                let mut did_recovery_compact_this_iteration = false;
                let mut exit_chat = false;
                let mut provider_errored = false;
                let mut provider_produced_content = false;
                let mut provider_reached_output_token_limit = false;
                let mut pending_final_output: Option<String> = None;
                let mut pending_turn_usage: Option<ProviderUsage> = None;
                let mut preferred_turn_usage_message_id: Option<String> = None;

                // Track whether this provider turn has already emitted visible
                // thinking so a later tool-call chunk can suppress replayed
                // reasoning without hiding final-only non-streaming thoughts.
                let mut surfaced_thinking_in_turn = false;

                loop {
                    let next = if let Some(cancel_token) = &cancel_token {
                        tokio::select! {
                            biased;
                            _ = cancel_token.cancelled() => break,
                            next = stream.next() => next,
                        }
                    } else {
                        stream.next().await
                    };
                    let Some(next) = next else {
                        break;
                    };

                    if exit_chat {
                        break;
                    }

                    match next {
                        Ok((response, usage)) => {
                            compaction_attempts = 0;

                            if let Some(ref usage) = usage {
                                let enriched = self.update_session_metrics(&session_config.id, session_config.schedule_id.clone(), usage, None).await?;
                                yield AgentEvent::Usage(enriched.clone());
                                turn_total_usage += enriched.usage;
                                pending_turn_usage = Some(enriched);
                            }

                            if let Some(response) = response {
                                provider_reached_output_token_limit |=
                                    response.metadata.output_token_limit_reached;

                                if !response.content.is_empty()
                                    && response.content.iter().all(|content| {
                                        matches!(content, MessageContent::SystemNotification(_))
                                    })
                                {
                                    yield AgentEvent::Message(response);
                                    tokio::task::yield_now().await;
                                    continue;
                                }

                                provider_produced_content |= response.content.iter().any(|content| {
                                    match content {
                                        MessageContent::Text(text) => !text.text.is_empty(),
                                        MessageContent::Image(image) => !image.data.is_empty(),
                                        MessageContent::Thinking(thinking) => {
                                            !thinking.thinking.is_empty()
                                                || !thinking.signature.is_empty()
                                        }
                                        MessageContent::RedactedThinking(thinking) => {
                                            !thinking.data.is_empty()
                                        }
                                        MessageContent::SystemNotification(notification) => {
                                            !notification.msg.is_empty()
                                        }
                                        _ => true,
                                    }
                                });

                                let (tool_requests, filtered_response) = self
                                    .categorize_tool_requests(
                                        &response,
                                        &tools,
                                        &toolshim_tools,
                                        surfaced_thinking_in_turn,
                                    );

                                let filtered_response = if let Some(inference) = inference.as_ref() {
                                    filtered_response.with_inference(inference.clone())
                                } else {
                                    filtered_response
                                };
                                let response = if let Some(inference) = inference.as_ref() {
                                    response.with_inference(inference.clone())
                                } else {
                                    response
                                };

                                surfaced_thinking_in_turn |= filtered_response.content.iter().any(
                                    |content| {
                                        matches!(
                                            content,
                                            MessageContent::Thinking(_)
                                                | MessageContent::RedactedThinking(_)
                                        )
                                    },
                                );

                                if !filtered_response.content.is_empty()
                                    || filtered_response.metadata.output_token_limit_reached
                                {
                                    yield AgentEvent::Message(filtered_response.clone());
                                    tokio::task::yield_now().await;
                                }

                                if tool_requests.is_empty() {
                                    let text = if response.is_user_visible() {
                                        filtered_response
                                            .user_visible_content()
                                            .as_concat_text()
                                    } else {
                                        String::new()
                                    };
                                    if !text.is_empty() {
                                        last_assistant_text.push_str(&text);
                                    }
                                    messages_to_add.push(response);
                                    continue;
                                }

                                let mut request_to_response_map = HashMap::new();
                                let mut request_metadata: HashMap<String, Option<ProviderMetadata>> = HashMap::new();
                                for request in &tool_requests {
                                    request_to_response_map.insert(request.id.clone(), Message::user().with_generated_id());
                                    request_metadata.insert(request.id.clone(), request.metadata.clone());
                                }

                                if goose_mode == GooseMode::Chat {
                                    for request in &tool_requests {
                                        // An unparseable tool call should surface the parse error
                                        // (added in the Err branch below), not a successful skip —
                                        // otherwise the model sees a malformed call as "skipped OK"
                                        // and can't correct the arguments.
                                        if request.tool_call.is_err() {
                                            continue;
                                        }
                                        if let Some(response) = request_to_response_map.get_mut(&request.id) {
                                            response.add_tool_response_with_metadata(
                                                request.id.clone(),
                                                Ok(CallToolResult::success(vec![ContentBlock::text(CHAT_MODE_TOOL_SKIPPED_RESPONSE)])),
                                                request.metadata.as_ref(),
                                            );
                                        }
                                    }
                                } else {
                                    // Run all tool inspectors
                                    let inspection_results = self.tool_inspection_manager
                                        .inspect_tools(
                                            &session_config.id,
                                            &tool_requests,
                                            conversation.messages(),
                                            goose_mode,
                                        )
                                        .await?;

                                    let permission_check_result = self.tool_inspection_manager
                                        .process_inspection_results_with_permission_inspector(
                                            &tool_requests,
                                            &inspection_results,
                                        )
                                        .unwrap_or_else(|| {
                                            let mut result = PermissionCheckResult {
                                                approved: vec![],
                                                needs_approval: vec![],
                                                denied: vec![],
                                            };
                                            result.needs_approval.extend(tool_requests.iter().cloned());
                                            result
                                        });

                                    // Track extension requests
                                    let mut enable_extension_request_ids = vec![];
                                    for request in &tool_requests {
                                        if let Ok(tool_call) = &request.tool_call
                                            && tool_call.name == MANAGE_EXTENSIONS_TOOL_NAME_COMPLETE {
                                                enable_extension_request_ids.push(request.id.clone());
                                            }
                                    }

                                    let mut tool_futures = self.handle_approved_and_denied_tools(
                                        &permission_check_result,
                                        &mut request_to_response_map,
                                        cancel_token.clone(),
                                        &session,
                                    ).await?;

                                    {
                                        let mut tool_approval_stream = self.handle_approval_tool_requests(
                                            &permission_check_result.needs_approval,
                                            &mut tool_futures,
                                            &mut request_to_response_map,
                                            cancel_token.clone(),
                                            &session,
                                            &inspection_results,
                                        );

                                        while let Some(msg) = tool_approval_stream.try_next().await? {
                                            yield AgentEvent::Message(msg);
                                        }
                                    }

                                    let with_id = tool_futures
                                        .into_iter()
                                        .map(|(request_id, stream)| {
                                            stream.map(move |item| (request_id.clone(), item))
                                        })
                                        .collect::<Vec<_>>();

                                    let mut combined = stream::select_all(with_id);
                                    let mut all_install_successful = true;

                                    loop {
                                        if is_token_cancelled(&cancel_token) {
                                            break;
                                        }

                                        tokio::select! {
                                            biased;

                                            tool_item = combined.next() => {
                                                match tool_item {
                                                    Some((request_id, item)) => {
                                                        match item {
                                                            ToolStreamItem::ActionRequired(msg) => {
                                                                let msg = msg.with_generated_id_if_missing();
                                                                if let Err(e) = session_manager.add_message(&session_config.id, &msg).await {
                                                                    warn!("Failed to save elicitation message to session: {}", e);
                                                                }
                                                                yield AgentEvent::Message(msg);
                                                            }
                                                            ToolStreamItem::Result(output) => {
                                                                if let Ok(ref call_result) = output
                                                                    && let Some(ref meta) = call_result.meta
                                                                        && let Some(notification_data) = meta.0.get("platform_notification")
                                                                            && let Some(method) = notification_data.get("method").and_then(|v| v.as_str()) {
                                                                                let params = notification_data.get("params").cloned();
                                                                                let custom_notification = rmcp::model::CustomNotification::new(
                                                                                    method.to_string(),
                                                                                    params,
                                                                                );

                                                                                let server_notification = rmcp::model::ServerNotification::CustomNotification(custom_notification);
                                                                                yield AgentEvent::McpNotification((request_id.clone(), server_notification));
                                                                            }

                                                                if enable_extension_request_ids.contains(&request_id)
                                                                    && output.is_err()
                                                                {
                                                                    all_install_successful = false;
                                                                }
                                                                if let Some(response) = request_to_response_map.get_mut(&request_id) {
                                                                    let metadata = request_metadata.get(&request_id).and_then(|m| m.as_ref());
                                                                    response.add_tool_response_with_metadata(request_id, output, metadata);
                                                                }
                                                            }
                                                            ToolStreamItem::Message(msg) => {
                                                                yield AgentEvent::McpNotification((request_id, msg));
                                                            }
                                                        }
                                                    }
                                                    None => break,
                                                }
                                            }

                                            _ = tokio::time::sleep(time::Duration::from_millis(100)) => {}
                                        }
                                    }

                                    if all_install_successful && !enable_extension_request_ids.is_empty() {
                                        if let Err(e) = self.save_extension_state(&session_config).await {
                                            warn!("Failed to save extension state after runtime changes: {}", e);
                                        }
                                        tools_updated = true;
                                    }
                                }

                                // DeepSeek and Kimi need the turn's thinking on every split
                                // tool-call message; fix_conversation removes the signed copies.
                                let is_thinking = |c: &MessageContent| {
                                    matches!(
                                        c,
                                        MessageContent::Thinking(_)
                                            | MessageContent::RedactedThinking(_)
                                    )
                                };
                                let prior_thinking: Vec<MessageContent> = messages_to_add
                                    .iter()
                                    .filter(|m| m.role == response.role)
                                    .flat_map(|m| m.content.iter())
                                    .filter(|c| is_thinking(c))
                                    .cloned()
                                    .collect();
                                let direct_thinking: Vec<MessageContent> = response
                                    .content
                                    .iter()
                                    .filter(|c| is_thinking(c) && !prior_thinking.contains(c))
                                    .cloned()
                                    .collect();
                                let mut turn_thinking = prior_thinking;
                                turn_thinking.extend(direct_thinking.iter().cloned());

                                let response_message_id = response
                                    .id
                                    .as_deref()
                                    .expect("provider stream responses have IDs");
                                let is_response_message = |message: &Message| {
                                    message.id.as_deref() == Some(response_message_id)
                                };
                                let first_tool_call_id = tool_requests
                                    .first()
                                    .map(|request| request.id.as_str());
                                // A same-id prefix at the tail coalesces with the first request on
                                // push, so tool-pair hiding removes the thinking with the call.
                                let carrier_tool_call_id = match messages_to_add.messages().last() {
                                    Some(last) if is_response_message(last) => first_tool_call_id,
                                    _ if messages_to_add.iter().any(is_response_message) => None,
                                    _ => first_tool_call_id,
                                };
                                preferred_turn_usage_message_id =
                                    Some(response_message_id.to_owned());

                                for (index, request) in tool_requests.iter().enumerate() {
                                    let mut request_msg =
                                        if carrier_tool_call_id == Some(request.id.as_str()) {
                                            Message::assistant().with_id(response_message_id)
                                        } else {
                                            Message::assistant().with_generated_id()
                                        };

                                    let thinking = if index == 0 {
                                        &direct_thinking
                                    } else {
                                        &turn_thinking
                                    };
                                    for thinking in thinking {
                                        request_msg = request_msg.with_content(thinking.clone());
                                    }

                                    // For an unparseable tool call (Err), store a valid
                                    // placeholder Ok tool-call in history instead of the Err. This
                                    // keeps the conversation well-formed through EVERY provider
                                    // formatter's normal Ok path — so we don't have to special-case
                                    // each formatter's Err arm — and preserves provider metadata
                                    // (e.g. thought signatures), which is passed through below and
                                    // copied by the Ok path. The actual parse error rides on the
                                    // paired tool response.
                                    let history_tool_call = match &request.tool_call {
                                        Ok(_) => request.tool_call.clone(),
                                        Err(_) => Ok(CallToolRequestParams::new(
                                            "unparseable_tool_call",
                                        )
                                        .with_arguments(serde_json::Map::new())),
                                    };
                                    request_msg = request_msg
                                        .with_tool_request_with_metadata(
                                            request.id.clone(),
                                            history_tool_call,
                                            request.metadata.as_ref(),
                                            request.tool_meta.clone(),
                                        );

                                    let final_response = match &request.tool_call {
                                        Ok(_) => request_to_response_map
                                            .remove(&request.id)
                                            .unwrap_or_else(|| Message::user().with_generated_id()),
                                        Err(error) => {
                                            error!("Tool call could not be parsed: {error}");
                                            let mut response = request_to_response_map
                                                .remove(&request.id)
                                                .unwrap_or_else(|| Message::user().with_generated_id());
                                            // Only feed the parse error back if this id isn't
                                            // already answered. In Chat mode the skip branch above
                                            // already added a tool response for it; adding another
                                            // here would duplicate the tool_call_id (which strict
                                            // providers reject).
                                            let already_answered = response.content.iter().any(|c| {
                                                matches!(c, MessageContent::ToolResponse(r) if r.id == request.id)
                                            });
                                            if !already_answered {
                                                response.add_tool_response_with_metadata(
                                                    request.id.clone(),
                                                    Err(error.clone()),
                                                    request.metadata.as_ref(),
                                                );
                                            }
                                            response
                                        }
                                    };

                                    // Response placeholder is created before tools run, so clamp request to avoid inverted ordering.
                                    if request_msg.created > final_response.created {
                                        request_msg.created = final_response.created;
                                    }
                                    messages_to_add.push(request_msg);
                                    yield AgentEvent::Message(project_message_for_user_event(&final_response));
                                    messages_to_add.push(final_response);
                                }

                                no_tools_called = false;
                            }
                        }
                        #[allow(unused_variables)]
                        Err(ref provider_err @ ProviderError::ContextLengthExceeded(_)) => {
                            provider_errored = true;
                            #[cfg(feature = "telemetry")]
                            crate::posthog::emit_error(provider_err.telemetry_type(), &provider_err.to_string());
                            compaction_attempts += 1;

                            if compaction_attempts >= 2 {
                                error!("Context limit exceeded after compaction - prompt too large");
                                yield AgentEvent::Message(
                                    Message::assistant().with_system_notification(
                                        SystemNotificationType::InlineMessage,
                                        "Unable to continue: Context limit still exceeded after compaction. Try using a shorter message, a model with a larger context window, or start a new session."
                                    )
                                );
                                break;
                            }

                            yield AgentEvent::Message(
                                Message::assistant().with_system_notification(
                                    SystemNotificationType::InlineMessage,
                                    "Context limit reached. Compacting to continue conversation...",
                                )
                            );
                            yield AgentEvent::Message(
                                Message::assistant().with_system_notification(
                                    SystemNotificationType::ProgressMessage,
                                    COMPACTION_PROGRESS_TEXT,
                                )
                            );

                            match compact_messages(
                                self.provider().await?.as_ref(),
                                &model_config,
                                &session_config.id,
                                &conversation,
                                false,
                            )
                            .await
                            {
                                Ok(compaction) => {
                                    session_manager.replace_conversation(&session_config.id, &compaction.conversation).await?;
                                    self.update_session_metrics(&session_config.id, session_config.schedule_id.clone(), &compaction.usage, Some(compaction.retained_context_tokens)).await?;
                                    conversation = compaction.conversation;
                                    did_recovery_compact_this_iteration = true;
                                    yield AgentEvent::HistoryReplaced(conversation.clone());
                                    break;
                                }
                                Err(e) => {
                                    #[cfg(feature = "telemetry")]
                                    crate::posthog::emit_error("compaction_failed", &e.to_string());
                                    error!("Compaction failed: {}", e);
                                    yield AgentEvent::Message(
                                        Message::assistant().with_text(
                                            format!("Ran into this error trying to compact: {e}.\n\nPlease try again or create a new session")
                                        )
                                    );
                                    break;
                                }
                            }
                        }
                        Err(ref provider_err @ ProviderError::CreditsExhausted { details: _, ref top_up_url }) => {
                            provider_errored = true;
                            #[cfg(feature = "telemetry")]
                            crate::posthog::emit_error(provider_err.telemetry_type(), &provider_err.to_string());
                            error!("Error: {}", provider_err);

                            let user_msg = if top_up_url.is_some() {
                                "Please add credits to your account, then resend your message to continue.".to_string()
                            } else {
                                "Please check your account with your provider to add more credits, then resend your message to continue.".to_string()
                            };

                            let notification_data = serde_json::json!({
                                "top_up_url": top_up_url,
                            });

                            yield AgentEvent::Message(
                                Message::assistant().with_system_notification_with_data(
                                    SystemNotificationType::CreditsExhausted,
                                    user_msg,
                                    notification_data,
                                )
                            );
                            break;
                        }
                        Err(ref provider_err @ ProviderError::Refusal { ref details, ref category }) => {
                            provider_errored = true;
                            #[cfg(feature = "telemetry")]
                            crate::posthog::emit_error(provider_err.telemetry_type(), &provider_err.to_string());
                            error!("Error: {}", provider_err);

                            let category = category.as_deref().map(|c| format!("\n\nCategory: {c}")).unwrap_or_default();
                            yield AgentEvent::Message(Message::assistant().with_text(format!(
                                "The provider refused this request.\n\n{details}{category}\n\nPlease start a new session to continue — resending this conversation is likely to be refused again."
                            )));
                            // A refusal is terminal: skip goal/grind nudges and
                            // recipe retry_config, which would resend the same
                            // refused conversation.
                            exit_chat = true;
                            break;
                        }
                        Err(ref provider_err @ ProviderError::Authentication(_)) => {
                            provider_errored = true;
                            #[cfg(feature = "telemetry")]
                            crate::posthog::emit_error(provider_err.telemetry_type(), &provider_err.to_string());
                            error!("Error: {}", provider_err);
                            let message = persist_and_push_message_with_id(
                                &session_manager,
                                &session_config.id,
                                &mut conversation,
                                Message::from_provider_error(provider_err),
                            )
                            .await?;
                            yield AgentEvent::Message(message);
                            break;
                        }
                        Err(ref provider_err @ ProviderError::NetworkError(_)) => {
                            provider_errored = true;
                            #[cfg(feature = "telemetry")]
                            crate::posthog::emit_error(provider_err.telemetry_type(), &provider_err.to_string());
                            error!("Error: {}", provider_err);
                            yield AgentEvent::Message(
                                Message::assistant().with_text(
                                    format!("{provider_err}\n\nPlease resend your message to try again.")
                                )
                            );
                            break;
                        }
                        Err(ref provider_err) => {
                            provider_errored = true;
                            #[cfg(feature = "telemetry")]
                            crate::posthog::emit_error(provider_err.telemetry_type(), &provider_err.to_string());
                            error!("Error: {}", provider_err);
                            yield AgentEvent::Message(
                                Message::assistant().with_text(
                                    format!("Ran into this error: {provider_err}.\n\nPlease retry if you think this is a transient or recoverable error.")
                                )
                            );
                            break;
                        }
                    }
                }
                can_drain_pending_steers = true;

                if tools_updated {
                    (tools, toolshim_tools, system_prompt, _) =
                        self.prepare_tools_and_prompt(&session_config.id, &session.working_dir).await?;
                }

                {
                    let has_new_hints = self
                        .prompt_manager
                        .lock()
                        .await
                        .load_subdirectory_hints(&working_dir);
                    if has_new_hints && !tools_updated {
                        (tools, toolshim_tools, system_prompt, _) =
                            self.prepare_tools_and_prompt(&session_config.id, &session.working_dir).await?;
                    }
                }

                // An empty provider response — no tool calls, no text, and no error
                // or recovery compaction that legitimately produces no assistant
                // output — must never be persisted: strict providers reject a
                // conversation that contains an empty assistant turn. Drop it here
                // regardless of what the match below decides to do about the turn
                // (final-output nudge, steer, goal/grind, retry, or fallback).
                let empty_response = no_tools_called
                    && !exit_chat
                    && !provider_errored
                    && !did_recovery_compact_this_iteration
                    && !provider_reached_output_token_limit
                    && !provider_produced_content
                    && last_assistant_text.is_empty();

                if empty_response {
                    messages_to_add = Conversation::default();
                } else {
                    empty_turn_retries = 0;
                }

                if no_tools_called && !exit_chat {
                    // Lock, extract state, drop guard before branching — handle_retry_logic
                    // also locks final_output_tool and tokio::sync::Mutex is not reentrant.
                    let final_output = {
                        let mut guard = self.final_output_tool.lock().await;
                        guard.as_mut().map(|fot| fot.final_output.take())
                    };

                    match final_output {
                        Some(None) => {
                            warn!("Final output tool has not been called yet. Continuing agent loop.");
                            let message = push_message_with_id(
                                &mut messages_to_add,
                                Message::user().with_text(FINAL_OUTPUT_CONTINUATION_MESSAGE),
                            );
                            yield AgentEvent::Message(message);
                        }
                        Some(Some(output)) => {
                            pending_final_output = Some(output);
                            exit_chat = true;
                        }
                        None if did_recovery_compact_this_iteration => {
                            // continue from last user message after recovery compact
                        }
                        None if self.has_pending_steers(&session_config.id).await => {}
                        None if self.goal.lock().await.is_some() && !goal_check_pending => {
                            goal_check_pending = true;
                            let goal = self.goal.lock().await.clone().unwrap();
                            let nudge = format!(
                                "Before finishing, check whether the following goal has been fully met:\n\n\
                                 **Goal:** {goal}\n\n\
                                 If not, continue working toward it."
                            );
                            let message = Message::user().with_text(&nudge)
                                .with_visibility(false, true);
                            push_message_with_id(&mut messages_to_add, message);
                            yield AgentEvent::Message(
                                Message::assistant().with_system_notification(
                                    SystemNotificationType::InlineMessage,
                                    format!("Goal: {goal}"),
                                )
                            );
                        }

                        None if self.grind.lock().await.is_some() => {
                            let grind = self.grind.lock().await.clone().unwrap();
                            let nudge = format!(
                                "Keep working. The grind goal is not yet complete:\n\n\
                                 **Goal:** {grind}\n\n\
                                 Continue until it is fully done."
                            );
                            let message = Message::user().with_text(&nudge)
                                .with_visibility(false, true);
                            push_message_with_id(&mut messages_to_add, message);
                            yield AgentEvent::Message(
                                Message::assistant().with_system_notification(
                                    SystemNotificationType::InlineMessage,
                                    format!("Grind: {grind}"),
                                )
                            );
                        }

                        None => {
                            self.set_goal(None).await;
                            self.set_grind(None).await;
                            // Recipe retry logic owns the turn whenever a
                            // retry_config is present: it runs success checks,
                            // on_failure, and max_retries. Only when no recipe
                            // retry is configured (Skipped) does the empty-turn
                            // fallback apply.
                            match self.handle_retry_logic(&mut conversation, &session_config, &initial_messages).await {
                                Ok(RetryResult::Retried) => {
                                    info!("Retry logic triggered, restarting agent loop");
                                    messages_to_add = Conversation::default();
                                    session_manager.replace_conversation(&session_config.id, &conversation).await?;
                                    yield AgentEvent::HistoryReplaced(conversation.clone());
                                }
                                Ok(RetryResult::Skipped)
                                    if empty_response
                                        && !ends_with_successful_tool_response(conversation.messages()) => {
                                    // No recipe retry configured, and this empty
                                    // turn would otherwise fall through to a
                                    // silent exit. Retry a bounded number of
                                    // times, then surface a visible message so
                                    // the user is never left with no response.
                                    if empty_turn_retries < MAX_EMPTY_TURN_RETRIES {
                                        empty_turn_retries += 1;
                                        retrying_after_empty_turn = true;
                                        warn!(
                                            "Provider returned an empty response; retrying ({}/{})",
                                            empty_turn_retries, MAX_EMPTY_TURN_RETRIES
                                        );
                                    } else {
                                        warn!("Provider returned an empty response after retries; ending turn");
                                        last_assistant_text = EMPTY_TURN_MESSAGE.to_string();
                                        let message = push_message_with_id(
                                            &mut messages_to_add,
                                            Message::assistant().with_text(EMPTY_TURN_MESSAGE),
                                        );
                                        yield AgentEvent::Message(message);
                                        exit_chat = true;
                                    }
                                }
                                Ok(RetryResult::MaxAttemptsReached(message)) => {
                                    // Surface and persist the failure message
                                    // through the normal path so recipes don't
                                    // exit silently when retries are exhausted.
                                    let message = push_message_with_id(&mut messages_to_add, message);
                                    last_assistant_text = message.as_concat_text();
                                    yield AgentEvent::Message(message);
                                    exit_chat = true;
                                }
                                Ok(_) => {
                                    exit_chat = true;
                                }
                                Err(e) => {
                                    error!("Retry logic failed: {}", e);
                                    yield AgentEvent::Message(
                                        Message::assistant().with_text(
                                            format!("Retry logic encountered an error: {}", e)
                                        )
                                    );
                                    exit_chat = true;
                                }
                            }
                        }
                    }
                }

                if is_token_cancelled(&cancel_token)
                    && let Some(ref task) = tool_pair_summarization_task {
                        task.abort();
                    }

                if let Some(task) = tool_pair_summarization_task {
                    tool_pair_summarization_done = true;
                    if let Ok(summaries) = task.await {
                        for (summary_msg, tool_id) in summaries {
                            let matching_ids: Vec<String> = conversation.messages()
                                .iter()
                                .filter(|msg| {
                                    msg.id.is_some() && msg.content.iter().any(|c| match c {
                                        MessageContent::ToolRequest(req) => req.id == tool_id,
                                        MessageContent::ToolResponse(resp) => resp.id == tool_id,
                                        _ => false,
                                    })
                                })
                                .filter_map(|msg| msg.id.clone())
                                .collect();

                            if matching_ids.len() == 2 {
                                for id in &matching_ids {
                                    session_manager.update_message_metadata(&session_config.id, id, |metadata| {
                                        metadata.with_agent_invisible()
                                    }).await?;
                                }
                                session_manager.add_message(&session_config.id, &summary_msg).await?;
                            } else {
                                warn!("Expected a tool request/reply pair, but found {} matching messages",
                                    matching_ids.len());
                            }
                        }
                    }
                }

                if let Some(output) = pending_final_output.take() {
                    preferred_turn_usage_message_id = None;
                    last_assistant_text = output.clone();
                    let message = push_message_with_id(
                        &mut messages_to_add,
                        Message::assistant().with_text(output),
                    );
                    yield AgentEvent::Message(message);
                }

                let mut messages_to_add = if let Some(ref inference) = inference {
                    Conversation::new_unvalidated(
                        messages_to_add
                            .into_iter()
                            .map(|message| message.with_inference_if_assistant(inference.clone())),
                    )
                } else {
                    messages_to_add
                };

                if let Some(usage) = pending_turn_usage.take()
                    && let Some((message_id, usage)) = attach_turn_usage(
                        &mut messages_to_add,
                        &usage,
                        preferred_turn_usage_message_id.as_deref(),
                    ) {
                        yield AgentEvent::MessageUsage { message_id, usage };
                    }

                for msg in &messages_to_add {
                    session_manager.add_message(&session_config.id, msg).await?;
                }
                conversation.extend(messages_to_add);

                if exit_chat && self.has_pending_steers(&session_config.id).await {
                    exit_chat = false;
                }

                if exit_chat {
                    match self
                        .emit_stop_hook_blocking(&session_config.id, &last_assistant_text, &session.working_dir.to_string_lossy())
                        .await
                    {
                        hooks::HookDecision::Allow => {
                            stop_hook_handled_for_exit = true;
                            break;
                        }
                        hooks::HookDecision::Deny { reason, plugin } => {
                            consecutive_stop_hook_blocks += 1;
                            if consecutive_stop_hook_blocks > stop_hook_block_cap {
                                let message = persist_message_with_id(
                                    &session_manager,
                                    &session_config.id,
                                    stop_hook_block_cap_warning(&plugin, stop_hook_block_cap),
                                )
                                .await?;
                                yield AgentEvent::Message(message);
                                stop_hook_handled_for_exit = true;
                                break;
                            }
                            persist_and_push_message_with_id(
                                &session_manager,
                                &session_config.id,
                                &mut conversation,
                                stop_hook_denial_context_message(&plugin, &reason),
                            )
                            .await?;
                            yield AgentEvent::Message(stop_hook_denial_notification(&plugin));
                            retrying_after_stop_hook_denial = true;
                        }
                    }
                }

                tokio::task::yield_now().await;
            }

            if !last_assistant_text.is_empty()
                && gen_ai_telemetry::capture_message_content()
            {
                tracing::Span::current().record("trace_output", last_assistant_text.as_str());
                let output_json = gen_ai_telemetry::simple_output_json(&last_assistant_text);
                tracing::Span::current().record("gen_ai.output.messages", output_json.as_str());
                reply_span.record("gen_ai.output.messages", output_json.as_str());
            }
            gen_ai_telemetry::record_usage(&tracing::Span::current(), &turn_total_usage);
            gen_ai_telemetry::record_usage(&reply_span, &turn_total_usage);

            if !stop_hook_handled_for_exit {
                self.emit_stop_hook(&session_config.id, &last_assistant_text, &session.working_dir.to_string_lossy()).await;
            }
        }.instrument(reply_stream_span));
        Ok(inner)
    }

    pub async fn extend_system_prompt(&self, key: String, instruction: String) {
        let mut prompt_manager = self.prompt_manager.lock().await;
        prompt_manager.add_system_prompt_extra(key, instruction);
    }

    pub async fn remove_system_prompt_extra(&self, key: &str) {
        let mut prompt_manager = self.prompt_manager.lock().await;
        prompt_manager.remove_system_prompt_extra(key);
    }

    pub async fn set_goal(&self, goal: Option<String>) {
        *self.goal.lock().await = goal;
    }

    pub async fn get_goal(&self) -> Option<String> {
        self.goal.lock().await.clone()
    }

    pub async fn set_grind(&self, goal: Option<String>) {
        *self.grind.lock().await = goal;
    }

    pub async fn get_grind(&self) -> Option<String> {
        self.grind.lock().await.clone()
    }

    pub async fn update_provider(
        &self,
        provider: Arc<dyn Provider>,
        model_config: bcaip_provider_types::model::ModelConfig,
        session_id: &str,
    ) -> Result<()> {
        let provider_name = provider.get_name().to_string();
        let registry_entry = providers::get_from_registry(&provider_name).await.ok();

        let model_config = if registry_entry.is_some() {
            model_config::materialize_model_config(&provider_name, model_config.clone())
                .unwrap_or(model_config)
        } else {
            model_config
        };
        let effort_support = provider.thinking_effort_support();
        let model_config = normalize_legacy_provider_thinking_effort(model_config, &effort_support);
        let effective_model_config = match registry_entry {
            Some(entry) => entry
                .normalize_model_config(model_config.clone())
                .unwrap_or_else(|_| model_config.clone()),
            None => model_config.clone(),
        };

        {
            let mut current_provider = self.provider.lock().await;
            *current_provider = Some(Arc::clone(&provider));
        }

        // A freshly created provider that manages its own model starts on its
        // own default, so the session's selection has to be pushed to it before
        // the next config snapshot is built. Failures are not fatal here: the
        // selection is re-applied at stream time.
        if let Err(e) = provider
            .apply_model_selection(&effective_model_config)
            .await
        {
            warn!("Failed to apply model selection to provider: {e}");
        }

        self.config
            .session_manager
            .clone()
            .update(session_id)
            .provider_name(&provider_name)
            .model_config(model_config)
            .apply()
            .await
            .context("Failed to persist provider config to session")
    }

    pub async fn update_goose_mode(&self, mode: GooseMode, session_id: &str) -> Result<()> {
        if let Some(provider) = self.provider.lock().await.as_ref() {
            provider
                .update_mode(session_id, mode)
                .await
                .map_err(|e| anyhow::anyhow!("Provider rejected mode update: {e}"))?;
        }
        *self.current_goose_mode.lock().await = mode;
        self.config
            .session_manager
            .clone()
            .update(session_id)
            .goose_mode(mode)
            .apply()
            .await
            .context("Failed to persist goose_mode to session")
    }

    pub async fn goose_mode(&self) -> GooseMode {
        *self.current_goose_mode.lock().await
    }

    pub async fn recreate_provider_for_session(
        &self,
        session_id: &str,
        provider_name: &str,
        model_config: bcaip_provider_types::model::ModelConfig,
    ) -> Result<()> {
        let session = self
            .config
            .session_manager
            .get_session(session_id, false)
            .await
            .context("Failed to get session")?;

        let extensions = EnabledExtensionsState::extensions_or_default(
            Some(&session.extension_data),
            Config::global(),
        );

        let provider = providers::create_with_working_dir(
            provider_name,
            extensions,
            session.working_dir.clone(),
        )
        .await
        .map_err(|error| provider_creation_error(error, "Could not create provider"))?;

        self.update_provider(provider, model_config, session_id)
            .await?;

        let mode = self.goose_mode().await;
        self.update_goose_mode(mode, session_id).await
    }

    /// Apply a thinking-effort selection. `effort` is the raw option value: a
    /// provider that manages effort through a harness has its own vocabulary,
    /// which is not always a `ThinkingEffort` member.
    pub async fn update_thinking_effort(&self, session_id: &str, effort: &str) -> Result<()> {
        let current_provider = self.provider().await?;
        // Context rather than a formatted string: the caller distinguishes a
        // value rejection from an operational failure by downcasting to
        // `ProviderError`, which stringifying would destroy.
        let provider_handled = current_provider
            .set_thinking_effort(session_id, effort)
            .await
            .context("Provider rejected thinking effort update")?;

        let model_config = self.model_config_for_session(session_id).await?;

        if provider_handled {
            // The provider applied the value live; recreating it would discard
            // the very session state we just configured.
            let model_config = model_config.with_merged_request_params(HashMap::from([(
                "thinking_effort".to_string(),
                Value::String(effort.to_string()),
            )]));
            return self
                .config
                .session_manager
                .clone()
                .update(session_id)
                .model_config(model_config)
                .apply()
                .await
                .context("Failed to persist thinking effort to session");
        }

        let effort = effort.parse::<ThinkingEffort>().map_err(|_| {
            anyhow::Error::new(ProviderError::InvalidValue(format!(
                "Invalid thinking effort: {effort}"
            )))
        })?;
        let provider_name = current_provider.get_name().to_string();
        self.recreate_provider_for_session(
            session_id,
            &provider_name,
            model_config.with_thinking_effort(effort),
        )
        .await
    }

    /// Restore the provider from session data or fall back to global config
    /// This is used when resuming a session to restore the provider state
    /// Returns true if the session's provider was replaced with a fallback.
    pub async fn restore_provider_from_session(&self, session: &Session) -> Result<bool> {
        let config = Config::global();

        let provider_name = session
            .provider_name
            .clone()
            .or_else(|| config.get_goose_provider().ok())
            .ok_or_else(|| anyhow!("Could not configure agent: missing provider"))?;

        let mut model_config = match session.model_config.clone() {
            Some(saved_config) => crate::model_config::with_rederived_cache_ttl(saved_config)
                .map_err(|e| anyhow!("Could not configure agent: {}", e))?,
            None => {
                let model_name = config
                    .get_goose_model()
                    .ok()
                    .ok_or_else(|| anyhow!("Could not configure agent: missing model"))?;
                model_config::model_config_from_user_config(&provider_name, &model_name)
                    .map_err(|e| anyhow!("Could not configure agent: invalid model {}", e))?
            }
        };

        // if the saved model is the ACP sentinel "current", only preserve this if the provider
        // uses this sentinel to indicate it's an ACP provider that manages its model
        if model_config.model_name == acp::ACP_CURRENT_MODEL
            && let Ok(entry) = providers::get_from_registry(&provider_name).await
            && entry.metadata().default_model != acp::ACP_CURRENT_MODEL
        {
            model_config = model_config::model_config_from_user_config(
                &provider_name,
                &entry.metadata().default_model,
            )
            .map_err(|e| anyhow!("Could not resolve default model: {}", e))?;
        }

        let extensions =
            EnabledExtensionsState::extensions_or_default(Some(&session.extension_data), config);

        let (provider, active_model_config, provider_changed) = if providers::get_from_registry(
            &provider_name,
        )
        .await
        .is_ok()
        {
            let p = providers::create_with_working_dir(
                &provider_name,
                extensions,
                session.working_dir.clone(),
            )
            .await
            .map_err(|error| provider_creation_error(error, "Could not create provider"))?;
            (p, model_config, false)
        } else {
            let fallback_provider_name = config
                .get_goose_provider()
                .ok()
                .filter(|name| name != &provider_name)
                .ok_or_else(|| {
                    anyhow!(
                        "Could not create provider: provider '{}' not found",
                        provider_name
                    )
                })?;

            tracing::warn!(
                "Session provider '{}' unavailable, falling back to '{}'",
                provider_name,
                fallback_provider_name
            );

            let fallback_model_name = config
                .get_goose_model()
                .ok()
                .ok_or_else(|| anyhow!("Could not configure fallback provider: missing model"))?;
            let fallback_model_config = model_config::model_config_from_user_config(
                &fallback_provider_name,
                &fallback_model_name,
            )
            .map_err(|e| anyhow!("Could not configure fallback provider: invalid model {}", e))?;

            let fallback_provider = providers::create_with_working_dir(
                    &fallback_provider_name,
                    extensions,
                    session.working_dir.clone(),
                )
                .await
                .map_err(|error| {
                    provider_creation_error(
                        error,
                        format!(
                            "Could not create provider '{provider_name}' or fallback '{fallback_provider_name}'"
                        ),
                    )
                })?;

            if let Err(e) = self
                .config
                .session_manager
                .update(&session.id)
                .provider_name(&fallback_provider_name)
                .model_config(fallback_model_config.clone())
                .apply()
                .await
            {
                tracing::warn!("Failed to update session provider: {}", e);
            }

            (fallback_provider, fallback_model_config, true)
        };

        self.update_provider(provider, active_model_config, &session.id)
            .await?;
        // Propagate session mode to the new provider
        if let Some(provider) = self.provider.lock().await.as_ref() {
            provider
                .update_mode(&session.id, session.goose_mode)
                .await
                .map_err(|e| anyhow!("Failed to propagate mode to provider: {}", e))?;
        }
        *self.current_goose_mode.lock().await = session.goose_mode;
        Ok(provider_changed)
    }

    /// Override the system prompt with a custom template
    pub async fn override_system_prompt(&self, template: String) {
        let mut prompt_manager = self.prompt_manager.lock().await;
        prompt_manager.set_system_prompt_override(template);
    }

    pub async fn clear_system_prompt_override(&self) {
        let mut prompt_manager = self.prompt_manager.lock().await;
        prompt_manager.clear_system_prompt_override();
    }

    pub async fn list_extension_prompts(&self, session_id: &str) -> HashMap<String, Vec<Prompt>> {
        self.extension_manager
            .list_prompts(session_id, CancellationToken::default())
            .await
            .expect("Failed to list prompts")
    }

    pub async fn get_prompt(
        &self,
        session_id: &str,
        name: &str,
        arguments: Value,
    ) -> Result<GetPromptResult> {
        // First find which extension has this prompt
        let prompts = self
            .extension_manager
            .list_prompts(session_id, CancellationToken::default())
            .await
            .map_err(|e| anyhow!("Failed to list prompts: {}", e))?;

        if let Some(extension) = prompts
            .iter()
            .find(|(_, prompt_list)| prompt_list.iter().any(|p| p.name == name))
            .map(|(extension, _)| extension)
        {
            return self
                .extension_manager
                .get_prompt(
                    session_id,
                    extension,
                    name,
                    arguments,
                    CancellationToken::default(),
                )
                .await
                .map_err(|e| anyhow!("Failed to get prompt: {}", e));
        }

        Err(anyhow!("Prompt '{}' not found", name))
    }
}

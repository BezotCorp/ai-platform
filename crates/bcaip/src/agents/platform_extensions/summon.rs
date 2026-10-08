use crate::agents::mcp_client::{Error, McpClientTrait};
use crate::agents::subagent_handler::{OnMessageCallback, SubagentRunParams, run_subagent_task};
use crate::agents::subagent_task_config::{DEFAULT_SUBAGENT_MAX_TURNS, TaskConfig};
use crate::agents::tool_execution::{ToolCallContext, ToolCallNotificationEmitter};
use crate::agents::{AgentConfig, extension::PlatformExtensionContext};
use crate::config::Config;
use crate::config::paths::Paths;
use crate::recipe::{RECIPE_FILE_EXTENSIONS, Recipe, RecipeParameter, Settings};
use crate::{
    providers,
    recipe::{build_recipe::build_recipe_from_template, local_recipes::load_local_recipe_file},
};
use crate::{
    session::{EnabledExtensionsState, SessionType},
    sources::parse_frontmatter,
    utils::safe_truncate,
};
use anyhow::Result;
use async_trait::async_trait;
use bcaip_agent::operation::messages_since_kickoff;
use bcaip_provider_types::bcaip_mode::BcaipMode;
use bcaip_sdk_types::custom_requests::{SourceEntry, SourceType};
use rmcp::model::{
    CallToolResult, ContentBlock, Implementation, InitializeResult, JsonObject, ListToolsResult,
    MetaObject, Role, ServerCapabilities, ServerNotification, Tool,
};
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{collections::HashMap, future::Future};
use tokio::{sync::Mutex, task::JoinHandle};
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub static EXTENSION_NAME: &str = "summon";

const SUBAGENT_DESCRIPTION_BUDGET: usize = 160;

const TASK_LABEL_BUDGET: usize = 60;

fn durable_assistant_turn_count(
    conversation: &bcaip_provider_types::conversations::Conversation,
) -> u32 {
    let Ok(messages) = messages_since_kickoff(conversation) else {
        return 0;
    };
    let mut turns = 0;
    let mut in_assistant_block = false;
    for message in messages.iter().rev() {
        // Compaction summaries and continuation prompts are assistant-role
        // scaffolding, but are not user-visible task turns. Ignore them
        // without merging across a hidden user replay below.
        if message.role == Role::Assistant && !message.is_user_visible() {
            continue;
        }
        if message.role == Role::Assistant {
            if !in_assistant_block {
                turns += 1;
                in_assistant_block = true;
            }
        } else {
            in_assistant_block = false;
        }
    }
    turns
}

fn kind_plural(kind: SourceType) -> &'static str {
    match kind {
        SourceType::Subrecipe => "Subrecipes",
        SourceType::Recipe => "Recipes",
        SourceType::Agent => "Agents",
        _ => "Other",
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct DelegateParams {
    pub instructions: Option<String>,
    pub source: Option<String>,
    pub parameters: Option<HashMap<String, serde_json::Value>>,
    pub extensions: Option<Vec<String>>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub temperature: Option<f32>,
    pub max_turns: Option<usize>,
    pub context: Option<String>,
    pub working_dir: Option<String>,
    #[serde(default)]
    pub r#async: bool,
}

pub struct BackgroundTask {
    pub id: String,
    pub description: String,
    pub started_at: Instant,
    pub turns: Arc<AtomicU32>,
    pub last_activity: Arc<AtomicU64>,
    pub handle: JoinHandle<Result<String>>,
    pub cancellation_token: CancellationToken,
    completion_token: CancellationToken,
    notification_sink: SharedNotificationSink,
}

fn spawn_background_task<F>(future: F) -> (JoinHandle<Result<String>>, CancellationToken)
where
    F: Future<Output = Result<String>> + Send + 'static,
{
    let completion_token = CancellationToken::new();
    let completion_guard = completion_token.clone().drop_guard();
    let handle = tokio::spawn(async move {
        let _completion_guard = completion_guard;
        future.await
    });
    (handle, completion_token)
}

pub struct CompletedTask {
    pub id: String,
    pub description: String,
    pub result: Result<String, String>,
    pub turns_taken: u32,
    pub duration: Duration,
    pub completed_at: Instant,
    notification_sink: SharedNotificationSink,
}

enum NotificationSink {
    Buffer(Vec<ServerNotification>),
    Emitter(ToolCallNotificationEmitter),
}

type SharedNotificationSink = Arc<Mutex<NotificationSink>>;

async fn yield_to_outer_tool_stream() {
    // The outer select may have polled its receiver before this future queues a
    // notification. Keep the result pending for the following select pass so
    // the now-ready receiver is observed before the terminal result.
    tokio::task::yield_now().await;
    tokio::task::yield_now().await;
}

impl NotificationSink {
    fn route(&mut self, notification: ServerNotification) {
        match self {
            Self::Buffer(buffer) => buffer.push(notification),
            Self::Emitter(emitter) => emitter.emit_best_effort(notification),
        }
    }

    async fn attach(&mut self, emitter: Option<ToolCallNotificationEmitter>) {
        let Some(emitter) = emitter else {
            return;
        };
        while let Self::Buffer(buffered) = self {
            let Some(notification) = buffered.first().cloned() else {
                break;
            };
            emitter.emit_best_effort(notification);
            yield_to_outer_tool_stream().await;
            buffered.remove(0);
        }
        *self = Self::Emitter(emitter);
    }

    fn detach(&mut self) {
        if matches!(self, Self::Emitter(_)) {
            *self = Self::Buffer(Vec::new());
        }
    }

    fn buffered_len(&self) -> usize {
        match self {
            Self::Buffer(buffer) => buffer.len(),
            Self::Emitter(_) => 0,
        }
    }
}

fn merge_subrecipe_parameters(
    fixed_values: Option<&HashMap<String, String>>,
    provided_parameters: Option<&HashMap<String, serde_json::Value>>,
) -> HashMap<String, String> {
    let mut merged = fixed_values.cloned().unwrap_or_default();
    if let Some(provided_parameters) = provided_parameters {
        for (key, value) in provided_parameters {
            let value = match value {
                serde_json::Value::String(value) => value.clone(),
                other => other.to_string(),
            };
            merged.entry(key.clone()).or_insert(value);
        }
    }
    merged
}

/// Result from handle_load_task_result with structured metadata for the caller
#[derive(Debug)]
struct TaskLoadResult {
    content: Vec<ContentBlock>,
    status: &'static str,
    turns: Option<u32>,
    duration_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct AgentMetadata {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    model: Option<String>,
}

fn parse_agent_content(content: &str, path: &Path) -> Option<SourceEntry> {
    let (metadata, body): (AgentMetadata, String) = match parse_frontmatter(content) {
        Ok(Some(parsed)) => parsed,
        Ok(None) => return None,
        Err(e) => {
            // Missing fields means this file has valid YAML but isn't an agent — skip silently.
            // Only warn on actual YAML syntax errors.
            if e.to_string().contains("missing field") {
                return None;
            }
            warn!("Failed to parse agent file {}: {}", path.display(), e);
            return None;
        }
    };

    let description = metadata.description.unwrap_or_else(|| {
        let model_info = metadata
            .model
            .as_ref()
            .map(|m| format!(" ({})", m))
            .unwrap_or_default();
        format!("Agent{}", model_info)
    });

    let mut properties = std::collections::HashMap::new();
    if let Some(model) = metadata.model {
        properties.insert("model".to_string(), serde_json::Value::String(model));
    }

    Some(SourceEntry {
        source_type: SourceType::Agent,
        name: metadata.name,
        description,
        content: body,
        path: path.to_string_lossy().into_owned(),
        global: false,
        writable: true,
        supporting_files: Vec::new(),
        properties,
    })
}

fn scan_recipes_from_dir(
    dir: &Path,
    kind: SourceType,
    suppress_config_warnings: bool,
    sources: &mut Vec<SourceEntry>,
    seen: &mut std::collections::HashSet<String>,
) {
    let Ok(source_dir) = dir.canonicalize() else {
        return;
    };
    let entries = match std::fs::read_dir(&source_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let path = source_dir.join(&file_name);

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if !RECIPE_FILE_EXTENSIONS.contains(&ext) {
            continue;
        }

        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();

        if name.is_empty() || seen.contains(&name) {
            continue;
        }

        let content = match crate::skills::read_source_file(&source_dir, Path::new(&file_name)) {
            Ok(content) => content,
            Err(error) => {
                warn!("Failed to read recipe {}: {}", path.display(), error);
                continue;
            }
        };

        match Recipe::from_content(&content) {
            Ok(recipe) => {
                seen.insert(name.clone());
                sources.push(SourceEntry {
                    source_type: kind,
                    name,
                    description: recipe.description.clone(),
                    content: recipe.instructions.clone().unwrap_or_default(),
                    path: path.to_string_lossy().into_owned(),
                    global: false,
                    writable: true,
                    supporting_files: Vec::new(),
                    properties: std::collections::HashMap::new(),
                });
            }
            Err(e) => {
                // The working directory commonly contains project config like package.json
                // and tsconfig.json, which parse as valid JSON but lack Recipe fields. In that
                // case treat them as "not a recipe" rather than warning. Dedicated recipe
                // directories still warn so a real recipe with a typo is not silently dropped.
                if suppress_config_warnings && e.to_string().contains("missing field") {
                    continue;
                }
                warn!("Failed to parse recipe {}: {}", path.display(), e);
            }
        }
    }
}

fn scan_agents_from_dir(
    dir: &Path,
    sources: &mut Vec<SourceEntry>,
    seen: &mut std::collections::HashSet<String>,
) {
    let Ok(source_dir) = dir.canonicalize() else {
        return;
    };
    let entries = match std::fs::read_dir(&source_dir) {
        Ok(e) => e,
        Err(_) => return,
    };

    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let path = source_dir.join(&file_name);

        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
        if ext != "md" {
            continue;
        }

        let content = match crate::skills::read_source_file(&source_dir, Path::new(&file_name)) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read agent file {}: {}", path.display(), e);
                continue;
            }
        };

        if let Some(source) = parse_agent_content(&content, &path)
            && !seen.contains(&source.name)
        {
            seen.insert(source.name.clone());
            sources.push(source);
        }
    }
}

pub fn discover_filesystem_sources(working_dir: &Path) -> Vec<SourceEntry> {
    let mut sources: Vec<SourceEntry> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    let home = dirs::home_dir();
    let config = Paths::config_dir();

    let local_recipe_dirs: Vec<PathBuf> = vec![
        working_dir.join(".bcaip/recipes"),
        working_dir.join(".agents/recipes"),
    ];

    let global_recipe_dirs: Vec<PathBuf> = std::env::var("BCAIP_RECIPE_PATH")
        .ok()
        .into_iter()
        .flat_map(|p| {
            let sep = if cfg!(windows) { ';' } else { ':' };
            p.split(sep).map(PathBuf::from).collect::<Vec<_>>()
        })
        .chain(
            [
                home.as_ref().map(|h| h.join(".bcaip/recipes")),
                Some(config.join("recipes")),
                home.as_ref().map(|h| h.join(".agents/recipes")),
            ]
            .into_iter()
            .flatten(),
        )
        .collect();

    let local_agent_dirs: Vec<PathBuf> = vec![
        working_dir.join(".bcaip/agents"),
        working_dir.join(".claude/agents"),
        working_dir.join(".agents/agents"),
    ];

    let global_agent_dirs: Vec<PathBuf> = [
        home.as_ref().map(|h| h.join(".bcaip/agents")),
        home.as_ref().map(|h| h.join(".agents/agents")),
        Some(config.join("agents")),
        home.as_ref().map(|h| h.join(".claude/agents")),
    ]
    .into_iter()
    .flatten()
    .collect();

    scan_recipes_from_dir(
        working_dir,
        SourceType::Recipe,
        true,
        &mut sources,
        &mut seen,
    );

    for dir in local_recipe_dirs {
        scan_recipes_from_dir(&dir, SourceType::Recipe, false, &mut sources, &mut seen);
    }

    for dir in local_agent_dirs {
        scan_agents_from_dir(&dir, &mut sources, &mut seen);
    }

    for dir in global_recipe_dirs {
        scan_recipes_from_dir(&dir, SourceType::Recipe, false, &mut sources, &mut seen);
    }

    for dir in global_agent_dirs {
        scan_agents_from_dir(&dir, &mut sources, &mut seen);
    }

    sources
}

fn build_instructions_with_context(context: &str, instructions: &str) -> String {
    let mut result = format!("# Reference Context\n\n{}", context);
    if !instructions.is_empty() {
        result.push_str(&format!("\n\n# Task Instructions\n\n{}", instructions));
    }
    result
}

fn build_subagent_instructions(session: Option<&crate::session::Session>) -> String {
    let Some(session) = session else {
        return String::new();
    };

    // filter the sources down to what we want even though currently that is what we get
    let mut sources: Vec<SourceEntry> = discover_filesystem_sources(&session.working_dir)
        .into_iter()
        .filter(|s| {
            matches!(
                s.source_type,
                SourceType::Agent | SourceType::Recipe | SourceType::Subrecipe
            )
        })
        .collect();

    // If the session is started from a recipe, also use the subrecipes for
    // that recipe as delegate targets
    if let Some(recipe) = session.recipe.as_ref()
        && let Some(subs) = recipe.sub_recipes.as_ref()
    {
        let mut seen: std::collections::HashSet<String> =
            sources.iter().map(|s| s.name.clone()).collect();
        for sr in subs {
            if !seen.insert(sr.name.clone()) {
                continue;
            }
            sources.push(SourceEntry {
                source_type: SourceType::Subrecipe,
                name: sr.name.clone(),
                description: sr.description.clone().unwrap_or_default(),
                content: String::new(),
                path: sr.path.clone(),
                global: false,
                writable: false,
                supporting_files: Vec::new(),
                properties: std::collections::HashMap::new(),
            });
        }
    }

    if sources.is_empty() {
        return String::new();
    }

    sources.sort_by(|a, b| (&a.source_type, &a.name).cmp(&(&b.source_type, &b.name)));
    let subagents: Vec<&SourceEntry> = sources.iter().collect();

    let names = subagents
        .iter()
        .map(|s| s.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let mut out = String::new();
    out.push_str(
        "\n\nThe following named subagents are available in this session and \
         can be invoked through the `delegate` tool (run as a subagent) or \
         the `load` tool (read their instructions into your own context):\n",
    );

    let mut current_kind: Option<SourceType> = None;
    for s in &subagents {
        if current_kind != Some(s.source_type) {
            out.push_str(&format!("\n{}:", kind_plural(s.source_type)));
            current_kind = Some(s.source_type);
        }
        out.push_str(&format!(
            "\n• {} — {}",
            s.name,
            safe_truncate(&s.description, SUBAGENT_DESCRIPTION_BUDGET)
        ));
    }

    out.push_str(&format!(
        "\n\nWhen to call a subagent (one of [{names}]):\n\
         • `@<name>` in the user's message — always call that subagent.\n\
         • The user mentions a subagent by name without `@` — infer from \
         context whether they want it invoked, and if so, call it.\n\
         • The user's request strongly matches a subagent's description — \
         call it.\n\n\
         Calling a subagent normally means `delegate(source: \"<name>\", \
         instructions: ...)`, which runs it as an isolated subagent and \
         returns its result. Use `load(source: \"<name>\")` instead if you \
         only want to read the subagent's instructions into your own \
         context. For long-running work, pass `async: true` to `delegate` — \
         it returns a task id immediately, and you collect the result later \
         with `load(source: \"<task_id>\")`, which waits for completion.",
    ));

    out
}

fn round_duration(d: Duration) -> String {
    let secs = d.as_secs();
    if secs < 60 {
        format!("{}s", (secs / 10) * 10)
    } else {
        format!("{}m", secs / 60)
    }
}

fn current_epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Get maximum number of concurrent background tasks
fn max_background_tasks() -> usize {
    Config::global()
        .get_param::<usize>("BCAIP_MAX_BACKGROUND_TASKS")
        .unwrap_or(5)
}

fn completed_task_ttl() -> Duration {
    let secs = Config::global()
        .get_param::<u64>("BCAIP_COMPLETED_TASK_TTL_SECS")
        .unwrap_or(600);
    Duration::from_secs(secs)
}

fn is_session_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('_').collect();
    parts.len() == 2 && parts[0].len() == 8 && parts[0].chars().all(|c| c.is_ascii_digit())
}

pub struct SummonClient {
    info: InitializeResult,
    context: PlatformExtensionContext,
    source_cache: Mutex<Option<(Instant, PathBuf, Vec<SourceEntry>)>>,
    background_tasks: Mutex<HashMap<String, BackgroundTask>>,
    completed_tasks: Mutex<HashMap<String, CompletedTask>>,
}

impl Drop for SummonClient {
    fn drop(&mut self) {
        // Best-effort cancellation of running tasks on shutdown
        if let Ok(tasks) = self.background_tasks.try_lock() {
            for task in tasks.values() {
                task.cancellation_token.cancel();
            }
        }
    }
}

impl SummonClient {
    pub fn new(context: PlatformExtensionContext) -> Result<Self> {
        let info = InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new(EXTENSION_NAME, "1.0.0").with_title("Summon"));

        Ok(Self {
            info,
            context,
            source_cache: Mutex::new(None),
            background_tasks: Mutex::new(HashMap::new()),
            completed_tasks: Mutex::new(HashMap::new()),
        })
    }

    async fn create_subagent_session(
        &self,
        task_config: &TaskConfig,
        name: String,
    ) -> Result<crate::session::Session, String> {
        let session = self
            .context
            .session_manager
            .create_session(
                task_config.parent_working_dir.clone(),
                name,
                SessionType::SubAgent,
                BcaipMode::Auto,
            )
            .await
            .map_err(|e| format!("Failed to create subagent session: {}", e))?;

        if !task_config.parent_session_id.is_empty() {
            self.context
                .session_manager
                .update(&session.id)
                .parent_session_id(Some(task_config.parent_session_id.clone()))
                .apply()
                .await
                .map_err(|e| format!("Failed to link subagent to parent session: {}", e))?;
        }

        Ok(session)
    }

    fn notification_sink(emitter: Option<ToolCallNotificationEmitter>) -> SharedNotificationSink {
        Arc::new(Mutex::new(match emitter {
            Some(emitter) => NotificationSink::Emitter(emitter),
            None => NotificationSink::Buffer(Vec::new()),
        }))
    }

    async fn attach_notification_emitter(
        sink: &SharedNotificationSink,
        emitter: Option<ToolCallNotificationEmitter>,
    ) {
        sink.lock().await.attach(emitter).await;
    }

    async fn run_subagent_with_notifications<Run, RunFuture>(
        sink: SharedNotificationSink,
        run_subagent: Run,
    ) -> Result<String>
    where
        Run: FnOnce(tokio::sync::mpsc::UnboundedSender<ServerNotification>) -> RunFuture,
        RunFuture: Future<Output = Result<String>>,
    {
        let (notification_tx, mut notification_rx) = tokio::sync::mpsc::unbounded_channel();
        let run = run_subagent(notification_tx);
        tokio::pin!(run);

        loop {
            tokio::select! {
                biased;
                result = &mut run => {
                    while let Ok(notification) = notification_rx.try_recv() {
                        sink.lock().await.route(notification);
                        yield_to_outer_tool_stream().await;
                    }
                    yield_to_outer_tool_stream().await;
                    return result;
                }
                Some(notification) = notification_rx.recv() => {
                    sink.lock().await.route(notification);
                    yield_to_outer_tool_stream().await;
                }
            }
        }
    }

    fn create_load_tool(&self) -> Tool {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "source": {
                    "type": "string",
                    "description": "Name of the source to load. If omitted, lists all available sources."
                },
                "cancel": {
                    "type": "boolean",
                    "default": false,
                    "description": "For running background tasks: cancel and return output."
                },
                "peek": {
                    "type": "boolean",
                    "default": false,
                    "description": "For running background tasks: check progress without blocking. Returns durable assistant-turn count, idle time, and recent tool activity."
                }
            }
        });

        Tool::new(
            "load",
            "Load knowledge into your current context or discover available sources.\n\n\
             Call with no arguments to list all available sources (subrecipes, recipes, agents).\n\
             Call with a source name to load its content into your context.\n\
             For background tasks: load(source: \"task_id\") waits for the task and returns the result.\n\
             To cancel a running task: load(source: \"task_id\", cancel: true) stops and returns output.\n\
             To check progress: load(source: \"task_id\", peek: true) returns status without blocking.\n\n\
             Examples:\n\
             - load() → Lists available sources\n\
             - load(source: \"deploy\") → Loads the deploy recipe\n\
             - load(source: \"20260219_1\") → Waits for background task, then returns result\n\
             - load(source: \"20260219_1\", peek: true) → Check task progress without waiting"
                .to_string(),
            schema.as_object().unwrap().clone(),
        )
    }

    fn create_delegate_tool(&self) -> Tool {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "instructions": {
                    "type": "string",
                    "description": "Task instructions. Required for ad-hoc tasks."
                },
                "source": {
                    "type": "string",
                    "description": "Name of a recipe or agent to run."
                },
                "parameters": {
                    "type": "object",
                    "additionalProperties": true,
                    "description": "Parameters for the source (only valid with source)."
                },
                "extensions": {
                    "type": "array",
                    "items": {"type": "string"},
                    "description": "Extensions to enable. Omit to inherit all, empty array for none."
                },
                "provider": {
                    "type": "string",
                    "description": "Override LLM provider."
                },
                "model": {
                    "type": "string",
                    "description": "Override model."
                },
                "temperature": {
                    "type": "number",
                    "description": "Override temperature."
                },
                "max_turns": {
                    "type": "integer",
                    "minimum": 1,
                    "description": "Maximum turns for this delegate. Overrides recipe settings.max_turns and BCAIP_SUBAGENT_MAX_TURNS."
                },
                "context": {
                    "type": "string",
                    "description": "Reference context to inject into the delegate's system prompt. Use for background information, file contents, or constraints the delegate needs but that aren't part of the task instructions."
                },
                "working_dir": {
                    "type": "string",
                    "description": "Working directory for the delegate. Must be within the parent session's working directory. Defaults to the parent's working directory."
                },
                "async": {
                    "type": "boolean",
                    "default": false,
                    "description": "Run in background (default: false)."
                }
            }
        });

        Tool::new(
            "delegate",
            "Delegate a task to a subagent that runs independently with its own context.\n\n\
             Modes:\n\
             1. Ad-hoc: Provide `instructions` for a custom task\n\
             2. Source-based: Provide `source` name to run a subrecipe, recipe, or agent\n\
             3. Combined: Pair a source with a task (e.g., source: \"deploy\", instructions: \"deploy to staging\")\n\n\
             Effective Delegation:\n\
             - Delegates know only instructions + source content\n\
             - Delegates cannot coordinate. Same-file work = conflicts.\n\
             - Parallel: async: true, then load(taskId) to wait and get results. Single: sync.\n\n\
             Research (read-only): parallelize freely - delegates explore and report back.\n\
             Work (writes): partition files strictly - no two delegates touch the same file.\n\n\
             Decompose → async delegates → load(taskId) for each → synthesize."
                .to_string(),
            schema.as_object().unwrap().clone(),
        )
    }

    async fn get_working_dir(&self, session_id: &str) -> PathBuf {
        self.context
            .session_manager
            .get_session(session_id, false)
            .await
            .ok()
            .map(|s| s.working_dir)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default())
    }

    async fn get_sources(&self, session_id: &str, working_dir: &Path) -> Vec<SourceEntry> {
        let fs_sources = self.get_filesystem_sources(working_dir).await;

        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut sources: Vec<SourceEntry> = Vec::new();

        self.add_subrecipes(session_id, &mut sources, &mut seen)
            .await;

        for source in fs_sources {
            if !seen.contains(&source.name) {
                seen.insert(source.name.clone());
                sources.push(source);
            }
        }

        sources.sort_by(|a, b| (&a.source_type, &a.name).cmp(&(&b.source_type, &b.name)));
        sources
    }

    async fn get_filesystem_sources(&self, working_dir: &Path) -> Vec<SourceEntry> {
        let mut cache = self.source_cache.lock().await;
        if let Some((cached_at, cached_dir, sources)) = cache.as_ref()
            && cached_dir == working_dir
            && cached_at.elapsed() < Duration::from_secs(60)
        {
            return sources.clone();
        }
        let sources = self.discover_filesystem_sources(working_dir);
        *cache = Some((Instant::now(), working_dir.to_path_buf(), sources.clone()));
        sources
    }

    async fn resolve_source(
        &self,
        session_id: &str,
        name: &str,
        working_dir: &Path,
    ) -> Result<Option<SourceEntry>, String> {
        let sources = self.get_sources(session_id, working_dir).await;

        Ok(sources.iter().find(|s| s.name == name).cloned())
    }

    async fn load_subrecipe_content(&self, session_id: &str, name: &str) -> Result<String, String> {
        let session = match self
            .context
            .session_manager
            .get_session(session_id, false)
            .await
        {
            Ok(s) => s,
            Err(_) => return Ok(String::new()),
        };

        let sub_recipes = match session.recipe.as_ref().and_then(|r| r.sub_recipes.as_ref()) {
            Some(sr) => sr,
            None => return Ok(String::new()),
        };

        let sr = match sub_recipes.iter().find(|sr| sr.name == name) {
            Some(sr) => sr,
            None => return Ok(String::new()),
        };

        match load_local_recipe_file(&sr.path) {
            Ok(recipe_file) => Self::format_subrecipe_content(name, &recipe_file.content),
            Err(_) => Ok(String::new()),
        }
    }

    fn format_subrecipe_content(name: &str, raw_content: &str) -> Result<String, String> {
        let recipe = Recipe::from_content(raw_content)
            .map_err(|_| format!("Subrecipe '{}' is not a valid recipe", name))?;
        let mut content = recipe.instructions.unwrap_or_default();
        if let Some(params) = &recipe.parameters
            && !params.is_empty()
        {
            content.push_str("\n\n");
            content.push_str(&Self::format_parameters(params));
        }
        Ok(content)
    }

    fn discover_filesystem_sources(&self, working_dir: &Path) -> Vec<SourceEntry> {
        discover_filesystem_sources(working_dir)
    }

    async fn add_subrecipes(
        &self,
        session_id: &str,
        sources: &mut Vec<SourceEntry>,
        seen: &mut std::collections::HashSet<String>,
    ) {
        let session = match self
            .context
            .session_manager
            .get_session(session_id, false)
            .await
        {
            Ok(s) => s,
            Err(_) => return,
        };

        let sub_recipes = match session.recipe.as_ref().and_then(|r| r.sub_recipes.as_ref()) {
            Some(sr) => sr,
            None => return,
        };

        for sr in sub_recipes {
            if seen.contains(&sr.name) {
                continue;
            }
            seen.insert(sr.name.clone());

            let description = self.build_subrecipe_description(sr).await;

            sources.push(SourceEntry {
                source_type: SourceType::Subrecipe,
                name: sr.name.clone(),
                description,
                content: String::new(),
                path: sr.path.clone(),
                global: false,
                writable: true,
                supporting_files: Vec::new(),
                properties: std::collections::HashMap::new(),
            });
        }
    }

    async fn build_subrecipe_description(&self, sr: &crate::recipe::SubRecipe) -> String {
        if let Some(desc) = &sr.description {
            return desc.clone();
        }

        if let Ok(recipe_file) = load_local_recipe_file(&sr.path)
            && let Ok(recipe) = Recipe::from_content(&recipe_file.content)
        {
            let mut desc = recipe.description.clone();

            if let Some(params) = &recipe.parameters
                && !params.is_empty()
            {
                desc = format!("{}\n{}", desc, Self::format_parameters(params));
            }

            return desc;
        }

        format!("Subrecipe from {}", sr.path)
    }

    fn format_parameters(params: &[RecipeParameter]) -> String {
        let mut out = String::from("Parameters:");
        for p in params {
            let mut detail = format!("\n  - {} ({}, {})", p.key, p.input_type, p.requirement);
            if let Some(default) = &p.default {
                detail.push_str(&format!(", default: \"{}\"", default));
            }
            if let Some(options) = &p.options
                && !options.is_empty()
            {
                detail.push_str(&format!(", options: [{}]", options.join(", ")));
            }
            detail.push_str(&format!(": {}", p.description));
            out.push_str(&detail);
        }
        out
    }

    async fn handle_load(
        &self,
        session_id: &str,
        arguments: Option<JsonObject>,
        notification_emitter: Option<ToolCallNotificationEmitter>,
    ) -> Result<CallToolResult, String> {
        self.cleanup_completed_tasks().await;

        let source_name = arguments
            .as_ref()
            .and_then(|args| args.get("source"))
            .and_then(|v| v.as_str());

        let cancel = arguments
            .as_ref()
            .and_then(|args| args.get("cancel"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let peek = arguments
            .as_ref()
            .and_then(|args| args.get("peek"))
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let working_dir = self.get_working_dir(session_id).await;

        if source_name.is_none() {
            return self
                .handle_load_discovery(session_id, &working_dir)
                .await
                .map(CallToolResult::success);
        }

        let name = source_name.unwrap();

        if is_session_id(name) {
            let task_result = self
                .handle_load_task_result(name, cancel, peek, notification_emitter)
                .await?;
            let mut meta = MetaObject::new();
            meta.0.insert(
                "subagent_session_id".to_string(),
                serde_json::Value::String(name.to_string()),
            );
            meta.0.insert(
                "task_status".to_string(),
                serde_json::Value::String(task_result.status.to_string()),
            );
            if let Some(turns) = task_result.turns {
                meta.0.insert(
                    "turns_taken".to_string(),
                    serde_json::Value::Number(turns.into()),
                );
            }
            if let Some(secs) = task_result.duration_secs {
                meta.0.insert(
                    "duration_secs".to_string(),
                    serde_json::Value::Number(secs.into()),
                );
            }
            return Ok(CallToolResult::success(task_result.content).with_meta(Some(meta)));
        }

        self.handle_load_source(session_id, name, &working_dir)
            .await
            .map(CallToolResult::success)
    }

    async fn handle_load_task_result(
        &self,
        task_id: &str,
        cancel: bool,
        peek: bool,
        notification_emitter: Option<ToolCallNotificationEmitter>,
    ) -> Result<TaskLoadResult, String> {
        let mut completed = self.completed_tasks.lock().await;

        let completed_entry = completed.get(task_id).map(|task| {
            (
                task.result.clone(),
                task.description.clone(),
                task.duration,
                task.turns_taken,
                Arc::clone(&task.notification_sink),
            )
        });

        if let Some((result, description, duration, turns_taken, notification_sink)) =
            completed_entry
        {
            if !peek {
                Self::attach_notification_emitter(&notification_sink, notification_emitter).await;
                completed.remove(task_id);
            }
            let status_key = match &result {
                Ok(_) => "completed",
                Err(e) if e.starts_with("Task panicked:") => "panicked",
                Err(_) => "failed",
            };
            let status = match status_key {
                "completed" => "✓ Completed",
                "panicked" => "✗ Panicked",
                _ => "✗ Failed",
            };
            let output = match result {
                Ok(output) => output,
                Err(error) => format!("Error: {}", error),
            };
            return Ok(TaskLoadResult {
                content: vec![ContentBlock::text(format!(
                    "# Background Task Result: {}\n\n\
                     **Task:** {}\n\
                     **Status:** {}\n\
                     **Duration:** {} ({} turns)\n\n\
                     ## Output\n\n{}",
                    task_id,
                    description,
                    status,
                    round_duration(duration),
                    turns_taken,
                    output
                ))],
                status: status_key,
                turns: Some(turns_taken),
                duration_secs: Some(duration.as_secs()),
            });
        }

        let mut running = self.background_tasks.lock().await;
        drop(completed);
        if running.contains_key(task_id) {
            if peek {
                let task = running.get(task_id).unwrap();
                let elapsed = task.started_at.elapsed();
                let turns = Arc::clone(&task.turns);
                let last_activity = Arc::clone(&task.last_activity);
                let description = task.description.clone();
                let notification_sink = Arc::clone(&task.notification_sink);

                drop(running);

                let turns_taken = self.refresh_task_turns(task_id, &turns).await;
                let now = current_epoch_millis();
                let last_activity_at = last_activity.load(Ordering::Relaxed);
                let idle_ms = if last_activity_at == 0 {
                    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
                } else {
                    now.saturating_sub(last_activity_at)
                };
                let buffered_count = notification_sink.lock().await.buffered_len();

                let mut output = format!(
                    "# Background Task Status: {}\n\n**Task:** {}\n**Status:** ⏳ Running\n**Elapsed:** {}\n**Turns taken:** {}\n**Idle:** {}\n**Buffered tool calls:** {}",
                    task_id,
                    description,
                    round_duration(elapsed),
                    turns_taken,
                    round_duration(Duration::from_millis(idle_ms)),
                    buffered_count,
                );

                if buffered_count == 0 && last_activity_at == 0 {
                    output.push_str("\n\n_Task is initialising (no tool activity yet)._");
                }

                return Ok(TaskLoadResult {
                    content: vec![ContentBlock::text(output)],
                    status: "running",
                    turns: Some(turns_taken),
                    duration_secs: Some(elapsed.as_secs()),
                });
            }

            if cancel {
                let notification_sink =
                    Arc::clone(&running.get(task_id).unwrap().notification_sink);
                Self::attach_notification_emitter(&notification_sink, notification_emitter).await;
                let task = running.remove(task_id).unwrap();
                drop(running);
                task.cancellation_token.cancel();

                let mut handle = task.handle;
                let output = tokio::select! {
                    result = &mut handle => {
                        match result {
                            Ok(Ok(s)) => s,
                            Ok(Err(e)) => format!("Error: {}", e),
                            Err(e) => format!("Task panicked: {}", e),
                        }
                    }
                    _ = tokio::time::sleep(Duration::from_secs(5)) => {
                        handle.abort();
                        "Task did not stop in time (aborted)".to_string()
                    }
                };
                let duration = task.started_at.elapsed();
                let turns_taken = self.refresh_task_turns(task_id, &task.turns).await;

                return Ok(TaskLoadResult {
                    content: vec![ContentBlock::text(format!(
                        "# Background Task Result: {}\n\n\
                         **Task:** {}\n\
                         **Status:** ⊘ Cancelled\n\
                         **Duration:** {} ({} turns)\n\n\
                         ## Output\n\n{}",
                        task_id,
                        task.description,
                        round_duration(duration),
                        turns_taken,
                        output
                    ))],
                    status: "cancelled",
                    turns: Some(turns_taken),
                    duration_secs: Some(duration.as_secs()),
                });
            }

            // Wait for the running task to complete, keeping the tool call
            // alive so notifications (subagent tool calls) stream in real time.
            let task = running.get(task_id).unwrap();
            let notification_sink = Arc::clone(&task.notification_sink);
            let completion_token = task.completion_token.clone();
            drop(running);
            Self::attach_notification_emitter(&notification_sink, notification_emitter).await;

            tokio::select! {
                _ = self.wait_for_background_task_completion(task_id, &completion_token) => {
                    self.cleanup_completed_tasks().await;
                    return Box::pin(
                        self.handle_load_task_result(task_id, false, false, None)
                    )
                    .await;
                }
                _ = tokio::time::sleep(Duration::from_secs(300)) => {
                    notification_sink.lock().await.detach();

                    return Err(format!(
                        "Task '{task_id}' is still running after waiting 5 min. \
                         Use load(source: \"{task_id}\") to wait again, or \
                         load(source: \"{task_id}\", cancel: true) to stop."
                    ));
                }
            }
        }

        Err(format!("Task '{}' not found.", task_id))
    }

    async fn handle_load_discovery(
        &self,
        session_id: &str,
        working_dir: &Path,
    ) -> Result<Vec<ContentBlock>, String> {
        {
            let mut cache = self.source_cache.lock().await;
            *cache = None;
        }

        let sources = self.get_sources(session_id, working_dir).await;
        let completed = self.completed_tasks.lock().await;

        if sources.is_empty() && completed.is_empty() {
            return Ok(vec![ContentBlock::text(
                "No sources available for load/delegate.\n\n\
                 Sources are discovered from:\n\
                 • Current recipe's sub_recipes\n\
                 • .agents/recipes/, .agents/agents/ (project-level)\n\
                 • ~/.agents/agents/ (global)\n\
                 • BCAIP_RECIPE_PATH directories",
            )]);
        }

        let mut output = String::from("Available sources for load/delegate:\n");

        if !completed.is_empty() {
            output.push_str("\nCompleted Tasks (awaiting retrieval):\n");
            let mut sorted_completed: Vec<_> = completed.values().collect();
            sorted_completed.sort_by_key(|t| &t.id);
            for task in sorted_completed {
                let status = if task.result.is_ok() {
                    "completed"
                } else {
                    "failed"
                };
                output.push_str(&format!(
                    "• {} - \"{}\" ({})\n",
                    task.id, task.description, status
                ));
            }
        }

        for kind in [SourceType::Subrecipe, SourceType::Recipe, SourceType::Agent] {
            let kind_sources: Vec<_> = sources.iter().filter(|s| s.source_type == kind).collect();
            if !kind_sources.is_empty() {
                output.push_str(&format!("\n{}:\n", kind_plural(kind)));
                for source in kind_sources {
                    output.push_str(&format!(
                        "• {} - {}\n",
                        source.name,
                        safe_truncate(&source.description, SUBAGENT_DESCRIPTION_BUDGET)
                    ));
                }
            }
        }

        output.push_str("\nUse load(source: \"name\") to load into context.\n");
        output.push_str("Use delegate(source: \"name\") to run as subagent.");

        Ok(vec![ContentBlock::text(output)])
    }

    async fn handle_load_source(
        &self,
        session_id: &str,
        name: &str,
        working_dir: &Path,
    ) -> Result<Vec<ContentBlock>, String> {
        let source = self.resolve_source(session_id, name, working_dir).await?;

        match source {
            Some(mut source) => {
                if source.source_type == SourceType::Subrecipe && source.content.is_empty() {
                    source.content = self
                        .load_subrecipe_content(session_id, &source.name)
                        .await?;
                }
                let content = source.to_load_text();

                let output = format!(
                    "# Loaded: {} ({})\n\n{}\n\n---\nThis knowledge is now available in your context.",
                    source.name, source.source_type, content
                );

                Ok(vec![ContentBlock::text(output)])
            }
            None => {
                let sources = self.get_sources(session_id, working_dir).await;

                let suggestions: Vec<&str> = sources
                    .iter()
                    .filter(|s| {
                        s.name.to_lowercase().contains(&name.to_lowercase())
                            || name.to_lowercase().contains(&s.name.to_lowercase())
                    })
                    .take(3)
                    .map(|s| s.name.as_str())
                    .collect();

                let error_msg = if suggestions.is_empty() {
                    format!(
                        "Source '{}' not found. Use load() to see available sources.",
                        name
                    )
                } else {
                    format!(
                        "Source '{}' not found. Did you mean: {}?",
                        name,
                        suggestions.join(", ")
                    )
                };

                Err(error_msg)
            }
        }
    }

    async fn handle_delegate(
        &self,
        session_id: &str,
        arguments: Option<JsonObject>,
        cancellation_token: CancellationToken,
        notification_emitter: Option<ToolCallNotificationEmitter>,
    ) -> Result<CallToolResult, String> {
        self.cleanup_completed_tasks().await;

        let params: DelegateParams = arguments
            .map(|args| serde_json::from_value(serde_json::Value::Object(args)))
            .transpose()
            .map_err(|e| format!("Invalid parameters: {}", e))?
            .unwrap_or_default();

        self.validate_delegate_params(&params)?;

        let session = self
            .context
            .session_manager
            .get_session(session_id, false)
            .await
            .map_err(|e| format!("Failed to get session: {}", e))?;

        if session.session_type == SessionType::SubAgent {
            return Err("Delegated tasks cannot spawn further delegations".to_string());
        }

        if params.r#async {
            let (content, task_id) = self.handle_async_delegate(session_id, params).await?;
            let mut meta = MetaObject::new();
            meta.0.insert(
                "subagent_session_id".to_string(),
                serde_json::Value::String(task_id),
            );
            return Ok(CallToolResult::success(content).with_meta(Some(meta)));
        }

        let working_dir = session.working_dir.clone();
        let recipe = self
            .build_delegate_recipe(&params, session_id, &working_dir)
            .await?;

        let task_config = self
            .build_task_config(&params, &recipe, &session)
            .await
            .map_err(|e| format!("Failed to build task config: {}", e))?;

        // Subagents must use Auto until get_agent_messages forwards
        // ActionRequired messages to the parent. Until then, any mode
        // that requires approval will hang on the subagent's confirmation_rx.
        let mut agent_config = AgentConfig::new(
            self.context.session_manager.clone(),
            crate::config::permission::PermissionManager::instance(),
            None,
            BcaipMode::Auto,
            true, // disable session naming for subagents
            crate::agents::BcaipPlatform::BcaipCli,
        )
        .with_use_login_shell_path(self.context.use_login_shell_path);
        agent_config.is_subagent = true;

        let subagent_session = self
            .create_subagent_session(&task_config, "Delegated task".to_string())
            .await?;

        let subagent_session_id = subagent_session.id.clone();

        let params = SubagentRunParams {
            config: agent_config,
            recipe,
            task_config,
            return_last_only: true,
            session_id: subagent_session.id,
            cancellation_token: Some(cancellation_token),
            on_message: None,
            notification_tx: None,
        };
        let result = Self::run_subagent_with_notifications(
            Self::notification_sink(notification_emitter),
            move |notification_tx| {
                let mut params = params;
                params.notification_tx = Some(notification_tx);
                run_subagent_task(params)
            },
        )
        .await;

        let mut meta = MetaObject::new();
        meta.0.insert(
            "subagent_session_id".to_string(),
            serde_json::Value::String(subagent_session_id),
        );

        match result {
            Ok(text) => {
                Ok(CallToolResult::success(vec![ContentBlock::text(text)]).with_meta(Some(meta)))
            }
            Err(e) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "Delegation failed: {}",
                e
            ))])
            .with_meta(Some(meta))),
        }
    }

    fn validate_delegate_params(&self, params: &DelegateParams) -> Result<(), String> {
        if params.instructions.is_none() && params.source.is_none() {
            return Err("Must provide 'instructions' or 'source' (or both)".to_string());
        }

        if params.parameters.is_some() && params.source.is_none() {
            return Err("'parameters' can only be used with 'source'".to_string());
        }

        if let Some(max) = params.max_turns
            && max < 1
        {
            return Err("'max_turns' must be at least 1".to_string());
        }

        Ok(())
    }

    async fn build_delegate_recipe(
        &self,
        params: &DelegateParams,
        session_id: &str,
        working_dir: &Path,
    ) -> Result<Recipe, String> {
        let mut recipe = if let Some(source_name) = &params.source {
            self.build_source_recipe(source_name, params, session_id, working_dir)
                .await?
        } else {
            self.build_adhoc_recipe(params)?
        };

        if let Some(ref context) = params.context {
            let existing = recipe.instructions.unwrap_or_default();
            recipe.instructions = Some(build_instructions_with_context(context, &existing));
        }

        Ok(recipe)
    }

    fn build_adhoc_recipe(&self, params: &DelegateParams) -> Result<Recipe, String> {
        let task = params
            .instructions
            .as_ref()
            .ok_or("Instructions required for ad-hoc task")?;

        Recipe::builder()
            .version("1.0.0")
            .title("Delegated Task")
            .description("Ad-hoc delegated task")
            .prompt(task)
            .build()
            .map_err(|e| format!("Failed to build recipe: {}", e))
    }

    async fn build_source_recipe(
        &self,
        source_name: &str,
        params: &DelegateParams,
        session_id: &str,
        working_dir: &Path,
    ) -> Result<Recipe, String> {
        let source = self
            .resolve_source(session_id, source_name, working_dir)
            .await?
            .ok_or_else(|| format!("Source '{}' not found", source_name))?;

        let mut recipe = match source.source_type {
            SourceType::Recipe | SourceType::Subrecipe => {
                self.build_recipe_from_source(&source, params, session_id)
                    .await?
            }
            SourceType::Agent => self.build_recipe_from_agent(&source, params)?,
            _ => {
                return Err(format!(
                    "Source '{}' has kind '{}' which cannot be delegated from summon",
                    source_name, source.source_type
                ));
            }
        };

        if let Some(extra_instructions) = &params.instructions {
            if recipe.prompt.is_some() {
                let current_prompt = recipe.prompt.take().unwrap();
                recipe.prompt = Some(format!("{}\n\n{}", current_prompt, extra_instructions));
            } else {
                recipe.prompt = Some(extra_instructions.clone());
            }
        }

        Ok(recipe)
    }

    async fn build_recipe_from_source(
        &self,
        source: &SourceEntry,
        params: &DelegateParams,
        session_id: &str,
    ) -> Result<Recipe, String> {
        let session = self
            .context
            .session_manager
            .get_session(session_id, false)
            .await
            .map_err(|e| format!("Failed to get session: {}", e))?;

        if source.source_type == SourceType::Subrecipe {
            let sub_recipes = session.recipe.as_ref().and_then(|r| r.sub_recipes.as_ref());

            if let Some(sub_recipes) = sub_recipes
                && let Some(sr) = sub_recipes.iter().find(|sr| sr.name == source.name)
            {
                let recipe_file = load_local_recipe_file(&sr.path)
                    .map_err(|e| format!("Failed to load subrecipe '{}': {}", source.name, e))?;

                let merged =
                    merge_subrecipe_parameters(sr.values.as_ref(), params.parameters.as_ref());
                let param_values: Vec<(String, String)> = merged.into_iter().collect();

                return build_recipe_from_template(
                    recipe_file.content,
                    &recipe_file.parent_dir,
                    param_values,
                    None::<fn(&str, &str) -> Result<String, anyhow::Error>>,
                )
                .map_err(|e| format!("Failed to build subrecipe: {}", e));
            }
        }

        let recipe_file = load_local_recipe_file(&source.path)
            .map_err(|e| format!("Failed to load recipe '{}': {}", source.name, e))?;

        let param_values: Vec<(String, String)> = params
            .parameters
            .as_ref()
            .map(|p| {
                p.iter()
                    .map(|(k, v)| {
                        let value_str = match v {
                            serde_json::Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        (k.clone(), value_str)
                    })
                    .collect()
            })
            .unwrap_or_default();

        build_recipe_from_template(
            recipe_file.content,
            &recipe_file.parent_dir,
            param_values,
            None::<fn(&str, &str) -> Result<String, anyhow::Error>>,
        )
        .map_err(|e| format!("Failed to build recipe: {}", e))
    }

    fn build_recipe_from_agent(
        &self,
        source: &SourceEntry,
        params: &DelegateParams,
    ) -> Result<Recipe, String> {
        if source.path.is_empty() {
            return Err("Agent source has no path".to_string());
        }

        let model = source
            .properties
            .get("model")
            .and_then(serde_json::Value::as_str)
            .map(str::to_string);

        // max_turns is set later in build_task_config so it can incorporate params.max_turns
        // with the correct priority ordering; setting it here would cause it to be overridden
        // by the parent session's recipe instead.
        let settings = model.map(|m| Settings {
            bcaip_model: Some(m),
            bcaip_provider: params.provider.clone(),
            temperature: params.temperature,
            max_turns: None,
        });

        let mut builder = Recipe::builder()
            .version("1.0.0")
            .title(format!("Agent: {}", source.name))
            .description(source.description.clone())
            .instructions(&source.content);

        if let Some(settings) = settings {
            builder = builder.settings(settings);
        }

        if params.instructions.is_none() {
            builder = builder.prompt("Proceed with your expertise to produce a useful result.");
        }

        builder
            .build()
            .map_err(|e| format!("Failed to build recipe from agent: {}", e))
    }

    async fn build_task_config(
        &self,
        params: &DelegateParams,
        recipe: &Recipe,
        session: &crate::session::Session,
    ) -> Result<TaskConfig, anyhow::Error> {
        let mut extensions = EnabledExtensionsState::extensions_or_default(
            Some(&session.extension_data),
            Config::global(),
        );

        if let Some(filter) = &params.extensions {
            if filter.is_empty() {
                extensions = Vec::new();
            } else {
                let available_names: Vec<String> =
                    extensions.iter().map(|ext| ext.name()).collect();
                extensions.retain(|ext| filter.contains(&ext.name()));
                let unmatched: Vec<&str> = filter
                    .iter()
                    .filter(|name| !available_names.iter().any(|n| n == *name))
                    .map(String::as_str)
                    .collect();
                if !unmatched.is_empty() {
                    warn!(
                        "Delegate requested extensions not available in session: {:?}. Available: {:?}",
                        unmatched, available_names
                    );
                }
            }
        }

        let (provider, model_config) = self
            .resolve_provider(params, recipe, session, &extensions)
            .await?;

        let max_turns = params
            .max_turns
            .or_else(|| recipe.settings.as_ref().and_then(|s| s.max_turns))
            .unwrap_or_else(|| self.resolve_max_turns(session));

        if max_turns == 0 || max_turns > u32::MAX as usize {
            anyhow::bail!(
                "max_turns must be between 1 and {} (got {})",
                u32::MAX,
                max_turns
            );
        }

        let effective_working_dir = match &params.working_dir {
            Some(dir) => resolve_working_dir(&session.working_dir, dir)?,
            None => session.working_dir.clone(),
        };

        let task_config = TaskConfig::new(
            provider,
            model_config,
            &session.id,
            &effective_working_dir,
            extensions,
        )
        .with_max_turns(Some(max_turns));

        Ok(task_config)
    }

    fn resolve_model_config(
        &self,
        params: &DelegateParams,
        recipe: &Recipe,
        session: &crate::session::Session,
        provider_name: &str,
        provider_default_model: Option<&str>,
    ) -> Result<bcaip_provider_types::model::ModelConfig, anyhow::Error> {
        let env_model = std::env::var("BCAIP_SUBAGENT_MODEL").ok();
        let env_provider = std::env::var("BCAIP_SUBAGENT_PROVIDER").ok();
        let recipe_settings = recipe.settings.as_ref();
        let configured = Config::global().all_values().ok();
        let configured_provider = configured
            .as_ref()
            .and_then(|values| values.get("BCAIP_SUBAGENT_PROVIDER"))
            .and_then(serde_json::Value::as_str);
        let configured_model = configured
            .as_ref()
            .and_then(|values| values.get("BCAIP_SUBAGENT_MODEL"))
            .and_then(serde_json::Value::as_str);
        let matches_provider =
            |candidate: Option<&str>| candidate.is_none() || candidate == Some(provider_name);
        let model = recipe_settings
            .and_then(|settings| settings.bcaip_model.clone())
            .filter(|_| {
                matches_provider(
                    recipe_settings.and_then(|settings| settings.bcaip_provider.as_deref()),
                )
            })
            .or_else(|| {
                env_model
                    .clone()
                    .filter(|_| matches_provider(env_provider.as_deref()))
            })
            .or_else(|| {
                params
                    .model
                    .clone()
                    .filter(|_| matches_provider(params.provider.as_deref()))
            })
            .or_else(|| {
                configured_model
                    .filter(|_| matches_provider(configured_provider))
                    .map(str::to_string)
            })
            .or_else(|| {
                session
                    .model_config
                    .as_ref()
                    .filter(|_| matches_provider(session.provider_name.as_deref()))
                    .map(|config| config.model_name.clone())
            })
            .or_else(|| {
                provider_default_model
                    .filter(|model| !model.is_empty())
                    .map(str::to_string)
            })
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "No model configured for provider '{}'; set BCAIP_SUBAGENT_MODEL",
                    provider_name
                )
            })?;

        let parent = session.model_config.as_ref();
        let mut model_config = if parent.is_some_and(|config| {
            matches_provider(session.provider_name.as_deref()) && config.model_name == model
        }) {
            parent.unwrap().clone()
        } else {
            let mut cfg = crate::model_config::model_config_from_user_config_with_session_settings(
                provider_name,
                &model,
                parent,
                None,
                None,
            )?;
            if let Some(parent) = parent {
                cfg.toolshim = parent.toolshim;
                cfg.toolshim_model = parent.toolshim_model.clone();
                cfg.temperature = cfg.temperature.or(parent.temperature);
            }
            cfg
        };

        if let Some(temp) = params.temperature {
            model_config = model_config.with_temperature(Some(temp));
        } else if let Some(temp) = recipe.settings.as_ref().and_then(|s| s.temperature) {
            model_config = model_config.with_temperature(Some(temp));
        }

        Ok(model_config)
    }

    async fn resolve_provider(
        &self,
        params: &DelegateParams,
        recipe: &Recipe,
        session: &crate::session::Session,
        extensions: &[crate::config::ExtensionConfig],
    ) -> Result<
        (
            Arc<dyn bcaip_provider_types::base::Provider>,
            bcaip_provider_types::model::ModelConfig,
        ),
        anyhow::Error,
    > {
        let env_provider = std::env::var("BCAIP_SUBAGENT_PROVIDER").ok();
        let provider_name = recipe
            .settings
            .as_ref()
            .and_then(|s| s.bcaip_provider.clone())
            .or_else(|| env_provider.clone())
            .or_else(|| params.provider.clone())
            .or_else(|| {
                Config::global()
                    .get_param::<String>("BCAIP_SUBAGENT_PROVIDER")
                    .ok()
            })
            .or_else(|| session.provider_name.clone())
            .ok_or_else(|| anyhow::anyhow!("No provider configured"))?;

        let provider_entry = providers::get_from_registry(&provider_name).await;
        let provider_default_model = provider_entry
            .as_ref()
            .ok()
            .map(|entry| entry.metadata().default_model.as_str());
        let model_config = self.resolve_model_config(
            params,
            recipe,
            session,
            &provider_name,
            provider_default_model,
        )?;
        let provider = match provider_entry {
            Ok(entry) => entry.create(extensions.to_vec()).await?,
            Err(error) => {
                let parent_provider = if let Some(extension_manager) = self
                    .context
                    .extension_manager
                    .as_ref()
                    .and_then(|weak| weak.upgrade())
                {
                    extension_manager.get_provider().lock().await.clone()
                } else {
                    None
                };

                match parent_provider {
                    Some(provider)
                        if provider.get_name() == provider_name
                            && !provider.manages_own_context() =>
                    {
                        provider
                    }
                    _ => return Err(error),
                }
            }
        };
        Ok((provider, model_config))
    }

    fn resolve_max_turns(&self, session: &crate::session::Session) -> usize {
        session
            .recipe
            .as_ref()
            .and_then(|r| r.settings.as_ref())
            .and_then(|s| s.max_turns)
            .or_else(|| {
                std::env::var("BCAIP_SUBAGENT_MAX_TURNS")
                    .ok()
                    .and_then(|v| v.parse().ok())
            })
            .or_else(|| {
                Config::global()
                    .get_param::<usize>("BCAIP_SUBAGENT_MAX_TURNS")
                    .ok()
            })
            .unwrap_or(DEFAULT_SUBAGENT_MAX_TURNS)
    }

    /// Count durable, user-visible assistant blocks in the active task turn,
    /// excluding assistant-only compaction scaffolding.
    async fn refresh_task_turns(&self, task_id: &str, cached_turns: &AtomicU32) -> u32 {
        match self
            .context
            .session_manager
            .get_session(task_id, true)
            .await
        {
            Ok(session) => {
                let turns = session
                    .conversation
                    .as_ref()
                    .map(durable_assistant_turn_count)
                    .unwrap_or_default();
                cached_turns.store(turns, Ordering::Relaxed);
                turns
            }
            Err(error) => {
                warn!(
                    "Failed to refresh turn count for background task {}: {}",
                    task_id, error
                );
                cached_turns.load(Ordering::Relaxed)
            }
        }
    }

    async fn refresh_running_task_turns(&self) -> HashMap<String, u32> {
        let tasks: Vec<_> = self
            .background_tasks
            .lock()
            .await
            .values()
            .map(|task| (task.id.clone(), Arc::clone(&task.turns)))
            .collect();
        let mut refreshed = HashMap::with_capacity(tasks.len());
        for (id, turns) in tasks {
            let count = self.refresh_task_turns(&id, &turns).await;
            refreshed.insert(id, count);
        }
        refreshed
    }

    async fn wait_for_background_task_completion(
        &self,
        task_id: &str,
        completion_token: &CancellationToken,
    ) {
        completion_token.cancelled().await;
        loop {
            let finished_or_moved = self
                .background_tasks
                .lock()
                .await
                .get(task_id)
                .map(|task| task.handle.is_finished())
                .unwrap_or(true);
            if finished_or_moved {
                return;
            }
            tokio::task::yield_now().await;
        }
    }

    async fn cleanup_completed_tasks(&self) {
        let finished: Vec<(String, Arc<AtomicU32>)> = self
            .background_tasks
            .lock()
            .await
            .iter()
            .filter(|(_, task)| task.handle.is_finished())
            .map(|(id, task)| (id.clone(), Arc::clone(&task.turns)))
            .collect();

        let mut refreshed = HashMap::with_capacity(finished.len());
        for (id, turns) in &finished {
            let count = self.refresh_task_turns(id, turns).await;
            refreshed.insert(id.clone(), count);
        }

        // Keep the same lock order as task lookup so the running -> completed
        // transition is atomic from callers' perspective.
        let mut completed = self.completed_tasks.lock().await;
        let mut tasks = self.background_tasks.lock().await;
        for (id, _) in finished {
            let Some(task) = tasks.remove(&id) else {
                continue;
            };
            let turns_taken = refreshed
                .remove(&id)
                .unwrap_or_else(|| task.turns.load(Ordering::Relaxed));
            let duration = task.started_at.elapsed();

            let result = match task.handle.await {
                Ok(Ok(output)) => {
                    info!("Background task {} completed successfully", id);
                    Ok(output)
                }
                Ok(Err(e)) => {
                    warn!("Background task {} failed: {}", id, e);
                    Err(e.to_string())
                }
                Err(e) => {
                    warn!("Background task {} panicked: {}", id, e);
                    Err(format!("Task panicked: {}", e))
                }
            };

            completed.insert(
                id.clone(),
                CompletedTask {
                    id,
                    description: task.description,
                    result,
                    turns_taken,
                    duration,
                    completed_at: Instant::now(),
                    notification_sink: task.notification_sink,
                },
            );
        }

        let ttl = completed_task_ttl();
        completed.retain(|_id, task| task.completed_at.elapsed() <= ttl);
    }

    fn get_task_description(params: &DelegateParams) -> String {
        match (&params.source, &params.instructions) {
            (Some(source), Some(instructions)) => format!("{}: {}", source, instructions),
            (Some(source), None) => source.clone(),
            (None, Some(instructions)) => instructions.clone(),
            (None, None) => "Unknown task".to_string(),
        }
    }

    async fn handle_async_delegate(
        &self,
        session_id: &str,
        params: DelegateParams,
    ) -> Result<(Vec<ContentBlock>, String), String> {
        let task_count = self.background_tasks.lock().await.len();
        let max_tasks = max_background_tasks();
        if task_count >= max_tasks {
            return Err(format!(
                "Maximum {} background tasks already running. Wait for completion or use sync mode.",
                max_tasks
            ));
        }

        let session = self
            .context
            .session_manager
            .get_session(session_id, false)
            .await
            .map_err(|e| format!("Failed to get session: {}", e))?;

        let working_dir = session.working_dir.clone();
        let recipe = self
            .build_delegate_recipe(&params, session_id, &working_dir)
            .await?;

        let task_config = self
            .build_task_config(&params, &recipe, &session)
            .await
            .map_err(|e| format!("Failed to build task config: {}", e))?;

        let description = safe_truncate(&Self::get_task_description(&params), TASK_LABEL_BUDGET);

        // Subagents must use Auto until get_agent_messages forwards
        // ActionRequired messages to the parent. Until then, any mode
        // that requires approval will hang on the subagent's confirmation_rx.
        let mut agent_config = AgentConfig::new(
            self.context.session_manager.clone(),
            crate::config::permission::PermissionManager::instance(),
            None,
            BcaipMode::Auto,
            true, // disable session naming for subagents
            crate::agents::BcaipPlatform::BcaipCli,
        )
        .with_use_login_shell_path(self.context.use_login_shell_path);
        agent_config.is_subagent = true;

        let subagent_session = self
            .create_subagent_session(&task_config, description.clone())
            .await?;

        let task_id = subagent_session.id.clone();

        let turns = Arc::new(AtomicU32::new(0));
        let last_activity = Arc::new(AtomicU64::new(0));

        let last_activity_clone = Arc::clone(&last_activity);

        let on_message: OnMessageCallback = Arc::new(move |_msg| {
            last_activity_clone.store(current_epoch_millis(), Ordering::Relaxed);
        });

        let task_token = CancellationToken::new();
        let task_token_clone = task_token.clone();

        let notification_sink = Self::notification_sink(None);
        let task_notification_sink = Arc::clone(&notification_sink);

        let (handle, completion_token) = spawn_background_task(async move {
            let params = SubagentRunParams {
                config: agent_config,
                recipe,
                task_config,
                return_last_only: true,
                session_id: subagent_session.id,
                cancellation_token: Some(task_token_clone),
                on_message: Some(on_message),
                notification_tx: None,
            };
            Self::run_subagent_with_notifications(task_notification_sink, move |notification_tx| {
                let mut params = params;
                params.notification_tx = Some(notification_tx);
                run_subagent_task(params)
            })
            .await
        });

        let task = BackgroundTask {
            id: task_id.clone(),
            description: description.clone(),
            started_at: Instant::now(),
            turns,
            last_activity,
            handle,
            cancellation_token: task_token,
            completion_token,
            notification_sink,
        };

        self.background_tasks
            .lock()
            .await
            .insert(task_id.clone(), task);

        let content = vec![ContentBlock::text(format!(
            "Task {} started in background: \"{}\"\n\
             Continue with other work. When you need the result, use load(source: \"{}\").",
            task_id, description, task_id
        ))];
        Ok((content, task_id))
    }
}

#[async_trait]
impl McpClientTrait for SummonClient {
    async fn list_tools(
        &self,
        session_id: &str,
        _next_cursor: Option<String>,
        _cancellation_token: CancellationToken,
    ) -> Result<ListToolsResult, Error> {
        self.cleanup_completed_tasks().await;

        let is_subagent = self
            .context
            .session_manager
            .get_session(session_id, false)
            .await
            .map(|s| s.session_type == SessionType::SubAgent)
            .unwrap_or(false);

        let mut tools = vec![self.create_load_tool()];

        if !is_subagent {
            tools.push(self.create_delegate_tool());
        }

        Ok(ListToolsResult {
            tools,
            next_cursor: None,
            meta: None,
            ..Default::default()
        })
    }

    async fn call_tool(
        &self,
        ctx: &ToolCallContext,
        name: &str,
        arguments: Option<JsonObject>,
        cancellation_token: CancellationToken,
    ) -> Result<CallToolResult, Error> {
        let session_id = &ctx.session_id;
        match name {
            "load" => match self
                .handle_load(session_id, arguments, ctx.notification_emitter().cloned())
                .await
            {
                Ok(result) => Ok(result),
                Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                    "Error: {}",
                    error
                ))])),
            },
            "delegate" => {
                match self
                    .handle_delegate(
                        session_id,
                        arguments,
                        cancellation_token,
                        ctx.notification_emitter().cloned(),
                    )
                    .await
                {
                    Ok(result) => Ok(result),
                    Err(error) => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                        "Error: {}",
                        error
                    ))])),
                }
            }
            _ => Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "Error: Unknown tool: {}",
                name
            ))])),
        }
    }

    fn get_info(&self) -> Option<&InitializeResult> {
        Some(&self.info)
    }

    fn get_instructions(&self) -> Option<String> {
        let instructions = build_subagent_instructions(self.context.session.as_deref());
        if instructions.is_empty() {
            None
        } else {
            Some(instructions)
        }
    }

    async fn get_moim(&self, _session_id: &str) -> Option<String> {
        self.cleanup_completed_tasks().await;
        let refreshed_turns = self.refresh_running_task_turns().await;

        let completed = self.completed_tasks.lock().await;
        let running = self.background_tasks.lock().await;

        if running.is_empty() && completed.is_empty() {
            return None;
        }

        let mut lines = vec!["Background tasks:".to_string()];
        let now = current_epoch_millis();

        let mut sorted_running: Vec<_> = running.values().collect();
        sorted_running.sort_by_key(|task| &task.id);

        for task in sorted_running {
            let elapsed = task.started_at.elapsed();
            let last_activity_at = task.last_activity.load(Ordering::Relaxed);
            let idle_ms = if last_activity_at == 0 {
                u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
            } else {
                now.saturating_sub(last_activity_at)
            };

            lines.push(format!(
                "• {}: \"{}\" - running {}, {} turns, idle {}",
                task.id,
                task.description,
                round_duration(elapsed),
                refreshed_turns
                    .get(&task.id)
                    .copied()
                    .unwrap_or_else(|| task.turns.load(Ordering::Relaxed)),
                round_duration(Duration::from_millis(idle_ms)),
            ));
        }

        let mut sorted_completed: Vec<_> = completed.values().collect();
        sorted_completed.sort_by_key(|task| &task.id);

        for task in sorted_completed {
            let status = if task.result.is_ok() {
                "completed"
            } else {
                "failed"
            };
            lines.push(format!(
                "• {}: \"{}\" - {} in {} ({} turns) - use load(\"{}\") to get result",
                task.id,
                task.description,
                status,
                round_duration(task.duration),
                task.turns_taken,
                task.id
            ));
        }

        if !running.is_empty() {
            lines.push(
                "\n→ Use load(source: \"<id>\") to wait for a task, or load(source: \"<id>\", cancel: true) to stop it"
                    .to_string(),
            );
        }

        Some(lines.join("\n"))
    }
}

/// Resolve a requested `working_dir` override against the parent session
/// directory. Relative paths are joined to the parent dir; the result must
/// canonicalize to an existing directory contained within the parent dir.
fn resolve_working_dir(parent_dir: &Path, requested: &str) -> Result<PathBuf, anyhow::Error> {
    let requested_path = PathBuf::from(requested);
    let resolved = if requested_path.is_absolute() {
        requested_path
    } else {
        parent_dir.join(&requested_path)
    };
    let canonical = resolved
        .canonicalize()
        .map_err(|e| anyhow::anyhow!("working_dir '{}' could not be resolved: {}", requested, e))?;
    let parent_canonical = parent_dir
        .canonicalize()
        .unwrap_or_else(|_| parent_dir.to_path_buf());
    if !canonical.starts_with(&parent_canonical) {
        anyhow::bail!(
            "working_dir '{}' is outside the parent session directory",
            requested
        );
    }
    if !canonical.is_dir() {
        anyhow::bail!("working_dir '{}' is not a directory", requested);
    }
    Ok(canonical)
}

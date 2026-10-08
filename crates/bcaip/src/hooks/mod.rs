//! Lifecycle hooks support, modelled after the Open Plugins
//! [hooks specification](https://open-plugins.com/agent-builders/components/hooks).
//!
//! Hooks live in `<plugin-root>/hooks/hooks.json` of any plugin discovered by
//! [`crate::plugins::discovery::discover_enabled_plugins`]. The schema is:
//!
//! ```json
//! {
//!   "hooks": {
//!     "PostToolUse": [
//!       {
//!         "matcher": "developer__shell|developer__text_editor",
//!         "hooks": [
//!           { "type": "command", "command": "${PLUGIN_ROOT}/scripts/log.sh" }
//!         ]
//!       }
//!     ]
//!   }
//! }
//! ```
//!
//! Bcaip currently supports `type: "command"` actions. Unknown event names and
//! action types are ignored per the spec. Hook scripts receive the JSON event
//! context on stdin and SHOULD exit 0 on success.

use crate::plugins::discovery::{DiscoveredPlugin, discover_enabled_plugins};
use anyhow::{Context, Result};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{process::Stdio, sync::OnceLock, time::Duration};
use tokio::{io::AsyncWriteExt, process::Command};
use tracing::{debug, info, warn};
use tracing_futures::Instrument;

/// Default per-hook timeout when the plugin does not specify one.
const DEFAULT_HOOK_TIMEOUT_SECS: u64 = 30;

const COMMAND_FAILED_REASON: &str = "the hook command failed to run";
const SERIALIZATION_FAILED_REASON: &str = "the hook payload could not be serialized";
const STDIN_DELIVERY_FAILED_REASON: &str = "the hook did not receive the request payload";

/// Lifecycle events a hook can subscribe to.
///
/// The variant names match the event names used in `hooks.json`. Unknown
/// events in user config are ignored at load time, per the spec.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    PreToolUse,
    PreToolUseResult,
    PostToolUse,
    PostToolUseFailure,
    SessionStart,
    SessionEnd,
    UserPromptSubmit,
    BeforeReadFile,
    AfterFileEdit,
    BeforeShellExecution,
    AfterShellExecution,
    Stop,
}

impl HookEvent {
    fn name(&self) -> &'static str {
        match self {
            HookEvent::PreToolUse => "PreToolUse",
            HookEvent::PreToolUseResult => "PreToolUseResult",
            HookEvent::PostToolUse => "PostToolUse",
            HookEvent::PostToolUseFailure => "PostToolUseFailure",
            HookEvent::SessionStart => "SessionStart",
            HookEvent::SessionEnd => "SessionEnd",
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::BeforeReadFile => "BeforeReadFile",
            HookEvent::AfterFileEdit => "AfterFileEdit",
            HookEvent::BeforeShellExecution => "BeforeShellExecution",
            HookEvent::AfterShellExecution => "AfterShellExecution",
            HookEvent::Stop => "Stop",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "PreToolUse" => HookEvent::PreToolUse,
            "PreToolUseResult" => HookEvent::PreToolUseResult,
            "PostToolUse" => HookEvent::PostToolUse,
            "PostToolUseFailure" => HookEvent::PostToolUseFailure,
            "SessionStart" => HookEvent::SessionStart,
            "SessionEnd" => HookEvent::SessionEnd,
            "UserPromptSubmit" => HookEvent::UserPromptSubmit,
            "BeforeReadFile" => HookEvent::BeforeReadFile,
            "AfterFileEdit" => HookEvent::AfterFileEdit,
            "BeforeShellExecution" => HookEvent::BeforeShellExecution,
            "AfterShellExecution" => HookEvent::AfterShellExecution,
            "Stop" => HookEvent::Stop,
            _ => return None,
        })
    }
}

impl std::fmt::Display for HookEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

#[derive(Debug, Default, Deserialize)]
struct HooksFile {
    #[serde(default)]
    hooks: HashMap<String, Value>,
}

#[derive(Debug, Deserialize)]
struct RawHookRule {
    #[serde(default)]
    matcher: Option<String>,
    #[serde(default)]
    hooks: Vec<Value>,
}

#[derive(Debug, Deserialize)]
struct RawCommandAction {
    command: String,
    #[serde(default)]
    timeout: Option<u64>,
    #[serde(default, deserialize_with = "deserialize_present_on_failure")]
    on_failure: Option<Value>,
}

#[derive(Debug, PartialEq, Eq)]
enum ActionSelection {
    Selected,
    Ignored,
}

fn select_action(action: &Value, plugin_name: &str, path: &Path) -> Result<ActionSelection> {
    let Some(obj) = action.as_object() else {
        anyhow::bail!("hook action in {} must be an object", path.display());
    };

    match obj.get("type") {
        None | Some(Value::Null) => {}
        Some(Value::String(action_type)) if action_type == "command" => {}
        Some(Value::String(action_type)) => {
            debug!(
                plugin = plugin_name,
                action_type = %action_type,
                "Ignoring unsupported hook action type",
            );
            return Ok(ActionSelection::Ignored);
        }
        Some(_) => anyhow::bail!("hook action `type` in {} must be a string", path.display()),
    }

    match obj.get("command") {
        None | Some(Value::Null) => {
            debug!(
                plugin = plugin_name,
                "Ignoring command hook action with no command",
            );
            Ok(ActionSelection::Ignored)
        }
        Some(Value::String(_)) => Ok(ActionSelection::Selected),
        Some(_) => anyhow::bail!(
            "hook action `command` in {} must be a string",
            path.display()
        ),
    }
}

fn deserialize_present_on_failure<'de, D>(d: D) -> Result<Option<serde_json::Value>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    serde_json::Value::deserialize(d).map(Some)
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum OnFailure {
    #[default]
    Allow,
    Block,
}

/// A loaded, plugin-bound hook rule ready to execute.
#[derive(Debug, Clone)]
struct LoadedRule {
    plugin_name: String,
    plugin_root: PathBuf,
    matcher: Option<Regex>,
    actions: Vec<LoadedAction>,
}

#[derive(Debug, Clone)]
enum LoadedAction {
    Command {
        command: String,
        timeout: Duration,
        on_failure: OnFailure,
    },
}

impl LoadedAction {
    fn on_failure(&self) -> OnFailure {
        let LoadedAction::Command { on_failure, .. } = self;
        *on_failure
    }
}

/// Context passed to a hook as JSON on stdin.
///
/// The `matcher_context` is the string the rule's `matcher` regex is tested
/// against — tool name for tool events, file path for file events, command
/// string for shell events. Other fields carry the same value plus the
/// raw JSON payload of the underlying event so scripts can do richer things
/// without needing to parse a hook-specific schema.
#[derive(Debug, Clone, Serialize)]
pub struct HookContext {
    pub event: String,
    pub session_id: String,
    pub matcher_context: Option<String>,
    /// Stable identifier for one tool call, the same value bcaip records as
    /// `gen_ai.tool.call.id`. Correlates the pre and post events of a single
    /// call, which tool name plus input cannot do when a call repeats.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_input: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_output: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_assistant_message: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// `PreToolUseResult` only: "allow" or "deny". There is no third value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    /// `PreToolUseResult` only: true when at least one matching `PreToolUse`
    /// hook ran to completion for this call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub policy_evaluated: Option<bool>,
    /// `PreToolUseResult` on deny only: the plugin that denied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocked_by: Option<String>,
    /// `PreToolUseResult` on deny only: the reason the plugin gave.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

impl HookContext {
    pub fn new(event: HookEvent, session_id: impl Into<String>) -> Self {
        Self {
            event: event.to_string(),
            session_id: session_id.into(),
            matcher_context: None,
            tool_call_id: None,
            tool_name: None,
            tool_input: None,
            tool_output: None,
            message: None,
            last_assistant_message: None,
            working_dir: None,
            decision: None,
            policy_evaluated: None,
            blocked_by: None,
            reason: None,
        }
    }

    pub fn with_tool(mut self, tool_name: impl Into<String>, tool_input: Option<Value>) -> Self {
        let name = tool_name.into();
        self.matcher_context = Some(name.clone());
        self.tool_name = Some(name);
        self.tool_input = tool_input;
        self
    }

    pub fn with_tool_output(mut self, output: Value) -> Self {
        self.tool_output = Some(output);
        self
    }

    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        let msg = message.into();
        self.matcher_context.get_or_insert_with(|| msg.clone());
        self.message = Some(msg);
        self
    }

    pub fn with_last_assistant_message(mut self, message: impl Into<String>) -> Self {
        let message = message.into();
        if !message.is_empty() {
            self.last_assistant_message = Some(message);
        }
        self
    }

    pub fn with_working_dir(mut self, dir: impl Into<String>) -> Self {
        self.working_dir = Some(dir.into());
        self
    }

    pub fn with_tool_call_id(mut self, tool_call_id: impl Into<String>) -> Self {
        self.tool_call_id = Some(tool_call_id.into());
        self
    }

    /// Populate the `PreToolUseResult` outcome fields. `blocked_by` and `reason`
    /// are set only on deny, so an allow payload omits them entirely.
    pub(crate) fn with_pre_tool_use_outcome(
        mut self,
        outcome: &HookChainOutcome,
    ) -> PreToolUseResultPayload {
        self.policy_evaluated = Some(outcome.policy_evaluated);
        match &outcome.decision {
            HookDecision::Allow => self.decision = Some("allow".to_string()),
            HookDecision::Deny { reason, plugin } => {
                self.decision = Some("deny".to_string());
                self.blocked_by = Some(plugin.clone());
                self.reason = Some(reason.clone());
            }
        }
        PreToolUseResultPayload {
            context: self,
            cause: outcome.cause.map(|cause| cause.as_str().to_string()),
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct PreToolUseResultPayload {
    #[serde(flatten)]
    context: HookContext,
    #[serde(skip_serializing_if = "Option::is_none")]
    cause: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HookDecision {
    Allow,
    Deny { reason: String, plugin: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HookOutcomeCause {
    PolicyDenial,
    HookFailure,
}

impl HookOutcomeCause {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            HookOutcomeCause::PolicyDenial => "policy_denial",
            HookOutcomeCause::HookFailure => "hook_failure",
        }
    }
}

/// Crate-internal: the public [`HookManager::emit_blocking`] contract is
/// unchanged and still returns a [`HookDecision`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookChainOutcome {
    pub decision: HookDecision,
    pub policy_evaluated: bool,
    pub cause: Option<HookOutcomeCause>,
}

impl HookChainOutcome {
    pub(crate) fn allow(policy_evaluated: bool) -> Self {
        Self {
            decision: HookDecision::Allow,
            policy_evaluated,
            cause: None,
        }
    }

    pub(crate) fn denial(&self) -> Option<HookDenial> {
        let HookDecision::Deny { reason, plugin } = &self.decision else {
            return None;
        };
        Some(match self.cause {
            Some(HookOutcomeCause::HookFailure) => HookDenial {
                message: format!(
                    "Tool call blocked because policy hook `{plugin}` could not complete: \
                     {reason}. That hook is configured to block on failure."
                ),
                error_type: "hook_failed",
            },
            _ => HookDenial {
                message: format!(
                    "Tool call denied by policy hook `{plugin}`: {reason}. \
                     Do not retry; this is a policy denial, not a transient failure."
                ),
                error_type: "hook_denied",
            },
        })
    }
}

pub(crate) struct HookDenial {
    pub message: String,
    pub error_type: &'static str,
}

/// Loads and executes plugin hooks.
#[derive(Debug, Default, Clone)]
pub struct HookManager {
    rules: HashMap<HookEvent, Vec<LoadedRule>>,
    use_login_shell_path: bool,
}

impl HookManager {
    /// Build a manager by scanning all enabled plugins for `hooks/hooks.json`.
    pub fn load(project_root: Option<&Path>, use_login_shell_path: bool) -> Self {
        let plugins = discover_enabled_plugins(project_root);
        Self::from_plugins(plugins, use_login_shell_path)
    }

    fn from_plugins(plugins: Vec<DiscoveredPlugin>, use_login_shell_path: bool) -> Self {
        let mut rules: HashMap<HookEvent, Vec<LoadedRule>> = HashMap::new();
        let mut total = 0usize;

        for plugin in plugins {
            let hooks_path = plugin.root.join("hooks").join("hooks.json");
            if !hooks_path.is_file() {
                continue;
            }
            match load_hooks_file(&hooks_path, &plugin.name, &plugin.root) {
                Ok(loaded) => {
                    for (event, plugin_rules) in loaded {
                        total += plugin_rules.len();
                        rules.entry(event).or_default().extend(plugin_rules);
                    }
                }
                Err(err) => warn!(
                    plugin = %plugin.name,
                    path = %hooks_path.display(),
                    error = %err,
                    "Failed to load plugin hooks; skipping",
                ),
            }
        }

        if total > 0 {
            info!(
                rule_count = total,
                events = ?rules.keys().map(|e| e.name()).collect::<Vec<_>>(),
                "Loaded plugin hooks",
            );
        }

        Self {
            rules,
            use_login_shell_path,
        }
    }

    /// Returns true if any rule is registered for `event`.
    pub fn has_hooks(&self, event: HookEvent) -> bool {
        self.rules.get(&event).is_some_and(|r| !r.is_empty())
    }

    async fn run_action(
        &self,
        event: HookEvent,
        session_id: &str,
        rule: &LoadedRule,
        command: &str,
        payload: &str,
        timeout: Duration,
    ) -> Result<HookRun> {
        let span = tracing::info_span!(
            target: "bcaip::hooks",
            "execute_hook",
            "gen_ai.operation.name" = "execute_hook",
            "bcaip.hook.event" = %event,
            "bcaip.hook.plugin" = %rule.plugin_name,
            "error.type" = tracing::field::Empty,
            session.id = %session_id,
        );
        let result = run_command_hook(
            command,
            &rule.plugin_root,
            payload,
            timeout,
            self.use_login_shell_path,
        )
        .instrument(span.clone())
        .await;
        match &result {
            Ok(run) if !run.output.status.success() => {
                span.record("error.type", "hook_exit");
            }
            Err(_) => {
                span.record("error.type", "hook_execution_error");
            }
            _ => {}
        }
        result
    }

    /// Fire all rules whose matcher matches the event context. Errors from
    /// individual hooks are logged but never propagated — a misbehaving hook
    /// MUST NOT crash the host tool.
    pub async fn emit(&self, event: HookEvent, ctx: HookContext) {
        if !self.has_hooks(event) {
            return;
        }
        let payload = match serde_json::to_string(&ctx) {
            Ok(s) => s,
            Err(err) => {
                warn!(event = %event, error = %err, "Failed to serialize hook context");
                return;
            }
        };
        self.emit_serialized(
            event,
            ctx.matcher_context.as_deref(),
            &ctx.session_id,
            &payload,
        )
        .await;
    }

    pub(crate) async fn emit_pre_tool_use_result(&self, payload: PreToolUseResultPayload) {
        let event = HookEvent::PreToolUseResult;
        if !self.has_hooks(event) {
            return;
        }
        let json = match serde_json::to_string(&payload) {
            Ok(s) => s,
            Err(err) => {
                warn!(event = %event, error = %err, "Failed to serialize hook context");
                return;
            }
        };
        self.emit_serialized(
            event,
            payload.context.matcher_context.as_deref(),
            &payload.context.session_id,
            &json,
        )
        .await;
    }

    async fn emit_serialized(
        &self,
        event: HookEvent,
        matcher_context: Option<&str>,
        session_id: &str,
        payload: &str,
    ) {
        let Some(rules) = self.rules.get(&event) else {
            return;
        };

        for rule in rules {
            if let Some(matcher) = &rule.matcher {
                let target = matcher_context.unwrap_or("");
                if !matcher.is_match(target) {
                    continue;
                }
            }

            for action in &rule.actions {
                let LoadedAction::Command {
                    command, timeout, ..
                } = action;
                debug!(
                    plugin = %rule.plugin_name,
                    event = %event,
                    command = %command,
                    "Running plugin hook",
                );
                let res = self
                    .run_action(event, session_id, rule, command, payload, *timeout)
                    .await
                    .and_then(|run| {
                        if run.output.status.success() {
                            Ok(())
                        } else {
                            anyhow::bail!(
                                "hook `{command}` exited with {:?}: {}",
                                run.output.status.code(),
                                String::from_utf8_lossy(&run.output.stderr).trim()
                            )
                        }
                    });
                if let Err(err) = res {
                    warn!(
                        plugin = %rule.plugin_name,
                        event = %event,
                        command = %command,
                        error = %err,
                        "Plugin hook failed",
                    );
                }
            }
        }
    }

    /// Like [`Self::emit`], but collects banner lines from hook stdout.
    ///
    /// If a hook exits successfully and its stdout contains valid JSON with a
    /// `"banner"` field, that string is collected. Multiple hooks can each
    /// contribute banner lines. Non-JSON stdout or missing `"banner"` field
    /// is silently ignored (backwards compatible).
    pub async fn emit_collecting_banners(&self, event: HookEvent, ctx: HookContext) -> Vec<String> {
        let mut banners = Vec::new();
        let Some(rules) = self.rules.get(&event) else {
            return banners;
        };
        if rules.is_empty() {
            return banners;
        }

        let payload = match serde_json::to_string(&ctx) {
            Ok(s) => s,
            Err(err) => {
                warn!(event = %event, error = %err, "Failed to serialize hook context");
                return banners;
            }
        };

        for rule in rules {
            if let Some(matcher) = &rule.matcher {
                let target = ctx.matcher_context.as_deref().unwrap_or("");
                if !matcher.is_match(target) {
                    continue;
                }
            }

            for action in &rule.actions {
                let LoadedAction::Command {
                    command, timeout, ..
                } = action;
                debug!(
                    plugin = %rule.plugin_name,
                    event = %event,
                    command = %command,
                    "Running plugin hook (banner-collecting)",
                );
                match run_command_hook(
                    command,
                    &rule.plugin_root,
                    &payload,
                    *timeout,
                    self.use_login_shell_path,
                )
                .await
                {
                    Ok(run) if run.output.status.success() => {
                        let stdout = String::from_utf8_lossy(&run.output.stdout);
                        if let Some(banner) = extract_banner(stdout.trim()) {
                            banners.push(banner);
                        }
                    }
                    Ok(run) => {
                        warn!(
                            plugin = %rule.plugin_name,
                            event = %event,
                            command = %command,
                            "hook exited with {:?}: {}",
                            run.output.status.code(),
                            String::from_utf8_lossy(&run.output.stderr).trim(),
                        );
                    }
                    Err(err) => {
                        warn!(
                            plugin = %rule.plugin_name,
                            event = %event,
                            command = %command,
                            error = %err,
                            "Plugin hook failed",
                        );
                    }
                }
            }
        }

        banners
    }

    /// Like [`Self::emit`], but stops at the first rule that denies the event
    /// and returns the denial. A hook denies by exiting with status code 2
    /// (reason on stderr) or by printing `{"decision":"block","reason":"..."}`
    pub async fn emit_blocking(&self, event: HookEvent, ctx: HookContext) -> HookDecision {
        self.emit_blocking_with_outcome(event, ctx).await.decision
    }

    pub(crate) async fn emit_blocking_with_outcome(
        &self,
        event: HookEvent,
        ctx: HookContext,
    ) -> HookChainOutcome {
        let matched = self.matching_actions(event, &ctx);
        if matched.is_empty() {
            return HookChainOutcome::allow(false);
        }

        let payload = match serde_json::to_string(&ctx) {
            Ok(payload) => payload,
            Err(err) => {
                warn!(event = %event, error = %err, "Failed to serialize hook context");
                return serialization_failure_outcome(&matched, event);
            }
        };

        let mut policy_evaluated = false;
        let mut failed = false;

        for (rule, action) in matched {
            let LoadedAction::Command {
                command,
                timeout,
                on_failure,
            } = action;
            let (verdict, evaluated, already_logged) = match self
                .run_action(event, &ctx.session_id, rule, command, &payload, *timeout)
                .await
            {
                Ok(run) => {
                    let verdict = classify_run(&run);
                    let evaluated = run.output.status.success()
                        || matches!(verdict, HookVerdict::PolicyDeny { .. });
                    (verdict, evaluated, false)
                }
                Err(err) => {
                    warn!(
                        plugin = %rule.plugin_name,
                        event = %event,
                        command = %command,
                        error = %format!("{err:#}"),
                        "Plugin hook could not be executed",
                    );
                    (
                        HookVerdict::HookFailure {
                            reason: COMMAND_FAILED_REASON.to_string(),
                        },
                        false,
                        true,
                    )
                }
            };
            policy_evaluated |= evaluated;

            match apply_verdict(verdict, *on_failure, event) {
                ChainStep::Allowed => {}
                ChainStep::FailedOpen { reason } => {
                    if !already_logged {
                        warn!(
                            plugin = %rule.plugin_name,
                            event = %event,
                            command = %command,
                            reason = %reason,
                            "Plugin hook failed; continuing without it",
                        );
                    }
                    failed = true;
                }
                ChainStep::Denied { reason, cause } => {
                    info!(
                        plugin = %rule.plugin_name,
                        event = %event,
                        command = %command,
                        cause = cause.as_str(),
                        reason = %reason,
                        "Plugin hook denied tool call",
                    );
                    return HookChainOutcome {
                        decision: HookDecision::Deny {
                            reason,
                            plugin: rule.plugin_name.clone(),
                        },
                        policy_evaluated,
                        cause: Some(cause),
                    };
                }
            }
        }

        HookChainOutcome {
            decision: HookDecision::Allow,
            policy_evaluated,
            cause: failed.then_some(HookOutcomeCause::HookFailure),
        }
    }

    fn matching_actions(
        &self,
        event: HookEvent,
        ctx: &HookContext,
    ) -> Vec<(&LoadedRule, &LoadedAction)> {
        let Some(rules) = self.rules.get(&event) else {
            return Vec::new();
        };
        let target = ctx.matcher_context.as_deref().unwrap_or("");
        rules
            .iter()
            .filter(|rule| rule.matcher.as_ref().is_none_or(|m| m.is_match(target)))
            .flat_map(|rule| rule.actions.iter().map(move |action| (rule, action)))
            .collect()
    }
}

fn serialization_failure_outcome(
    matched: &[(&LoadedRule, &LoadedAction)],
    event: HookEvent,
) -> HookChainOutcome {
    let reason = SERIALIZATION_FAILED_REASON.to_string();
    let blocker = matched
        .iter()
        .find(|(_, action)| action.on_failure() == OnFailure::Block);
    match blocker {
        Some((rule, _)) if event == HookEvent::PreToolUse => HookChainOutcome {
            decision: HookDecision::Deny {
                reason,
                plugin: rule.plugin_name.clone(),
            },
            policy_evaluated: false,
            cause: Some(HookOutcomeCause::HookFailure),
        },
        _ => HookChainOutcome {
            decision: HookDecision::Allow,
            policy_evaluated: false,
            cause: Some(HookOutcomeCause::HookFailure),
        },
    }
}

#[derive(Debug)]
struct HookRun {
    output: std::process::Output,
    stdin_delivered: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum HookVerdict {
    Allow,
    PolicyDeny { reason: String },
    HookFailure { reason: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum ChainStep {
    Allowed,
    Denied {
        reason: String,
        cause: HookOutcomeCause,
    },
    FailedOpen {
        reason: String,
    },
}

fn apply_verdict(verdict: HookVerdict, on_failure: OnFailure, event: HookEvent) -> ChainStep {
    match verdict {
        HookVerdict::Allow => ChainStep::Allowed,
        HookVerdict::PolicyDeny { reason } => ChainStep::Denied {
            reason,
            cause: HookOutcomeCause::PolicyDenial,
        },
        HookVerdict::HookFailure { reason } => {
            if on_failure == OnFailure::Block && event == HookEvent::PreToolUse {
                ChainStep::Denied {
                    reason,
                    cause: HookOutcomeCause::HookFailure,
                }
            } else {
                ChainStep::FailedOpen { reason }
            }
        }
    }
}

fn extract_banner(stdout: &str) -> Option<String> {
    if !stdout.starts_with('{') {
        return None;
    }

    #[derive(Deserialize)]
    struct BannerResp {
        banner: Option<String>,
    }

    let parsed: BannerResp = serde_json::from_str(stdout).ok()?;
    parsed.banner.filter(|b| !b.is_empty())
}

fn classify_run(run: &HookRun) -> HookVerdict {
    let verdict = classify_output(&run.output);
    if run.stdin_delivered || matches!(verdict, HookVerdict::PolicyDeny { .. }) {
        return verdict;
    }
    HookVerdict::HookFailure {
        reason: STDIN_DELIVERY_FAILED_REASON.to_string(),
    }
}

fn classify_output(output: &std::process::Output) -> HookVerdict {
    const DEFAULT_DENY: &str = "denied by plugin hook";
    let non_empty = |s: String| if s.is_empty() { DEFAULT_DENY.into() } else { s };

    if output.status.code() == Some(2) {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return HookVerdict::PolicyDeny {
            reason: non_empty(stderr),
        };
    }

    #[derive(Deserialize)]
    struct Resp {
        decision: Option<String>,
        reason: Option<String>,
    }

    let Ok(stdout) = std::str::from_utf8(&output.stdout) else {
        return HookVerdict::HookFailure {
            reason: "the hook wrote invalid UTF-8 to stdout".to_string(),
        };
    };
    let trimmed = stdout.trim();
    let parsed = trimmed
        .starts_with('{')
        .then(|| serde_json::from_str::<Resp>(trimmed).ok())
        .flatten();
    let (decision, reason) = match parsed {
        Some(resp) => (resp.decision, resp.reason),
        None => (None, None),
    };

    if decision.as_deref() == Some("block") {
        return HookVerdict::PolicyDeny {
            reason: non_empty(reason.unwrap_or_default()),
        };
    }
    if output.status.code() == Some(0)
        && (trimmed.is_empty() || decision.as_deref() == Some("allow"))
    {
        return HookVerdict::Allow;
    }

    HookVerdict::HookFailure {
        reason: match output.status.code() {
            Some(0) => "the hook exited 0 without an allow or block decision on stdout".to_string(),
            Some(code) => format!("the hook exited with status {code} and no usable decision"),
            None => "the hook was terminated by a signal".to_string(),
        },
    }
}

fn load_hooks_file(
    path: &Path,
    plugin_name: &str,
    plugin_root: &Path,
) -> Result<HashMap<HookEvent, Vec<LoadedRule>>> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let parsed: HooksFile =
        serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;

    let mut out: HashMap<HookEvent, Vec<LoadedRule>> = HashMap::new();
    for (event_name, raw_payload) in parsed.hooks {
        let Some(event) = HookEvent::from_name(&event_name) else {
            debug!(plugin = plugin_name, event = %event_name, "Ignoring unknown hook event");
            continue;
        };
        let raw_rules: Vec<RawHookRule> = serde_json::from_value(raw_payload)
            .with_context(|| format!("reading `{event_name}` rules in {}", path.display()))?;

        for raw in raw_rules {
            let mut selected = Vec::new();
            for raw_action in raw.hooks {
                if select_action(&raw_action, plugin_name, path)? == ActionSelection::Ignored {
                    continue;
                }
                let action: RawCommandAction = serde_json::from_value(raw_action)
                    .with_context(|| format!("reading hook action in {}", path.display()))?;
                selected.push(action);
            }

            let matcher = match raw.matcher.as_deref().filter(|s| !s.is_empty()) {
                Some(pattern) => match Regex::new(pattern) {
                    Ok(re) => Some(re),
                    Err(err) => {
                        warn!(
                            plugin = plugin_name,
                            pattern,
                            error = %err,
                            "Invalid hook matcher regex; skipping rule",
                        );
                        continue;
                    }
                },
                None => None,
            };

            let mut actions = Vec::new();
            for action in selected {
                let timeout =
                    Duration::from_secs(action.timeout.unwrap_or(DEFAULT_HOOK_TIMEOUT_SECS));
                let on_failure = if event == HookEvent::PreToolUse {
                    match action.on_failure {
                        None => OnFailure::Allow,
                        Some(value) => serde_json::from_value::<OnFailure>(value)
                            .with_context(|| format!("reading on_failure in {}", path.display()))?,
                    }
                } else {
                    OnFailure::Allow
                };
                actions.push(LoadedAction::Command {
                    command: action.command,
                    timeout,
                    on_failure,
                });
            }

            if actions.is_empty() {
                continue;
            }

            out.entry(event).or_default().push(LoadedRule {
                plugin_name: plugin_name.to_string(),
                plugin_root: plugin_root.to_path_buf(),
                matcher,
                actions,
            });
        }
    }

    Ok(out)
}

async fn run_command_hook(
    raw_command: &str,
    plugin_root: &Path,
    payload: &str,
    timeout: Duration,
    use_login_shell_path: bool,
) -> Result<HookRun> {
    match tokio::time::timeout(
        timeout,
        run_command_hook_inner(raw_command, plugin_root, payload, use_login_shell_path),
    )
    .await
    {
        Ok(res) => res,
        Err(_) => anyhow::bail!("hook `{raw_command}` timed out after {:?}", timeout),
    }
}

async fn run_command_hook_inner(
    raw_command: &str,
    plugin_root: &Path,
    payload: &str,
    use_login_shell_path: bool,
) -> Result<HookRun> {
    let command = expand_plugin_root(raw_command, plugin_root);
    let path = if use_login_shell_path {
        hook_path().await
    } else {
        None
    };
    let mut process = hook_command(&command, plugin_root, path.as_deref());
    process
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    let mut child = process
        .spawn()
        .with_context(|| format!("spawning hook `{command}`"))?;

    let mut stdin_delivered = true;
    if let Some(mut stdin) = child.stdin.take() {
        if let Err(err) = stdin.write_all(payload.as_bytes()).await {
            stdin_delivered = false;
            warn!(command = %command, error = %err, "Could not deliver the hook payload");
        } else if let Err(err) = stdin.shutdown().await {
            stdin_delivered = false;
            warn!(command = %command, error = %err, "Could not close the hook stdin pipe");
        }
    }

    let output = child
        .wait_with_output()
        .await
        .with_context(|| format!("waiting on hook `{command}`"))?;
    Ok(HookRun {
        output,
        stdin_delivered,
    })
}

fn hook_command(command: &str, plugin_root: &Path, path: Option<&str>) -> Command {
    #[cfg(not(windows))]
    {
        if crate::agents::platform_extensions::developer::shell::is_flatpak() {
            let mut process =
                crate::agents::platform_extensions::developer::shell::flatpak_spawn_command();
            process.arg(format!("--env=PLUGIN_ROOT={}", plugin_root.display()));
            if let Some(path) = path {
                process.arg(format!("--env=PATH={path}"));
            }
            process.arg("sh").arg("-c").arg(command);
            return process;
        }
    }

    let mut process = Command::new("sh");
    process
        .arg("-c")
        .arg(command)
        .env("PLUGIN_ROOT", plugin_root);
    if let Some(path) = path {
        process.env("PATH", path);
    }
    process
}

async fn hook_path() -> Option<String> {
    static HOOK_PATH: OnceLock<tokio::sync::watch::Receiver<Option<String>>> = OnceLock::new();
    let mut rx = HOOK_PATH
        .get_or_init(|| {
            let (tx, rx) = tokio::sync::watch::channel(None);
            tokio::spawn(async move {
                let path = resolve_hook_path().await;
                let _ = tx.send(path);
            });
            rx
        })
        .clone();

    if rx.borrow().is_some() {
        return rx.borrow().clone();
    }
    if rx.changed().await.is_ok() {
        rx.borrow().clone()
    } else {
        None
    }
}

async fn resolve_hook_path() -> Option<String> {
    #[cfg(not(windows))]
    {
        tokio::task::spawn_blocking(|| {
            crate::agents::platform_extensions::developer::shell::resolve_login_shell_path()
                .map(|login| merge_paths(&login, &std::env::var("PATH").unwrap_or_default()))
        })
        .await
        .ok()
        .flatten()
    }
    #[cfg(windows)]
    {
        None
    }
}

fn merge_paths(first: &str, second: &str) -> String {
    let mut seen = std::collections::HashSet::new();
    let mut merged = Vec::new();
    for entry in first.split(':').chain(second.split(':')) {
        if !entry.is_empty() && seen.insert(entry) {
            merged.push(entry);
        }
    }
    merged.join(":")
}

fn expand_plugin_root(command: &str, plugin_root: &Path) -> String {
    command.replace("${PLUGIN_ROOT}", &plugin_root.to_string_lossy())
}

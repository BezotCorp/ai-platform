use super::base::ProviderDef;
use super::cli_common::{error_from_event, extract_usage_tokens};
use super::private_file::create_private_named_temp_file;
use super::utils::filter_extensions_from_system_prompt;
use crate::config::{Config, ExtensionConfig};
use crate::config::{paths::Paths, search_path::SearchPaths};
use crate::subprocess::configure_subprocess;
use anyhow::Result;
use async_stream::try_stream;
use async_trait::async_trait;
use futures::future::BoxFuture;
use bcaip_provider_types::base::{
    ConfigKey, MessageStream, PermissionRouting, Provider, ProviderMetadata,
};
use bcaip_provider_types::conversations::{Message, MessageContent};
use bcaip_provider_types::conversations::{ProviderUsage, Usage};
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::goose_mode::GooseMode;
use bcaip_provider_types::model::ModelConfig;
use bcaip_provider_types::permission::PrincipalType;
use bcaip_provider_types::permission::{Permission, PermissionConfirmation};
use rmcp::model::{Role, Tool};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::{collections::HashMap, io::Write};
use std::{process::Stdio, sync::Arc};
use tempfile::NamedTempFile;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::{process::Command, sync::oneshot};
const CLAUDE_CODE_PROVIDER_NAME: &str = "claude-code";
pub const CLAUDE_CODE_DEFAULT_MODEL: &str = "default";
pub const CLAUDE_CODE_DOC_URL: &str = "https://code.claude.com/docs/en/setup";

// https://github.com/anthropics/claude-agent-sdk-python/blob/0e9397e/src/claude_agent_sdk/types.py#L857-L859
#[derive(Serialize)]
struct ControlResponse<T: Serialize> {
    #[serde(rename = "type")]
    msg_type: &'static str,
    response: ControlResponseBody<T>,
}

#[derive(Serialize)]
struct ControlResponseBody<T: Serialize> {
    subtype: &'static str,
    request_id: String,
    response: T,
}

// https://github.com/anthropics/claude-agent-sdk-python/blob/0e9397e/src/claude_agent_sdk/types.py#L135-L153
#[derive(Serialize)]
#[serde(tag = "behavior")]
enum PermissionResponse {
    #[serde(rename = "allow")]
    Allow {
        #[serde(rename = "updatedInput")]
        updated_input: serde_json::Map<String, Value>,
        #[serde(rename = "toolUseID")]
        tool_use_id: String,
    },
    #[serde(rename = "deny")]
    Deny { message: String },
}

#[derive(Serialize)]
struct ControlRequest {
    #[serde(rename = "type")]
    msg_type: &'static str,
    request_id: String,
    request: ControlRequestBody,
}

#[derive(Serialize)]
#[serde(tag = "subtype")]
enum ControlRequestBody {
    #[serde(rename = "initialize")]
    Initialize,
    #[serde(rename = "set_model")]
    SetModel { model: String },
}

impl ControlRequestBody {
    fn label(&self) -> &'static str {
        match self {
            Self::Initialize => "initialize",
            Self::SetModel { .. } => "set_model",
        }
    }
}

#[derive(Deserialize)]
struct IncomingControlResponse {
    response: IncomingControlResponseBody,
}

#[derive(Deserialize)]
#[serde(tag = "subtype")]
enum IncomingControlResponseBody {
    #[serde(rename = "success")]
    Success {
        request_id: String,
        #[serde(default)]
        response: Option<Value>,
    },
    #[serde(rename = "error")]
    Error {
        request_id: String,
        #[serde(default)]
        error: String,
    },
}

#[derive(Deserialize)]
struct IncomingControlRequest {
    request_id: String,
    request: IncomingRequestBody,
}

#[derive(Deserialize)]
#[serde(tag = "subtype")]
enum IncomingRequestBody {
    #[serde(rename = "can_use_tool")]
    CanUseTool {
        tool_name: String,
        #[serde(default)]
        input: serde_json::Map<String, Value>,
        #[serde(default)]
        tool_use_id: String,
    },
}

impl<T: Serialize> ControlResponse<T> {
    fn success(request_id: String, response: T) -> Self {
        Self {
            msg_type: "control_response",
            response: ControlResponseBody {
                subtype: "success",
                request_id,
                response,
            },
        }
    }
}

struct CliProcess {
    child: tokio::process::Child,
    stdin: Box<dyn tokio::io::AsyncWrite + Unpin + Send>,
    reader: BufReader<Box<dyn tokio::io::AsyncRead + Unpin + Send>>,
    #[allow(dead_code)]
    stderr_handle: tokio::task::JoinHandle<String>,
    current_model: String,
    log_model_update: bool,
    next_request_id: u64,
    needs_drain: bool,
    _system_prompt_file: NamedTempFile,
}

impl std::fmt::Debug for CliProcess {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CliProcess")
            .field("current_model", &self.current_model)
            .field("next_request_id", &self.next_request_id)
            .finish_non_exhaustive()
    }
}

impl CliProcess {
    fn next_request_id(&mut self) -> String {
        let id = self.next_request_id;
        self.next_request_id += 1;
        format!("req_{id}")
    }

    async fn send_control_request(
        &mut self,
        body: ControlRequestBody,
    ) -> Result<Option<Value>, ProviderError> {
        let request_id = self.next_request_id();
        exchange_control(&mut self.stdin, &mut self.reader, &request_id, body).await
    }

    async fn send_set_model(&mut self, model: &str) -> Result<(), ProviderError> {
        if model == self.current_model {
            return Ok(());
        }
        self.send_control_request(ControlRequestBody::SetModel {
            model: model.to_string(),
        })
        .await?;
        self.current_model = model.to_string();
        self.log_model_update = true;
        Ok(())
    }

    async fn drain_pending_response(&mut self) {
        if !self.needs_drain {
            return;
        }
        tracing::debug!("Draining cancelled response from CLI process");

        let drain = async {
            let mut line = String::new();
            loop {
                line.clear();
                match self.reader.read_line(&mut line).await {
                    Ok(0) => break,
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }
                        if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                            match parsed.get("type").and_then(|t| t.as_str()) {
                                Some("result") | Some("error") => break,
                                _ => continue,
                            }
                        } else {
                            tracing::trace!(line = trimmed, "Non-JSON line during drain");
                        }
                    }
                    Err(_) => break,
                }
            }
        };

        const DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
        if tokio::time::timeout(DRAIN_TIMEOUT, drain).await.is_err() {
            // CLI is still producing the old response. Leave needs_drain
            // true so the next call retries — by then the old response
            // likely completed and drain will succeed quickly.
            tracing::warn!(
                "Drain did not complete in {DRAIN_TIMEOUT:?}; \
                 will retry on next request"
            );
            return;
        }

        self.needs_drain = false;
        tracing::debug!("Drain complete, protocol re-synced");
    }
}

impl Drop for CliProcess {
    fn drop(&mut self) {
        self.stderr_handle.abort();
        let _ = self.child.start_kill();
    }
}

/// Spawns the Claude Code CLI (`claude`) as a persistent child process using
/// `--input-format stream-json --output-format stream-json`. The CLI stays alive
/// across turns, maintaining conversation state internally. Messages are sent as
/// NDJSON on stdin with content arrays supporting text and image blocks. Responses
/// are NDJSON on stdout (`assistant` + `result` events per turn).
#[derive(Debug, serde::Serialize)]
pub struct ClaudeCodeProvider {
    command: PathBuf,
    #[serde(skip)]
    name: String,
    /// Temp file holding MCP config JSON (auto-deleted on drop).
    #[serde(skip)]
    mcp_config_file: Option<NamedTempFile>,
    #[serde(skip)]
    cli_process: tokio::sync::OnceCell<Arc<tokio::sync::Mutex<CliProcess>>>,
    #[serde(skip)]
    pending_confirmations:
        Arc<tokio::sync::Mutex<HashMap<String, oneshot::Sender<PermissionConfirmation>>>>,
    #[serde(skip)]
    initial_mode: tokio::sync::Mutex<Option<GooseMode>>,
}

impl ClaudeCodeProvider {
    /// Build content blocks from the last user message only. The CLI maintains
    /// conversation context internally per session_id.
    fn last_user_content_blocks(&self, messages: &[Message]) -> Vec<Value> {
        let msgs = match messages.iter().rev().find(|m| m.role == Role::User) {
            Some(msg) => std::slice::from_ref(msg),
            None => messages,
        };
        let mut blocks: Vec<Value> = Vec::new();
        for message in msgs {
            let prefix = match message.role {
                Role::User => "Human: ",
                Role::Assistant => "Assistant: ",
            };
            let mut text_parts = Vec::new();
            for content in &message.content {
                match content {
                    MessageContent::Text(t) => text_parts.push(t.text.clone()),
                    MessageContent::Image(img) => {
                        if !text_parts.is_empty() {
                            blocks.push(json!({"type":"text","text":format!("{}{}", prefix, text_parts.join("\n"))}));
                            text_parts.clear();
                        }
                        blocks.push(json!({"type":"image","source":{"type":"base64","media_type":img.mime_type,"data":img.data}}));
                    }
                    MessageContent::ToolRequest(req) => {
                        if let Ok(call) = &req.tool_call {
                            text_parts.push(format!("[tool_use: {} id={}]", call.name, req.id));
                        }
                    }
                    MessageContent::ToolResponse(resp) => {
                        if let Ok(result) = &resp.tool_result {
                            let text: String = result
                                .content
                                .iter()
                                .filter_map(|c| match c {
                                    rmcp::model::ContentBlock::Text(t) => Some(t.text.as_str()),
                                    _ => None,
                                })
                                .collect::<Vec<&str>>()
                                .join("\n");
                            text_parts.push(format!("[tool_result id={}] {}", resp.id, text));
                        }
                    }
                    _ => {}
                }
            }
            if !text_parts.is_empty() {
                blocks.push(
                    json!({"type":"text","text":format!("{}{}", prefix, text_parts.join("\n"))}),
                );
            }
        }
        blocks
    }

    fn build_stream_json_command(&self) -> Command {
        let mut cmd = Command::new(&self.command);
        configure_subprocess(&mut cmd);
        // Allow goose to run inside a Claude Code session.
        cmd.env_remove("CLAUDECODE");
        cmd.arg("--input-format")
            .arg("stream-json")
            .arg("--output-format")
            .arg("stream-json")
            .arg("--verbose")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd
    }

    /// Returns true when the control protocol is enabled.
    fn apply_permission_flags(cmd: &mut Command) -> Result<bool, ProviderError> {
        let config = Config::global();
        let goose_mode = config.get_goose_mode().unwrap_or(GooseMode::Auto);

        match goose_mode {
            GooseMode::Auto => {
                cmd.arg("--dangerously-skip-permissions");
                Ok(false)
            }
            GooseMode::SmartApprove | GooseMode::Approve => {
                cmd.arg("--permission-prompt-tool").arg("stdio");
                Ok(true)
            }
            GooseMode::Chat => Ok(false),
        }
    }

    fn apply_system_prompt_file(
        cmd: &mut Command,
        state_dir: &Path,
        filtered_system: &str,
    ) -> Result<NamedTempFile, ProviderError> {
        let system_prompt_file =
            write_system_prompt_file(state_dir, filtered_system).map_err(|e| {
                ProviderError::RequestFailed(format!(
                    "Failed to create Claude CLI system prompt file: {e}"
                ))
            })?;
        cmd.arg("--system-prompt-file")
            .arg(system_prompt_file.path());
        Ok(system_prompt_file)
    }

    async fn spawn_process(
        &self,
        model: &ModelConfig,
        filtered_system: &str,
    ) -> Result<CliProcess, ProviderError> {
        let mut cmd = self.build_stream_json_command();

        if let Some(f) = &self.mcp_config_file {
            cmd.arg("--mcp-config").arg(f.path());
            cmd.arg("--strict-mcp-config");
        }

        cmd.arg("--include-partial-messages");
        let system_prompt_file =
            Self::apply_system_prompt_file(&mut cmd, &Paths::state_dir(), filtered_system)?;
        cmd.arg("--model").arg(&model.model_name);

        let control_protocol_enabled = Self::apply_permission_flags(&mut cmd)?;

        let mut child = cmd.spawn().map_err(|e| {
            ProviderError::RequestFailed(format!(
                "Failed to spawn Claude CLI command '{:?}': {}.",
                self.command, e
            ))
        })?;

        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::RequestFailed("Failed to capture stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::RequestFailed("Failed to capture stdout".to_string()))?;

        let stderr = child.stderr.take();
        let stderr_handle = tokio::spawn(async move {
            let mut output = String::new();
            if let Some(mut stderr) = stderr {
                use tokio::io::AsyncReadExt;
                let _ = stderr.read_to_string(&mut output).await;
            }
            output
        });

        let mut process = CliProcess {
            child,
            stdin: Box::new(stdin),
            reader: BufReader::new(Box::new(stdout)),
            stderr_handle,
            current_model: model.model_name.clone(),
            log_model_update: false,
            next_request_id: 0,
            needs_drain: false,
            _system_prompt_file: system_prompt_file,
        };

        if control_protocol_enabled {
            process
                .send_control_request(ControlRequestBody::Initialize)
                .await?;
        }

        Ok(process)
    }

    async fn get_or_init_process(
        &self,
        model_config: &ModelConfig,
        filtered_system: &str,
    ) -> Result<&Arc<tokio::sync::Mutex<CliProcess>>, ProviderError> {
        self.cli_process
            .get_or_try_init(|| async {
                Ok(Arc::new(tokio::sync::Mutex::new(
                    self.spawn_process(model_config, filtered_system).await?,
                )))
            })
            .await
    }
}

async fn exchange_control(
    stdin: &mut (impl AsyncWrite + Unpin),
    reader: &mut (impl AsyncBufRead + Unpin),
    request_id: &str,
    body: ControlRequestBody,
) -> Result<Option<Value>, ProviderError> {
    let label = body.label();
    let req = ControlRequest {
        msg_type: "control_request",
        request_id: request_id.to_string(),
        request: body,
    };
    let mut req_str = serde_json::to_string(&req).map_err(|e| {
        ProviderError::RequestFailed(format!("Failed to serialize {label} request: {e}"))
    })?;
    req_str.push('\n');
    stdin.write_all(req_str.as_bytes()).await.map_err(|e| {
        ProviderError::RequestFailed(format!("Failed to write {label} request: {e}"))
    })?;

    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => {
                return Err(ProviderError::RequestFailed(format!(
                    "CLI process terminated while waiting for {label} response"
                )));
            }
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                if let Ok(msg) = serde_json::from_str::<IncomingControlResponse>(trimmed) {
                    match msg.response {
                        IncomingControlResponseBody::Success {
                            request_id: ref rid,
                            response,
                        } if rid == request_id => return Ok(response),
                        IncomingControlResponseBody::Error {
                            request_id: ref rid,
                            error,
                        } if rid == request_id => {
                            return Err(ProviderError::RequestFailed(format!(
                                "{label} failed: {error}"
                            )));
                        }
                        _ => continue,
                    }
                }
            }
            Err(e) => {
                return Err(ProviderError::RequestFailed(format!(
                    "Failed to read {label} response: {e}"
                )));
            }
        }
    }
}

fn extract_model_aliases(response: Option<&Value>) -> Vec<String> {
    response
        .and_then(|v| v.get("models")?.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("value")?.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default()
}

fn build_stream_json_input(content_blocks: &[Value], session_id: &str) -> String {
    let msg = json!({"type":"user","session_id":session_id,"message":{"role":"user","content":content_blocks}});
    serde_json::to_string(&msg).expect("serializing JSON content blocks cannot fail")
}

fn claude_mcp_config_json(extensions: &[ExtensionConfig]) -> Option<String> {
    let mut mcp_servers = serde_json::Map::new();

    for extension in extensions {
        match extension {
            ExtensionConfig::StreamableHttp { uri, headers, .. } => {
                let key = extension.key();
                let mut config = serde_json::Map::new();
                config.insert("type".to_string(), json!("http"));
                config.insert("url".to_string(), json!(uri));
                if !headers.is_empty() {
                    config.insert("headers".to_string(), json!(headers));
                }
                mcp_servers.insert(key, Value::Object(config));
            }
            ExtensionConfig::Stdio {
                cmd, args, envs, ..
            } => {
                let key = extension.key();
                let mut config = serde_json::Map::new();
                config.insert("type".to_string(), json!("stdio"));
                config.insert("command".to_string(), json!(cmd));
                if !args.is_empty() {
                    config.insert("args".to_string(), json!(args));
                }
                let env_map = envs.get_env();
                if !env_map.is_empty() {
                    config.insert("env".to_string(), json!(env_map));
                }
                mcp_servers.insert(key, Value::Object(config));
            }
            _ => {}
        }
    }

    if mcp_servers.is_empty() {
        return None;
    }

    serde_json::to_string(&json!({ "mcpServers": mcp_servers })).ok()
}

fn write_claude_temp_file(
    state_dir: &Path,
    prefix: &str,
    suffix: &str,
    contents: &str,
) -> Result<NamedTempFile, anyhow::Error> {
    let dir = state_dir.join("claude-code");
    std::fs::create_dir_all(&dir)?;
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix).suffix(suffix);
    let mut tmp = create_private_named_temp_file(&mut builder, &dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tmp.as_file()
            .set_permissions(std::fs::Permissions::from_mode(0o600))?;
    }
    tmp.write_all(contents.as_bytes())?;
    Ok(tmp)
}

/// Write the MCP config JSON to a temp file with restricted permissions
/// so secrets (headers, env vars) are not leaked via process argv.
fn write_mcp_config_file(state_dir: &Path, json: &str) -> Result<NamedTempFile, anyhow::Error> {
    let prefix = format!("mcp-config-{}_", chrono::Utc::now().format("%Y%m%d"));
    write_claude_temp_file(state_dir, &prefix, ".json", json)
}

fn write_system_prompt_file(
    state_dir: &Path,
    system_prompt: &str,
) -> Result<NamedTempFile, anyhow::Error> {
    let prefix = format!("system-prompt-{}_", chrono::Utc::now().format("%Y%m%d"));
    write_claude_temp_file(state_dir, &prefix, ".txt", system_prompt)
}

impl bcaip_provider_types::base::ProviderDescriptor for ClaudeCodeProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            CLAUDE_CODE_PROVIDER_NAME,
            "Claude Code CLI",
            "[Deprecated: use claude-acp instead] Requires claude CLI installed, no MCPs. Use claude-acp for ACP support with extensions.",
            CLAUDE_CODE_DEFAULT_MODEL,
            // Only a few agentic choices; fetched dynamically via fetch_supported_models.
            vec![],
            CLAUDE_CODE_DOC_URL,
            vec![ConfigKey::new(
                "CLAUDE_CODE_COMMAND",
                true,
                false,
                Some("claude"),
                true,
            )],
        )
        .deprecated(Some("claude-acp"))
    }
}

impl ProviderDef for ClaudeCodeProvider {
    type Provider = Self;

    fn from_env(
        extensions: Vec<ExtensionConfig>,
        _tls_config: Option<goose_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(async move {
            let config = crate::config::Config::global();
            let command: String = config.get_claude_code_command().unwrap_or_default().into();
            let resolved_command = SearchPaths::builder().with_npm().resolve(command)?;

            let mut resolved = Vec::with_capacity(extensions.len());
            for ext in extensions {
                resolved.push(ext.resolve(config).await?);
            }

            let mcp_config_file = claude_mcp_config_json(&resolved)
                .map(|json| write_mcp_config_file(&Paths::state_dir(), &json))
                .transpose()?;

            Ok(Self {
                command: resolved_command,
                name: CLAUDE_CODE_PROVIDER_NAME.to_string(),
                mcp_config_file,
                cli_process: tokio::sync::OnceCell::new(),
                pending_confirmations: Arc::new(tokio::sync::Mutex::new(HashMap::new())),
                initial_mode: tokio::sync::Mutex::new(None),
            })
        })
    }
}

#[async_trait]
impl Provider for ClaudeCodeProvider {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn manages_own_context(&self) -> bool {
        true
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        // Uses a separate short-lived process because --system-prompt-file is a CLI-only
        // flag with no NDJSON equivalent. The persistent process needs it at spawn,
        // but it's unavailable during model listing.
        // See: https://code.claude.com/docs/en/cli-reference#system-prompt-flags
        let mut cmd = self.build_stream_json_command();
        let mut child = cmd.spawn().map_err(|e| {
            ProviderError::RequestFailed(format!("Failed to spawn CLI for model listing: {e}"))
        })?;

        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| ProviderError::RequestFailed("Failed to capture stdin".to_string()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::RequestFailed("Failed to capture stdout".to_string()))?;

        let mut reader = BufReader::new(stdout);
        let response = exchange_control(
            &mut stdin,
            &mut reader,
            "model_list",
            ControlRequestBody::Initialize,
        )
        .await;
        let _ = child.kill().await;
        Ok(extract_model_aliases(response.ok().flatten().as_ref()))
    }

    async fn update_mode(&self, _session_id: &str, mode: GooseMode) -> Result<(), ProviderError> {
        // Mode is baked into the subprocess at spawn; claude-acp replaces
        // this provider (#7801).
        let mut guard = self.initial_mode.lock().await;
        let current = *guard.get_or_insert(mode);
        if current != mode {
            return Err(ProviderError::RequestFailed(format!(
                "Mode change not supported: session is {current}, requested {mode}",
            )));
        }
        Ok(())
    }

    fn permission_routing(&self) -> PermissionRouting {
        PermissionRouting::ActionRequired
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
        system: &str,
        messages: &[Message],
        _tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let session_id = crate::session_context::current_session_id().unwrap_or_default();
        let filtered_system = filter_extensions_from_system_prompt(system);
        let process_arc = Arc::clone(
            self.get_or_init_process(model_config, &filtered_system)
                .await?,
        );

        // Prepare the payload outside the lock — these don't need the process.
        let blocks = self.last_user_content_blocks(messages);
        let ndjson_line = build_stream_json_input(&blocks, &session_id);
        let model_name = model_config.model_name.clone();
        let mut current_text_message_id = uuid::Uuid::new_v4().to_string();
        let pending_confirmations = Arc::clone(&self.pending_confirmations);

        Ok(Box::pin(try_stream! {
            // Single lock acquisition covers write-to-stdin and read-from-stdout,
            // eliminating the race window between the two.
            let mut process = process_arc.lock_owned().await;

            // Clean up pending permissions from a cancelled stream
            {
                let mut pending = pending_confirmations.lock().await;
                for (req_id, tx) in pending.drain() {
                    drop(tx);
                    let resp = ControlResponse::success(
                        req_id,
                        PermissionResponse::Deny { message: "Stream cancelled".to_string() },
                    );
                    let mut s = serde_json::to_string(&resp).map_err(|e| {
                        ProviderError::RequestFailed(format!("Failed to serialize cleanup deny response: {e}"))
                    })?;
                    s.push('\n');
                    let _ = process.stdin.write_all(s.as_bytes()).await;
                }
            }

            process.drain_pending_response().await;
            process.send_set_model(&model_name).await?;

            process
                .stdin
                .write_all(ndjson_line.as_bytes())
                .await
                .map_err(|e| {
                    ProviderError::RequestFailed(format!("Failed to write to stdin: {}", e))
                })?;
            process.stdin.write_all(b"\n").await.map_err(|e| {
                ProviderError::RequestFailed(format!("Failed to write newline to stdin: {}", e))
            })?;

            process.needs_drain = true;
            let mut line = String::new();
            let mut accumulated_usage = Usage::default();
            let mut stream_error: Option<ProviderError> = None;
            let stream_timestamp = chrono::Utc::now().timestamp();

            loop {
                line.clear();
                match process.reader.read_line(&mut line).await {
                    Ok(0) => {
                        process.needs_drain = false;
                        stream_error = Some(ProviderError::RequestFailed(
                            "Claude CLI process terminated unexpectedly".to_string(),
                        ));
                        break;
                    }
                    Ok(_) => {
                        let trimmed = line.trim();
                        if trimmed.is_empty() {
                            continue;
                        }

                        if let Ok(parsed) = serde_json::from_str::<Value>(trimmed) {
                            match parsed.get("type").and_then(|t| t.as_str()) {
                                Some("stream_event") => {
                                    if let Some(event) = parsed.get("event") {
                                        match event.get("type").and_then(|t| t.as_str()) {
                                            Some("content_block_delta") => {
                                                if let Some(text) = event
                                                    .get("delta")
                                                    .filter(|d| {
                                                        d.get("type").and_then(|t| t.as_str())
                                                            == Some("text_delta")
                                                    })
                                                    .and_then(|d| d.get("text"))
                                                    .and_then(|t| t.as_str())
                                                {
                                                    if !text.is_empty() {
                                                        let mut partial_message = Message::new(
                                                            Role::Assistant,
                                                            stream_timestamp,
                                                            vec![MessageContent::text(text)],
                                                        );
                                                        partial_message.id =
                                                            Some(current_text_message_id.clone());
                                                        yield (Some(partial_message), None);
                                                    }
                                                }
                                            }
                                            Some("message_start") => {
                                                if let Some(usage_info) = event
                                                    .get("message")
                                                    .and_then(|m| m.get("usage"))
                                                {
                                                    let new = extract_usage_tokens(usage_info);
                                                    if let Some(i) = new.input_tokens {
                                                        accumulated_usage.input_tokens = Some(i);
                                                        accumulated_usage.cache_read_input_tokens =
                                                            new.cache_read_input_tokens;
                                                        accumulated_usage.cache_write_input_tokens =
                                                            new.cache_write_input_tokens;
                                                    }
                                                }
                                            }
                                            Some("message_delta") => {
                                                if let Some(usage_info) = event.get("usage") {
                                                    let new = extract_usage_tokens(usage_info);
                                                    if let Some(o) = new.output_tokens {
                                                        accumulated_usage.output_tokens = Some(o);
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                Some("result") => {
                                    process.needs_drain = false;
                                    if parsed
                                        .get("is_error")
                                        .and_then(Value::as_bool)
                                        .unwrap_or(false)
                                    {
                                        let subtype = parsed
                                            .get("subtype")
                                            .and_then(Value::as_str)
                                            .unwrap_or("error");
                                        let mut details = Vec::new();
                                        if let Some(error) =
                                            parsed.get("error").and_then(Value::as_str)
                                        {
                                            details.push(error);
                                        }
                                        if let Some(errors) =
                                            parsed.get("errors").and_then(Value::as_array)
                                        {
                                            details.extend(errors.iter().filter_map(Value::as_str));
                                        }
                                        if let Some(result) =
                                            parsed.get("result").and_then(Value::as_str)
                                        {
                                            details.push(result);
                                        }
                                        let details = details.join("; ");
                                        let message = match (subtype, details.is_empty()) {
                                            ("success", false) => details,
                                            (_, false) => format!("{subtype}: {details}"),
                                            _ => subtype.to_string(),
                                        };
                                        stream_error = Some(ProviderError::RequestFailed(format!(
                                            "Claude CLI error: {message}"
                                        )));
                                        break;
                                    }
                                    if let Some(usage_info) = parsed.get("usage") {
                                        let new = extract_usage_tokens(usage_info);
                                        let reports_own_cache = new.cache_read_input_tokens.is_some()
                                            || new.cache_write_input_tokens.is_some();
                                        let cache_read = new
                                            .cache_read_input_tokens
                                            .or(accumulated_usage.cache_read_input_tokens);
                                        let cache_write = new
                                            .cache_write_input_tokens
                                            .or(accumulated_usage.cache_write_input_tokens);
                                        // A result with raw input but no cache breakdown
                                        // inherits the streamed breakdown; fold it back in
                                        // so input stays inclusive of cache tokens.
                                        let output_tokens =
                                            new.output_tokens.or(accumulated_usage.output_tokens);
                                        accumulated_usage = if new.input_tokens.is_some()
                                            && !reports_own_cache
                                        {
                                            Usage::from_cache_exclusive_input(
                                                new.input_tokens,
                                                output_tokens,
                                                None,
                                                cache_read,
                                                cache_write,
                                            )
                                        } else {
                                            Usage::new(
                                                new.input_tokens.or(accumulated_usage.input_tokens),
                                                output_tokens,
                                                None,
                                            )
                                            .with_cache_tokens(cache_read, cache_write)
                                        };
                                    }
                                    break;
                                }
                                Some("error") => {
                                    process.needs_drain = false;
                                    stream_error = Some(error_from_event("Claude CLI", &parsed));
                                    break;
                                }
                                Some("control_request") => {
                                    if let Ok(IncomingControlRequest {
                                        request_id,
                                        request: IncomingRequestBody::CanUseTool { tool_name, input, tool_use_id },
                                    }) = serde_json::from_str::<IncomingControlRequest>(trimmed) {
                                        tracing::debug!(raw = %parsed, "can_use_tool control_request received");

                                        let (tx, rx) = oneshot::channel();
                                        pending_confirmations.lock().await.insert(request_id.clone(), tx);

                                        let action_msg = Message::assistant().with_action_required(
                                            request_id.clone(), tool_name, input.clone(), None,
                                        );
                                        yield (Some(action_msg), None);
                                        current_text_message_id = uuid::Uuid::new_v4().to_string();

                                        let confirmation = rx.await.unwrap_or(PermissionConfirmation {
                                            principal_type: PrincipalType::Tool,
                                            permission: Permission::Cancel,
                                        });
                                        pending_confirmations.lock().await.remove(&request_id);

                                        let perm_resp = match confirmation.permission {
                                            Permission::AlwaysAllow | Permission::AllowOnce => {
                                                PermissionResponse::Allow {
                                                    updated_input: input,
                                                    tool_use_id,
                                                }
                                            }
                                            _ => PermissionResponse::Deny {
                                                message: "User denied the tool call".to_string(),
                                            },
                                        };
                                        let resp = ControlResponse::success(request_id, perm_resp);
                                        let mut resp_str = serde_json::to_string(&resp).map_err(|e| {
                                            ProviderError::RequestFailed(format!("Failed to serialize permission response: {e}"))
                                        })?;
                                        tracing::debug!(json = %resp_str, "can_use_tool control_response sent");
                                        resp_str.push('\n');
                                        process.stdin.write_all(resp_str.as_bytes()).await.map_err(|e| {
                                            ProviderError::RequestFailed(format!("Failed to write permission response: {e}"))
                                        })?;
                                    }
                                }
                                Some("system") if process.log_model_update => {
                                    if let Some(resolved) = parsed.get("model").and_then(|m| m.as_str()) {
                                        tracing::debug!(
                                            from = %process.current_model,
                                            to = %resolved,
                                            "set_model resolved"
                                        );
                                    }
                                    process.log_model_update = false;
                                }
                                _ => {}
                            }
                        }
                    }
                    Err(e) => {
                        process.needs_drain = false;
                        stream_error = Some(ProviderError::RequestFailed(format!(
                            "Failed to read streaming output: {e}"
                        )));
                        break;
                    }
                }
            }

            if let Some(err) = stream_error {
                Err(err)?;
            }

            let provider_usage = ProviderUsage::new(model_name, accumulated_usage);
            yield (None, Some(provider_usage));
        }))
    }
}

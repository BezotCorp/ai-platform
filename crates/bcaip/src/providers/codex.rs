use super::base::ProviderDef;
use super::utils::filter_extensions_from_system_prompt;
use crate::config::{Config, ExtensionConfig};
use crate::config::{paths::Paths, search_path::SearchPaths};
use crate::subprocess::configure_subprocess;
use anyhow::{Result, anyhow};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bcaip_provider_types::base::{ConfigKey, MessageStream, Provider, ProviderMetadata};
use bcaip_provider_types::bcaip_mode::BcaipMode;
use bcaip_provider_types::conversations::{Message, MessageContent};
use bcaip_provider_types::conversations::{ProviderUsage, Usage};
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::model::ModelConfig;
use bcaip_provider_types::request_log::{LoggerHandleExt, start_log};
use bcaip_provider_types::thinking::ThinkingEffort;
use futures::future::BoxFuture;
use rmcp::model::{Role, Tool};
use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::{collections::HashMap, io::Write};
use tempfile::NamedTempFile;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
const CODEX_PROVIDER_NAME: &str = "codex";
pub const CODEX_DEFAULT_MODEL: &str = "gpt-5.2-codex";
pub const CODEX_KNOWN_MODELS: &[&str] = &[
    "gpt-5.2-codex",
    "gpt-5.2",
    "gpt-5.1-codex-max",
    "gpt-5.1-codex-mini",
];
pub const CODEX_DOC_URL: &str = "https://developers.openai.com/codex/cli";

/// Spawns the Codex CLI (`codex exec`) as a one-shot child process per turn.
/// Text prompt is piped via stdin (`-`), images are passed as temporary files
/// via the `-i` flag. Output is JSONL on stdout (`--json`), with events like
/// `item.completed`, `turn.completed`, and `error`.
#[derive(Debug, serde::Serialize)]
pub struct CodexProvider {
    command: PathBuf,
    #[serde(skip)]
    name: String,
    /// Whether to skip git repo check
    skip_git_check: bool,
    /// CLI config overrides for MCP servers
    mcp_config_overrides: Vec<String>,
    #[serde(skip)]
    mode_by_session: tokio::sync::RwLock<HashMap<String, BcaipMode>>,
}

impl CodexProvider {
    fn legacy_reasoning_effort() -> Option<ThinkingEffort> {
        Config::global()
            .get_param::<String>("CODEX_REASONING_EFFORT")
            .ok()
            .and_then(|effort| match effort.to_lowercase().as_str() {
                "none" => Some(ThinkingEffort::Off),
                "low" => Some(ThinkingEffort::Low),
                "medium" => Some(ThinkingEffort::Medium),
                "high" => Some(ThinkingEffort::High),
                "xhigh" => Some(ThinkingEffort::Max),
                _ => None,
            })
    }

    fn map_thinking_effort(_model_name: &str, effort: Option<ThinkingEffort>) -> Option<String> {
        use ThinkingEffort;
        match effort
            .or_else(Self::legacy_reasoning_effort)
            .unwrap_or(ThinkingEffort::High)
        {
            ThinkingEffort::Off => Some("none".to_string()),
            ThinkingEffort::Low => Some("low".to_string()),
            ThinkingEffort::Medium => Some("medium".to_string()),
            ThinkingEffort::High => Some("high".to_string()),
            ThinkingEffort::Max => Some("xhigh".to_string()),
        }
    }

    /// Apply permission flags based on BcaipMode
    fn apply_permission_flags(
        cmd: &mut Command,
        bcaip_mode: BcaipMode,
    ) -> Result<(), ProviderError> {
        match bcaip_mode {
            BcaipMode::Auto => {
                // --yolo is shorthand for --dangerously-bypass-approvals-and-sandbox
                cmd.arg("--yolo");
            }
            BcaipMode::SmartApprove => {
                // --full-auto applies workspace-write sandbox and approvals only on failure
                cmd.arg("--full-auto");
            }
            BcaipMode::Approve => {
                // Default codex behavior - interactive approvals
                // No special flags needed
            }
            BcaipMode::Chat => {
                // Read-only sandbox mode
                cmd.arg("--sandbox").arg("read-only");
            }
        }
        Ok(())
    }

    /// Execute codex CLI command
    async fn execute_command(
        &self,
        model: &ModelConfig,
        system: &str,
        messages: &[Message],
        _tools: &[Tool],
        bcaip_mode: BcaipMode,
    ) -> Result<Vec<String>, ProviderError> {
        // Single pass: text → prompt (stdin), images → temp files (-i flags)
        let image_dir = Paths::state_dir().join("codex/images");
        std::fs::create_dir_all(&image_dir).ok();
        let (prompt, temp_files) = prepare_input(system, messages, &image_dir)?;

        if std::env::var("BCAIP_CODEX_DEBUG").is_ok() {
            let reasoning_effort =
                Self::map_thinking_effort(&model.model_name, model.thinking_effort());
            println!("=== CODEX PROVIDER DEBUG ===");
            println!("Command: {:?}", self.command);
            println!("Model: {}", model.model_name);
            println!("Reasoning effort: {:?}", reasoning_effort);
            println!("Skip git check: {}", self.skip_git_check);
            println!("Prompt length: {} chars", prompt.len());
            println!("Prompt: {}", prompt);
            println!("Image files: {}", temp_files.len());
            println!("============================");
        }

        let mut cmd = Command::new(&self.command);
        configure_subprocess(&mut cmd);

        // Propagate extended PATH so the codex subprocess can find Node.js
        // and other dependencies (especially when launched from the desktop app
        // where the inherited PATH is limited).
        if let Ok(path) = SearchPaths::builder().with_npm().path() {
            cmd.env("PATH", path);
        }

        // Use 'exec' subcommand for non-interactive mode
        cmd.arg("exec");

        // Only pass model parameter if it's in the known models list
        // This allows users to set BCAIP_PROVIDER=codex without needing to specify a model
        if CODEX_KNOWN_MODELS.contains(&model.model_name.as_str()) {
            cmd.arg("-m").arg(&model.model_name);
        }

        if let Some(reasoning_effort) =
            Self::map_thinking_effort(&model.model_name, model.thinking_effort())
        {
            cmd.arg("-c")
                .arg(format!("model_reasoning_effort=\"{}\"", reasoning_effort));
        }

        for override_config in &self.mcp_config_overrides {
            cmd.arg("-c").arg(override_config);
        }

        // JSON output format for structured parsing
        cmd.arg("--json");

        Self::apply_permission_flags(&mut cmd, bcaip_mode)?;

        // Skip git repo check if configured
        if self.skip_git_check {
            cmd.arg("--skip-git-repo-check");
        }

        // Codex treats -i as supplementary context, not positionally interleaved with text
        for tmp in &temp_files {
            cmd.arg("-i").arg(tmp.path());
        }

        // Pass the prompt via stdin using '-' argument
        cmd.arg("-");

        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        let mut child = cmd.spawn().map_err(|e| {
            ProviderError::RequestFailed(format!(
                "Failed to spawn Codex CLI command '{:?}': {}. \
                Make sure the Codex CLI is installed (npm i -g @openai/codex) \
                and available in the configured search paths.",
                self.command, e
            ))
        })?;

        // Write prompt to stdin
        if let Some(mut stdin) = child.stdin.take() {
            use tokio::io::AsyncWriteExt;
            stdin.write_all(prompt.as_bytes()).await.map_err(|e| {
                ProviderError::RequestFailed(format!("Failed to write to stdin: {}", e))
            })?;
            // Close stdin to signal end of input
            drop(stdin);
        }

        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| ProviderError::RequestFailed("Failed to capture stdout".to_string()))?;

        // Drain stderr concurrently to prevent pipe buffer deadlock
        let stderr_handle = {
            let stderr = child.stderr.take();
            tokio::spawn(async move {
                let mut output = String::new();
                if let Some(mut stderr) = stderr {
                    use tokio::io::AsyncReadExt;
                    let _ = stderr.read_to_string(&mut output).await;
                }
                output
            })
        };

        let mut reader = BufReader::new(stdout);
        let mut lines = Vec::new();
        let mut line = String::new();

        loop {
            line.clear();
            match reader.read_line(&mut line).await {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        lines.push(trimmed.to_string());
                    }
                }
                Err(e) => {
                    return Err(ProviderError::RequestFailed(format!(
                        "Failed to read output: {}",
                        e
                    )));
                }
            }
        }

        let exit_status = child.wait().await.map_err(|e| {
            ProviderError::RequestFailed(format!("Failed to wait for command: {}", e))
        })?;

        // Allow the stderr task to finish
        let _ = stderr_handle.await;

        if !exit_status.success() && lines.is_empty() {
            return Err(ProviderError::RequestFailed(format!(
                "Codex command failed with exit code: {:?}",
                exit_status.code()
            )));
        }

        tracing::debug!("Codex CLI executed successfully, got {} lines", lines.len());

        Ok(lines)
    }

    /// Extract text content from an item.completed event (agent_message only, skip reasoning)
    fn extract_text_from_item(item: &serde_json::Value) -> Option<String> {
        let item_type = item.get("type").and_then(|t| t.as_str());
        if item_type == Some("agent_message") {
            item.get("text")
                .and_then(|t| t.as_str())
                .filter(|text| !text.trim().is_empty())
                .map(|s| s.to_string())
        } else {
            None
        }
    }

    /// Codex `input_tokens` already includes `cached_input_tokens`.
    fn extract_usage(usage_info: &serde_json::Value, usage: &mut Usage) {
        if usage.input_tokens.is_none() {
            usage.input_tokens = usage_info
                .get("input_tokens")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32);
        }
        if usage.output_tokens.is_none() {
            usage.output_tokens = usage_info
                .get("output_tokens")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32);
        }
        if usage.cache_read_input_tokens.is_none() {
            usage.cache_read_input_tokens = usage_info
                .get("cached_input_tokens")
                .and_then(|v| v.as_i64())
                .map(|v| v as i32);
        }
    }

    /// Extract error message from an error event
    fn extract_error(parsed: &serde_json::Value) -> Option<String> {
        parsed
            .get("message")
            .and_then(|m| m.as_str())
            .map(|s| s.to_string())
            .or_else(|| {
                parsed
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .map(|s| s.to_string())
            })
    }

    /// Extract text from legacy message formats
    fn extract_legacy_text(parsed: &serde_json::Value) -> Vec<String> {
        let mut texts = Vec::new();
        if let Some(content) = parsed.get("content").and_then(|c| c.as_array()) {
            for item in content {
                if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                    texts.push(text.to_string());
                }
            }
        }
        if let Some(text) = parsed.get("text").and_then(|t| t.as_str()) {
            texts.push(text.to_string());
        }
        if let Some(text) = parsed.get("result").and_then(|r| r.as_str()) {
            texts.push(text.to_string());
        }
        texts
    }

    /// Build fallback text from non-JSON lines
    fn build_fallback_text(lines: &[String]) -> Option<String> {
        let response_text: String = lines
            .iter()
            .filter(|line| {
                !line.starts_with('{')
                    || serde_json::from_str::<serde_json::Value>(line)
                        .map(|v| v.get("type").is_none())
                        .unwrap_or(true)
            })
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        if response_text.trim().is_empty() {
            None
        } else {
            Some(response_text)
        }
    }

    /// Parse newline-delimited JSON response from Codex CLI
    fn parse_response(&self, lines: &[String]) -> Result<(Message, Usage), ProviderError> {
        let mut all_text_content = Vec::new();
        let mut usage = Usage::default();
        let mut error_message: Option<String> = None;

        for line in lines {
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(line)
                && let Some(event_type) = parsed.get("type").and_then(|t| t.as_str())
            {
                match event_type {
                    "item.completed" => {
                        if let Some(item) = parsed.get("item")
                            && let Some(text) = Self::extract_text_from_item(item)
                        {
                            all_text_content.push(text);
                        }
                    }
                    "turn.completed" | "result" | "done" => {
                        if let Some(usage_info) = parsed.get("usage") {
                            Self::extract_usage(usage_info, &mut usage);
                        }
                        all_text_content.extend(Self::extract_legacy_text(&parsed));
                    }
                    "error" | "turn.failed" => {
                        error_message = Self::extract_error(&parsed);
                    }
                    "message" | "assistant" => {
                        all_text_content.extend(Self::extract_legacy_text(&parsed));
                    }
                    _ => {}
                }
            }
        }

        if let Some(err) = error_message
            && all_text_content.is_empty()
        {
            if err.contains("context window") || err.contains("context_length_exceeded") {
                return Err(ProviderError::ContextLengthExceeded(err));
            }
            if err.to_lowercase().contains("rate limit") {
                return Err(ProviderError::RateLimitExceeded {
                    details: err,
                    retry_delay: None,
                });
            }
            return Err(ProviderError::RequestFailed(format!(
                "Codex CLI error: {}",
                err
            )));
        }

        if all_text_content.is_empty()
            && let Some(fallback) = Self::build_fallback_text(lines)
        {
            all_text_content.push(fallback);
        }

        if let (Some(input), Some(output)) = (usage.input_tokens, usage.output_tokens) {
            usage.total_tokens = Some(input + output);
        }

        let combined_text = all_text_content.join("\n\n");
        if combined_text.is_empty() {
            return Err(ProviderError::RequestFailed(
                "Empty response from Codex CLI".to_string(),
            ));
        }

        let message = Message::new(
            Role::Assistant,
            chrono::Utc::now().timestamp(),
            vec![MessageContent::text(combined_text)],
        );

        Ok((message, usage))
    }
}

/// Builds the text prompt and extracts images to temp files in a single pass.
/// Text goes to the prompt string (piped via stdin); images become temp files
/// (passed via `-i` flags). Returns (prompt, temp_files).
fn prepare_input(
    system: &str,
    messages: &[Message],
    image_dir: &Path,
) -> Result<(String, Vec<NamedTempFile>), ProviderError> {
    let mut prompt = String::new();
    let mut temp_files = Vec::new();

    let filtered_system = filter_extensions_from_system_prompt(system);
    if !filtered_system.is_empty() {
        prompt.push_str(&filtered_system);
        prompt.push_str("\n\n");
    }

    for message in messages.iter().filter(|m| m.is_agent_visible()) {
        let role_prefix = match message.role {
            Role::User => "Human: ",
            Role::Assistant => "Assistant: ",
        };
        prompt.push_str(role_prefix);

        for content in &message.content {
            match content {
                MessageContent::Text(t) => {
                    prompt.push_str(&t.text);
                    prompt.push('\n');
                }
                MessageContent::Image(img) => {
                    let decoded = BASE64.decode(&img.data).map_err(|e| {
                        ProviderError::RequestFailed(format!("Failed to decode image: {}", e))
                    })?;
                    // Codex only supports png and jpeg:
                    // https://github.com/openai/codex/blob/aea7610c/codex-rs/utils/image/src/lib.rs#L162-L167
                    let ext = match img.mime_type.as_str() {
                        "image/png" => "png",
                        "image/jpeg" => "jpg",
                        _ => {
                            return Err(ProviderError::RequestFailed(format!(
                                "Unsupported image MIME type for Codex: {}",
                                img.mime_type
                            )));
                        }
                    };
                    let mut tmp = tempfile::Builder::new()
                        .suffix(&format!(".{}", ext))
                        .tempfile_in(image_dir)
                        .map_err(|e| {
                            ProviderError::RequestFailed(format!(
                                "Failed to create temp file: {}",
                                e
                            ))
                        })?;
                    tmp.write_all(&decoded).map_err(|e| {
                        ProviderError::RequestFailed(format!("Failed to write image: {}", e))
                    })?;
                    temp_files.push(tmp);
                }
                MessageContent::ToolRequest(req) => {
                    if let Ok(call) = &req.tool_call {
                        prompt.push_str(&format!("[tool_use: {} id={}]\n", call.name, req.id));
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
                        prompt.push_str(&format!("[tool_result id={}] {}\n", resp.id, text));
                    }
                }
                _ => {}
            }
        }
        prompt.push('\n');
    }

    prompt.push_str("Assistant: ");
    Ok((prompt, temp_files))
}

fn toml_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{0008}' => out.push_str("\\b"),
            '\u{000C}' => out.push_str("\\f"),
            c if c.is_control() => {
                // TOML \uXXXX for other control characters
                for unit in c.encode_utf16(&mut [0; 2]) {
                    out.push_str(&format!("\\u{:04X}", unit));
                }
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

// Codex CLI only supports inline `-c key=value` TOML overrides — no file-based
// config merging. Resolved secrets (from env_keys/keystore) in envs/headers end
// up in process argv, visible via `ps`. Claude Code avoids this by writing to a
// temp file with 0o600 permissions.
// Tracking: https://github.com/openai/codex/issues/2628
fn codex_mcp_config_overrides(extensions: &[ExtensionConfig]) -> Result<Vec<String>> {
    let mut overrides = Vec::new();
    for extension in extensions {
        match extension {
            ExtensionConfig::StreamableHttp {
                name,
                socket: Some(_),
                ..
            } => {
                return Err(anyhow!(
                    "Codex provider does not support socket-backed Streamable HTTP extension '{name}'; use a provider that preserves socket-backed MCP transport, or remove the socket setting only if remote HTTP is intended"
                ));
            }
            ExtensionConfig::StreamableHttp { uri, headers, .. } => {
                let key = extension.key();
                overrides.push(format!("mcp_servers.{}.url={}", key, toml_quote(uri)));
                if !headers.is_empty() {
                    let mut hkeys: Vec<_> = headers.keys().collect();
                    hkeys.sort();
                    let entries: Vec<_> = hkeys
                        .iter()
                        .map(|k| format!("{} = {}", toml_quote(k), toml_quote(&headers[*k])))
                        .collect();
                    overrides.push(format!(
                        "mcp_servers.{}.http_headers={{{}}}",
                        key,
                        entries.join(", ")
                    ));
                }
            }
            ExtensionConfig::Stdio {
                cmd, args, envs, ..
            } => {
                let key = extension.key();
                overrides.push(format!("mcp_servers.{}.command={}", key, toml_quote(cmd)));
                if !args.is_empty() {
                    let items: Vec<_> = args.iter().map(|a| toml_quote(a)).collect();
                    overrides.push(format!("mcp_servers.{}.args=[{}]", key, items.join(", ")));
                }
                let env_map = envs.get_env();
                if !env_map.is_empty() {
                    let mut ekeys: Vec<_> = env_map.keys().collect();
                    ekeys.sort();
                    let entries: Vec<_> = ekeys
                        .iter()
                        .map(|k| {
                            format!("{} = {}", toml_quote(k), toml_quote(&env_map[k.as_str()]))
                        })
                        .collect();
                    overrides.push(format!(
                        "mcp_servers.{}.env={{{}}}",
                        key,
                        entries.join(", ")
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(overrides)
}

impl bcaip_provider_types::base::ProviderDescriptor for CodexProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            CODEX_PROVIDER_NAME,
            "OpenAI Codex CLI",
            "[Deprecated: use chatgpt_codex or codex-acp instead] Execute OpenAI models via Codex CLI tool. Requires codex CLI installed.",
            CODEX_DEFAULT_MODEL,
            CODEX_KNOWN_MODELS.to_vec(),
            CODEX_DOC_URL,
            vec![
                ConfigKey::new("CODEX_COMMAND", true, false, Some("codex"), true),
                ConfigKey::new("CODEX_SKIP_GIT_CHECK", false, false, Some("false"), true),
            ],
        )
        .deprecated(Some("codex-acp"))
    }
}

impl ProviderDef for CodexProvider {
    type Provider = Self;

    fn from_env(
        extensions: Vec<ExtensionConfig>,
        _tls_config: Option<bcaip_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(async move {
            let config = Config::global();
            let command: String = config.get_codex_command().unwrap_or_default().into();
            let resolved_command = SearchPaths::builder().with_npm().resolve(command)?;

            // Get skip_git_check from config, default to false
            let skip_git_check = config
                .get_codex_skip_git_check()
                .map(|s| s.to_lowercase() == "true")
                .unwrap_or(false);

            let mut resolved = Vec::with_capacity(extensions.len());
            for ext in extensions {
                resolved.push(ext.resolve(config).await?);
            }

            Ok(Self {
                command: resolved_command,
                name: CODEX_PROVIDER_NAME.to_string(),
                skip_git_check,
                mcp_config_overrides: codex_mcp_config_overrides(&resolved)?,
                mode_by_session: tokio::sync::RwLock::new(HashMap::new()),
            })
        })
    }
}

#[async_trait]
impl Provider for CodexProvider {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn uses_local_session_naming(&self) -> bool {
        true
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let session_id = crate::session_context::current_session_id().unwrap_or_default();
        let bcaip_mode = {
            let map = self.mode_by_session.read().await;
            map.get(&session_id).copied().unwrap_or_default()
        };
        let lines = self
            .execute_command(model_config, system, messages, tools, bcaip_mode)
            .await?;

        let (message, usage) = self.parse_response(&lines)?;

        // Create a payload for debug tracing
        let payload = json!({
            "command": self.command,
            "model": model_config.model_name,
            "reasoning_effort": Self::map_thinking_effort(&model_config.model_name, model_config.thinking_effort()),
            "system_length": system.len(),
            "messages_count": messages.len()
        });

        let mut log = start_log(model_config, &payload)?;

        let response = json!({
            "lines": lines.len(),
            "usage": usage
        });

        log.write(&response, Some(&usage)).map_err(|e| {
            ProviderError::RequestFailed(format!("Failed to write request log: {}", e))
        })?;

        let provider_usage = ProviderUsage::new(model_config.model_name.clone(), usage);
        Ok(bcaip_provider_types::base::stream_from_single_message(
            message,
            provider_usage,
        ))
    }

    async fn update_mode(&self, session_id: &str, mode: BcaipMode) -> Result<(), ProviderError> {
        self.mode_by_session
            .write()
            .await
            .insert(session_id.to_string(), mode);
        Ok(())
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        Ok(CODEX_KNOWN_MODELS.iter().map(|s| s.to_string()).collect())
    }
}

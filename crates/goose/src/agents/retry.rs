use anyhow::Result;
use std::{process::Stdio, sync::Arc, time::Duration};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::{process::Command, sync::Mutex};
use tracing::{debug, info, warn};

use crate::agents::types::{
    DEFAULT_ON_FAILURE_TIMEOUT_SECONDS, DEFAULT_RETRY_TIMEOUT_SECONDS, RetryConfig, SuccessCheck,
};
use crate::config::Config;
use crate::tool_monitor::RepetitionInspector;
use crate::{agents::types::SessionConfig, subprocess::SubprocessExt};
use bcaip_provider_types::conversations::{Conversation, Message};
/// Result of a retry logic evaluation
#[derive(Debug, Clone, PartialEq)]
pub enum RetryResult {
    /// No retry configuration or session available, retry logic skipped
    Skipped,
    /// Maximum retry attempts reached, cannot retry further. Carries the
    /// user-facing failure message so the caller can yield and persist it.
    MaxAttemptsReached(Message),
    /// Success checks passed, no retry needed
    SuccessChecksPassed,
    /// Retry is needed and will be performed
    Retried,
}

/// Environment variable for configuring retry timeout globally
const GOOSE_RECIPE_RETRY_TIMEOUT_SECONDS: &str = "GOOSE_RECIPE_RETRY_TIMEOUT_SECONDS";

/// Environment variable for configuring on_failure timeout globally
const GOOSE_RECIPE_ON_FAILURE_TIMEOUT_SECONDS: &str = "GOOSE_RECIPE_ON_FAILURE_TIMEOUT_SECONDS";

const MAX_COMMAND_STDERR_BYTES: usize = 8 * 1024;

/// Manages retry state and operations for agent execution
#[derive(Debug)]
pub struct RetryManager {
    /// Current number of retry attempts
    attempts: Arc<Mutex<u32>>,
    /// Optional repetition inspector for reset operations
    repetition_inspector: Option<Arc<Mutex<Option<RepetitionInspector>>>>,
}

impl Default for RetryManager {
    fn default() -> Self {
        Self::new()
    }
}

impl RetryManager {
    /// Create a new retry manager
    pub fn new() -> Self {
        Self {
            attempts: Arc::new(Mutex::new(0)),
            repetition_inspector: None,
        }
    }

    /// Create a new retry manager with repetition inspector
    pub fn with_repetition_inspector(
        repetition_inspector: Arc<Mutex<Option<RepetitionInspector>>>,
    ) -> Self {
        Self {
            attempts: Arc::new(Mutex::new(0)),
            repetition_inspector: Some(repetition_inspector),
        }
    }

    /// Reset the retry attempts counter to 0
    pub async fn reset_attempts(&self) {
        let mut attempts = self.attempts.lock().await;
        *attempts = 0;

        // Reset repetition inspector if available
        if let Some(inspector) = &self.repetition_inspector
            && let Some(inspector) = inspector.lock().await.as_mut()
        {
            inspector.reset();
        }
    }

    /// Increment the retry attempts counter and return the new value
    pub async fn increment_attempts(&self) -> u32 {
        let mut attempts = self.attempts.lock().await;
        *attempts += 1;
        *attempts
    }

    /// Get the current retry attempts count
    pub async fn get_attempts(&self) -> u32 {
        *self.attempts.lock().await
    }

    async fn reset_status_for_retry(messages: &mut Conversation, initial_messages: &[Message]) {
        *messages = Conversation::new_unvalidated(initial_messages.to_vec());
        info!("Reset message history to initial state for retry");
    }

    pub async fn handle_retry_logic(
        &self,
        messages: &mut Conversation,
        session_config: &SessionConfig,
        initial_messages: &[Message],
    ) -> Result<RetryResult> {
        let Some(retry_config) = &session_config.retry_config else {
            return Ok(RetryResult::Skipped);
        };

        let success = execute_success_checks(&retry_config.checks, retry_config).await?;

        if success {
            info!("All success checks passed, no retry needed");
            return Ok(RetryResult::SuccessChecksPassed);
        }

        let current_attempts = self.get_attempts().await;
        if current_attempts >= retry_config.max_retries {
            let error_msg = Message::assistant().with_text(format!(
                "Maximum retry attempts ({}) exceeded. Unable to complete the task successfully.",
                retry_config.max_retries
            ));
            warn!(
                "Maximum retry attempts ({}) exceeded",
                retry_config.max_retries
            );
            #[cfg(feature = "telemetry")]
            crate::posthog::emit_error(
                "retry_max_exceeded",
                &format!("Max retries ({}) exceeded", retry_config.max_retries),
            );
            return Ok(RetryResult::MaxAttemptsReached(error_msg));
        }

        if let Some(on_failure_cmd) = &retry_config.on_failure {
            info!("Executing on_failure command: {}", on_failure_cmd);
            execute_on_failure_command(on_failure_cmd, retry_config).await?;
        }

        Self::reset_status_for_retry(messages, initial_messages).await;

        let new_attempts = self.increment_attempts().await;
        info!("Incrementing retry attempts to {}", new_attempts);

        Ok(RetryResult::Retried)
    }
}

/// Get the configured timeout duration for retry operations
/// retry_config.timeout_seconds -> env var -> default
fn get_retry_timeout(retry_config: &RetryConfig) -> Duration {
    let timeout_seconds = retry_config
        .timeout_seconds
        .or_else(|| {
            let config = Config::global();
            config.get_param(GOOSE_RECIPE_RETRY_TIMEOUT_SECONDS).ok()
        })
        .unwrap_or(DEFAULT_RETRY_TIMEOUT_SECONDS);

    Duration::from_secs(timeout_seconds)
}

/// Get the configured timeout duration for on_failure operations
/// retry_config.on_failure_timeout_seconds -> env var -> default
fn get_on_failure_timeout(retry_config: &RetryConfig) -> Duration {
    let timeout_seconds = retry_config
        .on_failure_timeout_seconds
        .or_else(|| {
            let config = Config::global();
            config
                .get_param(GOOSE_RECIPE_ON_FAILURE_TIMEOUT_SECONDS)
                .ok()
        })
        .unwrap_or(DEFAULT_ON_FAILURE_TIMEOUT_SECONDS);

    Duration::from_secs(timeout_seconds)
}

/// Execute all success checks and return true if all pass
pub async fn execute_success_checks(
    checks: &[SuccessCheck],
    retry_config: &RetryConfig,
) -> Result<bool> {
    execute_success_checks_with_timeout(checks, get_retry_timeout(retry_config)).await
}

pub async fn execute_success_checks_with_timeout(
    checks: &[SuccessCheck],
    timeout: Duration,
) -> Result<bool> {
    for check in checks {
        match check {
            SuccessCheck::Shell { command } => {
                let result = execute_shell_command(command, timeout).await?;
                if !result.status.success() {
                    warn!(
                        "Success check failed: command '{}' exited with status {}, stderr: {}",
                        command,
                        result.status,
                        String::from_utf8_lossy(&result.stderr)
                    );
                    return Ok(false);
                }
                info!(
                    "Success check passed: command '{}' completed successfully",
                    command
                );
            }
        }
    }
    Ok(true)
}

/// Execute a shell command with cross-platform compatibility and mandatory timeout
pub async fn execute_shell_command(
    command: &str,
    timeout: std::time::Duration,
) -> Result<std::process::Output> {
    debug!(
        "Executing shell command with timeout {:?}: {}",
        timeout, command
    );

    let future = async {
        let mut cmd = if cfg!(target_os = "windows") {
            let mut cmd = Command::new("cmd");
            cmd.args(["/C", command]);
            cmd.env("GOOSE_TERMINAL", "1");
            cmd.env("AGENT", "goose");
            cmd
        } else {
            let mut cmd = Command::new("sh");
            cmd.args(["-c", command]);
            cmd.env("GOOSE_TERMINAL", "1");
            cmd.env("AGENT", "goose");
            cmd
        };

        cmd.set_no_window();

        let mut child = cmd
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .stdin(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let stderr = child
            .stderr
            .take()
            .expect("stderr was configured to be piped");
        let (status, stderr) = tokio::try_join!(child.wait(), capture_tail(stderr))?;
        let output = std::process::Output {
            status,
            stdout: Vec::new(),
            stderr,
        };

        debug!(
            "Shell command completed with status: {}, stderr: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );

        Ok(output)
    };

    match tokio::time::timeout(timeout, future).await {
        Ok(result) => result,
        Err(_) => {
            let error_msg = format!("Shell command timed out after {:?}: {}", timeout, command);
            warn!("{}", error_msg);
            Err(anyhow::anyhow!("{}", error_msg))
        }
    }
}

async fn capture_tail<R>(mut reader: R) -> std::io::Result<Vec<u8>>
where
    R: AsyncRead + Unpin,
{
    let mut tail = Vec::with_capacity(MAX_COMMAND_STDERR_BYTES);
    let mut chunk = [0; 4096];

    loop {
        let count = reader.read(&mut chunk).await?;
        if count == 0 {
            return Ok(tail);
        }

        if count >= MAX_COMMAND_STDERR_BYTES {
            tail.clear();
            tail.extend_from_slice(&chunk[count - MAX_COMMAND_STDERR_BYTES..count]);
            continue;
        }

        let overflow = tail
            .len()
            .saturating_add(count)
            .saturating_sub(MAX_COMMAND_STDERR_BYTES);
        if overflow > 0 {
            tail.drain(..overflow);
        }
        tail.extend_from_slice(&chunk[..count]);
    }
}

/// Execute an on_failure command and return an error if it fails
pub async fn execute_on_failure_command(command: &str, retry_config: &RetryConfig) -> Result<()> {
    execute_on_failure_command_with_timeout(command, get_on_failure_timeout(retry_config)).await
}

pub async fn execute_on_failure_command_with_timeout(
    command: &str,
    timeout: Duration,
) -> Result<()> {
    info!(
        "Executing on_failure command with timeout {:?}: {}",
        timeout, command
    );

    let output = match execute_shell_command(command, timeout).await {
        Ok(output) => output,
        Err(e) => {
            if e.to_string().contains("timed out") {
                let error_msg = format!(
                    "On_failure command timed out after {:?}: {}",
                    timeout, command
                );
                warn!("{}", error_msg);
                return Err(anyhow::anyhow!(error_msg));
            } else {
                warn!("On_failure command execution error: {}", e);
                return Err(e);
            }
        }
    };

    if !output.status.success() {
        let error_msg = format!(
            "On_failure command failed: command '{}' exited with status {}, stderr: {}",
            command,
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        warn!("{}", error_msg);
        return Err(anyhow::anyhow!(error_msg));
    } else {
        info!("On_failure command completed successfully: {}", command);
    }

    Ok(())
}

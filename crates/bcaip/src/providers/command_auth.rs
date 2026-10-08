use crate::subprocess::configure_subprocess;
use anyhow::Result;
use async_trait::async_trait;
use bcaip_providers::api_client::AuthProvider;
use bcaip_providers::declarative::AuthConfig;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
const DEFAULT_AUTH_COMMAND_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_STDERR_BYTES: usize = 500;

struct CachedCredential {
    token: String,
    fetched_at: Instant,
}

/// Fetches a credential by running a user-configured command and caches the
/// result for `refresh_interval` before re-running it. Runtime counterpart of
/// `DeclarativeProviderConfig::auth`, for custom providers whose credentials
/// are short-lived and issued by an external script rather than a static key.
pub struct CommandAuthProvider {
    command: String,
    args: Vec<String>,
    refresh_interval: Duration,
    timeout: Duration,
    /// Working directory for the command, and the base a relative `command`
    /// resolves against. Captured once at construction (defaults to bcaip's
    /// current directory at that point), not re-read per invocation.
    cwd: PathBuf,
    header_name: String,
    header_value_prefix: String,
    cached: Arc<RwLock<Option<CachedCredential>>>,
}

impl CommandAuthProvider {
    pub fn new(
        auth_config: &AuthConfig,
        header_name: impl Into<String>,
        header_value_prefix: impl Into<String>,
    ) -> Self {
        // Made absolute up front: `cwd` doubles as both `current_dir` for the
        // spawned process and the join base in `resolve_program`, and a
        // relative value would otherwise be applied twice (once by us, once
        // by the OS resolving a relative program path against the process's
        // now-`current_dir`-ed working directory).
        let cwd = auth_config
            .cwd
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        let cwd = std::env::current_dir()
            .map(|base| {
                if cwd.is_absolute() {
                    cwd.clone()
                } else {
                    base.join(&cwd)
                }
            })
            .unwrap_or(cwd);
        Self {
            command: auth_config.command.clone(),
            args: auth_config.args.clone(),
            refresh_interval: Duration::from_secs(auth_config.refresh_interval),
            timeout: auth_config
                .timeout_seconds
                .map(Duration::from_secs)
                .unwrap_or(DEFAULT_AUTH_COMMAND_TIMEOUT),
            cwd,
            header_name: header_name.into(),
            header_value_prefix: header_value_prefix.into(),
            cached: Arc::new(RwLock::new(None)),
        }
    }

    async fn fetch_token(&self) -> Result<String> {
        // Spawned directly, never through a shell, so `args` is never
        // the same user configures bcaip and writes the script.
        // same user configures bcaip and writes the script.
        let program = resolve_program(&self.command, &self.cwd);
        let mut command = tokio::process::Command::new(&program);
        command
            .args(&self.args)
            .current_dir(&self.cwd)
            // No stdin, so a script that unexpectedly tries to prompt fails
            // fast instead of hanging on the parent's stdin. `kill_on_drop`
            // is a fallback for exit paths other than the timeout below,
            // which kills the whole process group explicitly — the direct
            // child alone wouldn't take a shell pipeline's descendants with
            // it, since `configure_subprocess` puts the child in its own
            // new process group.
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        configure_subprocess(&mut command);

        let child = command
            .spawn()
            .map_err(|e| anyhow::anyhow!("failed to run auth command '{}': {}", self.command, e))?;
        #[cfg(unix)]
        let pid = child.id();

        let output = match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
            Ok(result) => result.map_err(|e| {
                anyhow::anyhow!("failed to run auth command '{}': {}", self.command, e)
            })?,
            Err(_) => {
                #[cfg(unix)]
                if let Some(pid) = pid
                    && let Some(pid) = rustix::process::Pid::from_raw(pid as i32)
                {
                    let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
                }
                anyhow::bail!(
                    "auth command '{}' timed out after {:?}",
                    self.command,
                    self.timeout
                );
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!(
                "auth command '{}' exited with {}: {}",
                self.command,
                output.status,
                truncate(&stderr, MAX_STDERR_BYTES)
            );
        }

        let token = String::from_utf8(output.stdout)
            .map_err(|_| {
                anyhow::anyhow!(
                    "auth command '{}' wrote non-UTF-8 data to stdout",
                    self.command
                )
            })?
            .trim()
            .to_string();
        if token.is_empty() {
            anyhow::bail!(
                "auth command '{}' produced an empty credential",
                self.command
            );
        }

        Ok(token)
    }

    /// `refresh_interval: 0` means "never expire proactively" — matching
    /// Codex's `refresh_interval_ms: 0` convention — so the credential is
    /// only refreshed reactively, via `refresh_credentials()` after an auth
    /// failure, not on a TTL.
    fn is_fresh(&self, cached: &CachedCredential) -> bool {
        self.refresh_interval.is_zero() || cached.fetched_at.elapsed() < self.refresh_interval
    }
}

/// A bare command name (no path separator) is left alone for `PATH` lookup;
/// an absolute path is used as-is; a relative path is resolved against `cwd`.
fn resolve_program(command: &str, cwd: &Path) -> PathBuf {
    let path = Path::new(command);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    if path.components().count() > 1 {
        return cwd.join(path);
    }
    PathBuf::from(command)
}

fn truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.get(..end).unwrap_or(s)
}

#[async_trait]
impl AuthProvider for CommandAuthProvider {
    async fn get_auth_header(&self) -> Result<(String, String)> {
        // Try read lock first for better concurrency
        if let Some(cached) = self.cached.read().await.as_ref()
            && self.is_fresh(cached)
        {
            return Ok((
                self.header_name.clone(),
                format!("{}{}", self.header_value_prefix, cached.token),
            ));
        }

        // Take write lock only if needed
        let mut guard = self.cached.write().await;

        // Double-check freshness after acquiring write lock
        if let Some(cached) = guard.as_ref()
            && self.is_fresh(cached)
        {
            return Ok((
                self.header_name.clone(),
                format!("{}{}", self.header_value_prefix, cached.token),
            ));
        }

        // Get a new token. No fallback to a stale cached token on failure:
        // that would surface as a confusing downstream 401 instead of a
        // clear "the refresh command failed" error.
        let token = self.fetch_token().await?;
        *guard = Some(CachedCredential {
            token: token.clone(),
            fetched_at: Instant::now(),
        });

        Ok((
            self.header_name.clone(),
            format!("{}{}", self.header_value_prefix, token),
        ))
    }

    async fn refresh_credentials(&self) -> Result<()> {
        *self.cached.write().await = None;
        Ok(())
    }
}

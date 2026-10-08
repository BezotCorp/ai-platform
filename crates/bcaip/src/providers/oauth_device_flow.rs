//! Shared OAuth 2.0 Device Authorization Grant (RFC 8628) helper.
//!
//! Used by providers that authenticate via device-code flow (kimicode,
//! githubcopilot). Handles the authorization request, user-interaction UI,
//! polling loop with RFC 8628 `authorization_pending` / `slow_down` semantics,
//! and optional `refresh_token` grant (RFC 6749 §6).

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Duration, Utc};
use reqwest::{Client, header::HeaderMap};
use serde::{Deserialize, Serialize};

tokio::task_local! {
    /// When set, called instead of the default CLI announce when a device code
    /// is obtained. Args: (user_code, verification_uri, expires_in_secs).
    /// Set by the ACP server to forward the code to the desktop UI.
    static DEVICE_CODE_ANNOUNCE: Box<dyn Fn(String, String, u64) + Send + Sync>;
}

pub async fn with_device_code_announce<F, T>(
    announce: Box<dyn Fn(String, String, u64) + Send + Sync>,
    fut: F,
) -> T
where
    F: std::future::Future<Output = T>,
{
    DEVICE_CODE_ANNOUNCE.scope(announce, fut).await
}

/// Fallback poll interval when the server omits `interval` (RFC 8628 §3.2).
const DEFAULT_POLL_INTERVAL_SECS: u64 = 5;

/// Fallback device-code window when the server omits `expires_in` (RFC 8628 §3.2).
const DEFAULT_DEVICE_CODE_LIFETIME_SECS: u64 = 300;

/// Extra seconds added to the poll interval after an RFC 8628 `slow_down`.
const SLOW_DOWN_BACKOFF_SECS: u64 = 5;

/// How a provider expects the device-authorization and token request bodies to
/// be encoded. RFC 8628 §3.1 specifies `application/x-www-form-urlencoded`, but
/// GitHub accepts JSON when `Accept: application/json` is set.
#[derive(Debug, Clone, Copy)]
pub enum RequestEncoding {
    Form,
    Json,
}

/// Connection details for a provider's device flow.
#[derive(Debug, Clone)]
pub struct DeviceFlowConfig<'a> {
    /// `device_authorization_endpoint` (RFC 8628 §3.1).
    /// `None` when only the refresh grant is needed.
    pub device_auth_url: Option<&'a str>,
    /// `token_endpoint` used for both device-code polling and refresh grants.
    pub token_url: &'a str,
    /// Public OAuth client identifier.
    pub client_id: &'a str,
    /// Space-separated scope string, or `None` to omit the parameter.
    pub scopes: Option<&'a str>,
    /// Provider-specific headers (user-agent, platform markers, `Accept`, etc.).
    pub extra_headers: HeaderMap,
    /// Body encoding for device-auth, polling, and refresh requests.
    pub encoding: RequestEncoding,
}

/// Fields returned by `/device_authorization` (RFC 8628 §3.2).
#[derive(Debug, Clone, Deserialize)]
pub struct DeviceCodeResponse {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri: String,
    /// Pre-populated URI with user_code embedded, used when the provider
    /// supports it (e.g. Kimi). Fall back to `verification_uri` otherwise.
    pub verification_uri_complete: Option<String>,
    pub interval: Option<u64>,
    pub expires_in: Option<u64>,
}

impl DeviceCodeResponse {
    /// URI the user should visit. Prefers the `_complete` form when present.
    pub fn verification_url(&self) -> &str {
        self.verification_uri_complete
            .as_deref()
            .unwrap_or(&self.verification_uri)
    }
}

/// Access + optional refresh credentials from a device-code exchange.
#[derive(Debug, Clone)]
pub struct DeviceFlowTokens {
    pub access_token: String,
    /// Some providers (GitHub Copilot) do not issue a refresh token.
    pub refresh_token: Option<String>,
    /// Derived from `expires_in` on the token response. `None` when the server
    /// omits it (RFC 6749 §5.1 permits that).
    pub expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, thiserror::Error)]
#[error("token refresh failed ({status}): {body}")]
pub struct DeviceFlowTokenRefreshError {
    pub status: reqwest::StatusCode,
    pub error: Option<String>,
    body: String,
}

// ── Public entry points ──────────────────────────────────────────────────────

/// Request a device code from the authorization server.
pub async fn request_device_code(
    client: &Client,
    cfg: &DeviceFlowConfig<'_>,
) -> Result<DeviceCodeResponse> {
    #[derive(Serialize)]
    struct DeviceAuthReq<'a> {
        client_id: &'a str,
        #[serde(skip_serializing_if = "Option::is_none")]
        scope: Option<&'a str>,
    }

    let body = DeviceAuthReq {
        client_id: cfg.client_id,
        scope: cfg.scopes,
    };

    let url = cfg
        .device_auth_url
        .ok_or_else(|| anyhow!("device_auth_url is required for device code request"))?;
    send_request(client, cfg, url, &body)
        .await
        .context("failed to request device authorization")?
        .error_for_status()
        .context("device authorization request failed")?
        .json::<DeviceCodeResponse>()
        .await
        .context("failed to parse device authorization response")
}

/// Poll the token endpoint until the user authorizes (or the device code expires).
/// Implements RFC 8628 §3.5 — handles `authorization_pending` and `slow_down`.
pub async fn poll_for_tokens(
    client: &Client,
    cfg: &DeviceFlowConfig<'_>,
    device_code: &str,
    interval_secs: u64,
    expires_in_secs: u64,
) -> Result<DeviceFlowTokens> {
    #[derive(Serialize)]
    struct PollReq<'a> {
        client_id: &'a str,
        device_code: &'a str,
        grant_type: &'static str,
    }

    let req = PollReq {
        client_id: cfg.client_id,
        device_code,
        grant_type: "urn:ietf:params:oauth:grant-type:device_code",
    };

    let deadline = tokio::time::Instant::now() + tokio::time::Duration::from_secs(expires_in_secs);
    let mut effective_interval = interval_secs;

    loop {
        if tokio::time::Instant::now() >= deadline {
            return Err(anyhow!("timed out waiting for user authorization"));
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(effective_interval)).await;

        let response = send_request(client, cfg, cfg.token_url, &req)
            .await
            .context("failed to poll for token")?;

        // RFC 8628 §3.5 returns pending/slow_down as 4xx with a JSON error
        // payload, so don't `error_for_status()` before parsing. If the body
        // is unparseable AND the status is non-2xx, surface the HTTP status.
        match parse_token_response(response).await? {
            TokenPollOutcome::Issued(tokens) => return Ok(tokens),
            TokenPollOutcome::Pending => {
                tracing::debug!("authorization pending, continuing to poll");
            }
            TokenPollOutcome::SlowDown => {
                tracing::debug!("slow_down received, increasing poll interval");
                effective_interval += SLOW_DOWN_BACKOFF_SECS;
            }
            TokenPollOutcome::Failed(err) => {
                return Err(anyhow!("authorization failed: {}", err));
            }
        }
    }
}

/// High-level flow: request a device code, print user-facing instructions,
/// open the browser, and poll until tokens are issued.
pub async fn run_device_flow(
    client: &Client,
    cfg: &DeviceFlowConfig<'_>,
) -> Result<DeviceFlowTokens> {
    let device = request_device_code(client, cfg).await?;
    announce_user_action(&device);

    let interval = device.interval.unwrap_or(DEFAULT_POLL_INTERVAL_SECS);
    let expires_in = device
        .expires_in
        .unwrap_or(DEFAULT_DEVICE_CODE_LIFETIME_SECS);

    poll_for_tokens(client, cfg, &device.device_code, interval, expires_in).await
}

/// Exchange a refresh token for a new access token (RFC 6749 §6).
pub async fn refresh_device_flow_token(
    client: &Client,
    cfg: &DeviceFlowConfig<'_>,
    refresh_token: &str,
) -> Result<DeviceFlowTokens> {
    #[derive(Serialize)]
    struct RefreshReq<'a> {
        client_id: &'a str,
        grant_type: &'static str,
        refresh_token: &'a str,
    }

    let req = RefreshReq {
        client_id: cfg.client_id,
        grant_type: "refresh_token",
        refresh_token,
    };

    let response = send_request(client, cfg, cfg.token_url, &req)
        .await
        .context("failed to refresh token")?;
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .context("failed to read token refresh response")?;
    let raw = serde_json::from_slice::<TokenResponseBody>(&bytes);

    if !status.is_success() {
        return Err(anyhow::Error::new(DeviceFlowTokenRefreshError {
            status,
            error: raw.ok().and_then(|body| body.error),
            body: String::from_utf8_lossy(&bytes).into_owned(),
        }));
    }

    let raw = raw.context("failed to parse token refresh response")?;

    let access_token = raw
        .access_token
        .ok_or_else(|| anyhow!("refresh response missing access_token"))?;
    Ok(DeviceFlowTokens {
        access_token,
        refresh_token: raw.refresh_token,
        expires_at: raw
            .expires_in
            .map(|secs| Utc::now() + Duration::seconds(secs)),
    })
}

// ── Internals ────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponseBody {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
}

enum TokenPollOutcome {
    Issued(DeviceFlowTokens),
    Pending,
    SlowDown,
    Failed(String),
}

async fn parse_token_response(response: reqwest::Response) -> Result<TokenPollOutcome> {
    let status = response.status();
    let bytes = response
        .bytes()
        .await
        .context("failed to read token poll response")?;

    let body: TokenResponseBody = match serde_json::from_slice(&bytes) {
        Ok(p) => p,
        Err(e) => {
            if !status.is_success() {
                return Err(anyhow!(
                    "token poll HTTP {}: {}",
                    status,
                    String::from_utf8_lossy(&bytes)
                ));
            }
            return Err(anyhow::Error::new(e).context("failed to parse token poll response"));
        }
    };

    if let Some(access_token) = body.access_token {
        return Ok(TokenPollOutcome::Issued(DeviceFlowTokens {
            access_token,
            refresh_token: body.refresh_token,
            expires_at: body
                .expires_in
                .map(|secs| Utc::now() + Duration::seconds(secs)),
        }));
    }

    Ok(match body.error.as_deref() {
        Some("authorization_pending") => TokenPollOutcome::Pending,
        Some("slow_down") => TokenPollOutcome::SlowDown,
        Some(err) => TokenPollOutcome::Failed(err.to_string()),
        None => TokenPollOutcome::Failed(
            "unexpected token response: no access_token and no error code".to_string(),
        ),
    })
}

async fn send_request<T: Serialize + ?Sized>(
    client: &Client,
    cfg: &DeviceFlowConfig<'_>,
    url: &str,
    body: &T,
) -> reqwest::Result<reqwest::Response> {
    let builder = client
        .post(url)
        .timeout(std::time::Duration::from_secs(
            bcaip_providers::api_client::DEFAULT_PROVIDER_TIMEOUT_SECS,
        ))
        .headers(cfg.extra_headers.clone());
    let builder = match cfg.encoding {
        RequestEncoding::Form => builder.form(body),
        RequestEncoding::Json => builder.json(body),
    };
    builder.send().await
}

fn announce_user_action(device: &DeviceCodeResponse) {
    let verify_url = device.verification_url().to_string();

    if DEVICE_CODE_ANNOUNCE
        .try_with(|f| {
            let expires_in = device
                .expires_in
                .unwrap_or(DEFAULT_DEVICE_CODE_LIFETIME_SECS);
            f(device.user_code.clone(), verify_url.clone(), expires_in)
        })
        .is_ok()
    {
        return;
    }

    let copied = arboard::Clipboard::new()
        .ok()
        .and_then(|mut cb| cb.set_text(&device.user_code).ok())
        .is_some();
    if let Err(e) = webbrowser::open(&verify_url) {
        tracing::warn!("Failed to open browser: {}", e);
    }
    // stderr keeps stdout clean for CLI workflows parsing provider output.
    let clipboard_hint = if copied { " (copied to clipboard)" } else { "" };
    eprintln!(
        "Please visit {} and enter code {}{}",
        verify_url, device.user_code, clipboard_hint
    );
}

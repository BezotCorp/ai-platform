use std::time::Duration;
use thiserror::Error;

use crate::request_log::LogError;

#[derive(Error, Debug, Clone, PartialEq)]
pub enum ProviderError {
    #[error("Provider is not configured")]
    NotConfigured,

    #[error("Authentication error: {0}")]
    Authentication(String),

    #[error("Context length exceeded: {0}")]
    ContextLengthExceeded(String),

    #[error("Rate limit exceeded: {details}")]
    RateLimitExceeded {
        details: String,
        retry_delay: Option<Duration>,
    },

    #[error("Server error: {0}")]
    ServerError(String),

    #[error("Network error: {0}")]
    NetworkError(String),

    #[error("Request failed: {0}")]
    RequestFailed(String),

    /// Bad input rather than an operational failure: retrying is pointless, but
    /// a different value may succeed.
    #[error("Invalid value: {0}")]
    InvalidValue(String),

    #[error("Execution error: {0}")]
    ExecutionError(String),

    #[error("Usage data error: {0}")]
    UsageError(String),

    #[error("Unsupported operation: {0}")]
    NotImplemented(String),

    #[error("Endpoint not found (404): {0}")]
    EndpointNotFound(String),

    #[error("Credits exhausted: {details}")]
    CreditsExhausted {
        details: String,
        top_up_url: Option<String>,
    },

    #[error("Provider refused request: {details}")]
    Refusal {
        details: String,
        category: Option<String>,
    },
}

impl ProviderError {
    pub fn stream_decode_error(error: impl std::fmt::Display) -> Self {
        ProviderError::NetworkError(format!("Stream decode error: {error}"))
    }

    pub fn telemetry_type(&self) -> &'static str {
        match self {
            ProviderError::NotConfigured => "not_configured",
            ProviderError::Authentication(_) => "auth",
            ProviderError::ContextLengthExceeded(_) => "context_length",
            ProviderError::RateLimitExceeded { .. } => "rate_limit",
            ProviderError::ServerError(_) => "server",
            ProviderError::NetworkError(_) => "network",
            ProviderError::RequestFailed(_) => "request",
            ProviderError::InvalidValue(_) => "invalid_value",
            ProviderError::ExecutionError(_) => "execution",
            ProviderError::UsageError(_) => "usage",
            ProviderError::NotImplemented(_) => "not_implemented",
            ProviderError::EndpointNotFound(_) => "endpoint_not_found",
            ProviderError::CreditsExhausted { .. } => "credits_exhausted",
            ProviderError::Refusal { .. } => "refusal",
        }
    }

    pub fn is_endpoint_not_found(&self) -> bool {
        matches!(self, ProviderError::EndpointNotFound(_))
    }

    /// Recover a typed `ProviderError` from a streaming decode error, falling
    /// back to a retryable stream decode error for errors that did not
    /// originate as one.
    pub fn from_stream_error(error: anyhow::Error) -> Self {
        error
            .downcast()
            .unwrap_or_else(ProviderError::stream_decode_error)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn is_connect_error(error: &reqwest::Error) -> bool {
    error.is_connect()
}

// reqwest's wasm client does not expose is_connect
#[cfg(target_arch = "wasm32")]
fn is_connect_error(_error: &reqwest::Error) -> bool {
    false
}

fn is_network_error(err: &reqwest::Error) -> bool {
    is_connect_error(err) || err.is_timeout() || (err.status().is_none() && err.is_request())
}

fn sanitized_reqwest_url(error: &reqwest::Error) -> Option<String> {
    let mut url = error.url()?.clone();
    let _ = url.set_password(None);
    let _ = url.set_username("");
    url.set_query(None);
    url.set_fragment(None);
    Some(url.to_string())
}

fn reqwest_error_category(error: &reqwest::Error) -> &'static str {
    if error.is_builder() {
        "Request builder error"
    } else if error.is_redirect() {
        "Redirect error"
    } else if error.is_status() {
        "HTTP status error"
    } else if error.is_body() {
        "Request body error"
    } else if error.is_decode() {
        "Response decode error"
    } else {
        "Request error"
    }
}

fn provider_error_from_reqwest(error: &reqwest::Error) -> ProviderError {
    if is_network_error(error) {
        let msg = if error.is_timeout() {
            "Request timed out — check your network connection and try again.".to_string()
        } else if is_connect_error(error) {
            if let Some(url) = error.url() {
                if let Some(host) = url.host_str() {
                    let port_info = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
                    format!(
                        "Could not connect to {}{} — check your network connection and try again.",
                        host, port_info
                    )
                } else {
                    "Could not connect to the provider — check your network connection and try again.".to_string()
                }
            } else {
                "Could not connect to the provider — check your network connection and try again."
                    .to_string()
            }
        } else {
            "Network error — check your network connection and try again.".to_string()
        };
        return ProviderError::NetworkError(msg);
    }

    let mut details = Vec::new();
    if let Some(status) = error.status() {
        details.push(format!("status: {}", status));
    }
    if let Some(url) = sanitized_reqwest_url(error) {
        details.push(format!("url: {url}"));
    }

    let category = reqwest_error_category(error);
    let msg = if details.is_empty() {
        category.to_string()
    } else {
        format!("{category} ({})", details.join(", "))
    };
    ProviderError::RequestFailed(msg)
}

impl From<anyhow::Error> for ProviderError {
    fn from(error: anyhow::Error) -> Self {
        if let Some(provider_error) = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<ProviderError>())
        {
            return provider_error.clone();
        }
        if let Some(reqwest_err) = error
            .chain()
            .find_map(|cause| cause.downcast_ref::<reqwest::Error>())
        {
            return provider_error_from_reqwest(reqwest_err);
        }
        if error.chain().any(|cause| {
            cause
                .downcast_ref::<tokio::time::error::Elapsed>()
                .is_some()
        }) {
            return ProviderError::NetworkError(
                "Request timed out — check your network connection and try again.".to_string(),
            );
        }
        ProviderError::ExecutionError(error.to_string())
    }
}

impl From<reqwest::Error> for ProviderError {
    fn from(error: reqwest::Error) -> Self {
        provider_error_from_reqwest(&error)
    }
}

impl From<LogError> for ProviderError {
    fn from(value: LogError) -> Self {
        ProviderError::ExecutionError(value.to_string())
    }
}

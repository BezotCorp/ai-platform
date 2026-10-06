use crate::utils::safe_truncate;
use bcaip_provider_types::conversations::{Message, MessageContent};
use bcaip_provider_types::conversations::{ProviderUsage, Usage};
use bcaip_provider_types::errors::ProviderError;
use rmcp::model::Role;
use serde_json::Value;
pub(crate) fn extract_usage_tokens(usage_info: &Value) -> Usage {
    let get = |key: &str| {
        usage_info
            .get(key)
            .and_then(|v| v.as_i64())
            .and_then(|v| i32::try_from(v).ok())
    };
    Usage::from_cache_exclusive_input(
        get("input_tokens"),
        get("output_tokens"),
        get("total_tokens"),
        get("cache_read_input_tokens"),
        get("cache_creation_input_tokens"),
    )
}

pub(crate) fn error_from_event(provider_name: &str, parsed: &Value) -> ProviderError {
    let error_msg = parsed
        .get("error")
        .and_then(|e| e.as_str())
        .or_else(|| parsed.get("message").and_then(|m| m.as_str()))
        .unwrap_or("Unknown error");
    if error_msg.contains("context window exceeded") {
        ProviderError::ContextLengthExceeded(error_msg.to_string())
    } else {
        ProviderError::RequestFailed(format!("{provider_name} error: {error_msg}"))
    }
}

pub(crate) const SESSION_NAME_BEGIN_MARKER: &str = "---BEGIN USER MESSAGES---";
pub(crate) const SESSION_NAME_END_MARKER: &str = "---END USER MESSAGES---";
pub(crate) const SESSION_NAME_SUFFIX: &str = "Generate a short title for the above messages.";

pub(crate) fn generate_simple_session_description(
    model_name: &str,
    messages: &[Message],
) -> Result<(Message, ProviderUsage), ProviderError> {
    let description = messages
        .iter()
        .filter(|m| m.role == Role::User && m.is_user_visible())
        .find_map(|m| {
            m.content
                .iter()
                .filter_map(|content| content.filter_for_audience(Role::User))
                .find_map(|content| content.as_text().map(str::to_owned))
        })
        .map(|text| {
            let text = text.as_str();
            let text = text
                .rfind(SESSION_NAME_BEGIN_MARKER)
                .and_then(|idx| text.get(idx..))
                .unwrap_or(text);
            let stripped = text
                .strip_prefix(SESSION_NAME_BEGIN_MARKER)
                .unwrap_or(text)
                .trim_start_matches(['\n', '\r']);
            let full_suffix = format!("{}\n\n{}", SESSION_NAME_END_MARKER, SESSION_NAME_SUFFIX);
            let stripped = stripped
                .strip_suffix(&full_suffix)
                .or_else(|| stripped.strip_suffix(SESSION_NAME_END_MARKER))
                .unwrap_or(stripped)
                .trim();

            let desc: String = stripped
                .split_whitespace()
                .take(4)
                .collect::<Vec<_>>()
                .join(" ");
            if desc.is_empty() {
                "Simple task".to_string()
            } else {
                safe_truncate(&desc, 100)
            }
        })
        .unwrap_or_else(|| "Simple task".to_string());

    tracing::debug!(
        description = %description,
        "Generated simple session description, skipped subprocess"
    );

    let message = Message::new(
        Role::Assistant,
        chrono::Utc::now().timestamp(),
        vec![MessageContent::text(description)],
    );

    Ok((
        message,
        ProviderUsage::new(model_name.to_string(), Usage::default()),
    ))
}

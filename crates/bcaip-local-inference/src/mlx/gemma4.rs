use crate::{
    extract_text_content, provider_utils::filter_extensions_from_system_prompt,
    tool_emulation::load_tiny_model_prompt,
};
use goose_provider_types::conversations::Message;
use safemlx_lm::models::LoadedModel;
use safemlx_lm_utils::tokenizer::Role;
use serde_json::json;
pub(crate) fn is_gemma4(model: &LoadedModel) -> bool {
    matches!(model.model_type(), "gemma4" | "gemma4_text")
}
pub(crate) fn gemma4_messages(
    model_name: &str,
    system: &str,
    messages: &[Message],
) -> Vec<serde_json::Value> {
    let system = gemma4_system_prompt(model_name, system);
    gemma4_messages_with_optional_system(system.as_deref(), messages)
}
pub(crate) fn gemma4_messages_with_system(
    system: &str,
    messages: &[Message],
) -> Vec<serde_json::Value> {
    gemma4_messages_with_optional_system(Some(system), messages)
}
pub(crate) fn gemma4_messages_with_optional_system(
    system: Option<&str>,
    messages: &[Message],
) -> Vec<serde_json::Value> {
    let mut values = Vec::new();
    if let Some(system) = system.map(str::trim).filter(|system| !system.is_empty()) {
        values.push(json!({
            "role": "system",
            "content": system,
        }));
    }

    for message in messages.iter().filter(|message| message.is_agent_visible()) {
        let text = extract_text_content(message);
        if text.trim().is_empty() {
            continue;
        }

        match message.role {
            rmcp::model::Role::User => values.push(json!({
                "role": "user",
                "content": [{"type": "text", "text": text.trim(), "content": text.trim()}],
            })),
            rmcp::model::Role::Assistant => values.push(json!({
                "role": "assistant",
                "content": text.trim(),
            })),
        }
    }

    values
}
pub(crate) fn gemma4_system_prompt(model_name: &str, system: &str) -> Option<String> {
    if should_use_tiny_system_prompt(model_name) {
        return Some(load_tiny_model_prompt());
    }

    let filtered = filter_extensions_from_system_prompt(system);
    let system = filtered.trim();
    if system.is_empty() {
        None
    } else {
        Some(system.to_string())
    }
}
pub(crate) fn should_use_tiny_system_prompt(model_name: &str) -> bool {
    estimate_model_size_billions(model_name).is_some_and(|size| size <= 4.0)
}
pub(crate) fn estimate_model_size_billions(model_name: &str) -> Option<f32> {
    let normalized = model_name.to_ascii_lowercase().replace('-', "_");
    for part in normalized.split('_') {
        if let Some(value) = part.strip_suffix('b') {
            if let Ok(size) = value.parse::<f32>() {
                return Some(size);
            }
        }
        if let Some(value) = part
            .strip_prefix('e')
            .and_then(|value| value.strip_suffix('b'))
        {
            if let Ok(size) = value.parse::<f32>() {
                return Some(size);
            }
        }
    }
    None
}

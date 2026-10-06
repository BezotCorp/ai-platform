use bcaip_provider_types::conversations::{Message, MessageContent, ProviderMetadata};
use bcaip_provider_types::formats::response_to_message_openai;
use bcaip_provider_types::model::ModelConfig;
use bcaip_provider_types::thinking::ThinkingEffort;
use rmcp::model::Role;
use serde_json::{Value, json};

pub const REASONING_DETAILS_KEY: &str = "reasoning_details";

fn has_assistant_content(message: &Message) -> bool {
    message.content.iter().any(|c| match c {
        MessageContent::Text(t) => !t.text.is_empty(),
        MessageContent::Image(_) => true,
        MessageContent::ToolRequest(req) => req.tool_call.is_ok(),
        _ => false,
    })
}

pub fn extract_reasoning_details(response: &Value) -> Option<Vec<Value>> {
    response
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|m| m.get("message"))
        .and_then(|msg| msg.get("reasoning_details"))
        .and_then(|d| d.as_array())
        .cloned()
}

pub fn get_reasoning_details(metadata: &Option<ProviderMetadata>) -> Option<Vec<Value>> {
    metadata
        .as_ref()
        .and_then(|m| m.get(REASONING_DETAILS_KEY))
        .and_then(|v| v.as_array())
        .cloned()
}

pub fn response_to_message(response: &Value) -> anyhow::Result<Message> {
    let mut message = response_to_message_openai(response)?;

    if let Some(details) = extract_reasoning_details(response) {
        for content in &mut message.content {
            if let MessageContent::ToolRequest(req) = content {
                let mut meta = req.metadata.clone().unwrap_or_default();
                meta.insert(REASONING_DETAILS_KEY.to_string(), json!(details));
                req.metadata = Some(meta);
            }
        }
    }

    Ok(message)
}

pub fn add_reasoning_details_to_request(payload: &mut Value, messages: &[Message]) {
    let mut assistant_reasoning: Vec<Option<Vec<Value>>> = messages
        .iter()
        .filter(|m| m.is_agent_visible())
        .filter(|m| m.role == Role::Assistant)
        .filter(|m| has_assistant_content(m))
        .map(|message| {
            message.content.iter().find_map(|c| match c {
                MessageContent::ToolRequest(req) => get_reasoning_details(&req.metadata),
                _ => None,
            })
        })
        .collect();

    if let Some(payload_messages) = payload
        .as_object_mut()
        .and_then(|obj| obj.get_mut("messages"))
        .and_then(|m| m.as_array_mut())
    {
        let mut assistant_idx = 0;
        for payload_msg in payload_messages.iter_mut() {
            if payload_msg.get("role").and_then(|r| r.as_str()) == Some("assistant") {
                if assistant_idx < assistant_reasoning.len() {
                    if let Some(details) = assistant_reasoning
                        .get_mut(assistant_idx)
                        .and_then(|d| d.take())
                    {
                        if let Some(obj) = payload_msg.as_object_mut() {
                            obj.insert("reasoning_details".to_string(), json!(details));
                        }
                    }
                }
                assistant_idx += 1;
            }
        }
    }
}

fn reasoning_effort_for_openrouter(effort: ThinkingEffort) -> Option<&'static str> {
    match effort {
        ThinkingEffort::Off => None,
        ThinkingEffort::Low => Some("low"),
        ThinkingEffort::Medium => Some("medium"),
        ThinkingEffort::High => Some("high"),
        ThinkingEffort::Max => Some("xhigh"),
    }
}

/// Returns true when a reasoning disable request was inserted, which
/// mandatory-reasoning endpoints reject; the provider downgrades those to
/// the lowest effort on OpenRouter's mandatory-reasoning error.
pub fn apply_reasoning_config(payload: &mut Value, model_config: &ModelConfig) -> bool {
    let Some(effort) = model_config.thinking_effort() else {
        return false;
    };

    if let Some(obj) = payload.as_object_mut() {
        if obj.contains_key("reasoning") {
            obj.remove("reasoning_effort");
            return false;
        }

        let clamped_effort = obj
            .remove("reasoning_effort")
            .and_then(|value| value.as_str().map(str::to_owned));
        if effort == ThinkingEffort::Off {
            if !model_config.is_reasoning_model() {
                return false;
            }
            return match clamped_effort {
                Some(clamped) => {
                    obj.insert("reasoning".to_string(), json!({ "effort": clamped }));
                    false
                }
                None => {
                    obj.insert("reasoning".to_string(), json!({ "enabled": false }));
                    true
                }
            };
        }
        if clamped_effort.is_none() && !model_config.is_reasoning_model() {
            return false;
        }

        let effort = clamped_effort
            .as_deref()
            .or_else(|| reasoning_effort_for_openrouter(effort));
        if let Some(effort) = effort {
            obj.insert("reasoning".to_string(), json!({ "effort": effort }));
        }
    }
    false
}

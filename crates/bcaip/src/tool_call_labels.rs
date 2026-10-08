use crate::agents::Agent;
use crate::session::SessionManager;
use crate::utils::safe_truncate;
use bcaip_provider_types::base::Provider;
use bcaip_provider_types::conversations::{
    Message, MessageContent, TOOL_META_CHAIN_SUMMARY_KEY, TOOL_META_TITLE_KEY, ToolChainSummary,
    ToolRequest,
};
use bcaip_provider_types::model::ModelConfig;
use serde_json::json;
use std::{slice::from_ref, time::Duration};
use tokio::time::sleep;
use tracing::warn;
const TOOL_TITLE_SYSTEM_PROMPT: &str = "Summarize this tool call in a short lowercase phrase (3-8 words). \
     No punctuation. No quotes. Examples: reading project configuration, \
     checking network connectivity, listing files in src directory";
const TOOL_TITLE_ARGUMENTS_MAX_LENGTH: usize = 300;
const TOOL_CHAIN_SUMMARY_SYSTEM_PROMPT: &str = "Summarize this sequence of tool calls in a short lowercase phrase \
     (3-8 words). No punctuation. No quotes. \
     Examples: applied dark mode polish, scanned for security issues, \
     refactored config loading";
const TOOL_CHAIN_ARGUMENTS_MAX_LENGTH: usize = 200;
const LABEL_GENERATION_MAX_ATTEMPTS: usize = 2;
const LABEL_GENERATION_RETRY_DELAY: Duration = Duration::from_millis(150);

pub(crate) async fn generate_tool_title(
    agent: &Agent,
    session_manager: &SessionManager,
    session_id: &str,
    tool_request: &ToolRequest,
) -> Option<String> {
    let provider = agent.provider().await.ok()?;
    if provider.manages_own_context() {
        return None;
    }

    let model_config = agent.model_config_for_session(session_id).await.ok()?;
    let title = generate_tool_title_with_provider(
        provider.as_ref(),
        &model_config,
        session_id,
        tool_request,
    )
    .await?;
    let request_id = &tool_request.id;

    let patch = json!({
        (TOOL_META_TITLE_KEY): &title,
    });
    if let Err(error) = session_manager
        .update_tool_request_meta(session_id, request_id, patch)
        .await
    {
        warn!("tool call title: persist failed for {request_id}: {error}");
    }

    Some(title)
}

pub(crate) async fn generate_tool_chain_summary(
    agent: &Agent,
    session_manager: &SessionManager,
    session_id: &str,
    tool_requests: &[ToolRequest],
) -> Option<ToolChainSummary> {
    let steps = prepare_tool_chain_steps(tool_requests);
    if steps.len() < 2 {
        return None;
    }

    let provider = agent.provider().await.ok()?;
    if provider.manages_own_context() {
        return None;
    }

    let model_config = agent.model_config_for_session(session_id).await.ok()?;
    let chain_summary = ToolChainSummary {
        summary: generate_tool_chain_summary_with_provider(
            provider.as_ref(),
            &model_config,
            session_id,
            &steps,
        )
        .await?,
        count: tool_requests.len(),
    };
    let first_tool_call_id = &tool_requests.first()?.id;
    let patch = json!({
        (TOOL_META_CHAIN_SUMMARY_KEY): &chain_summary,
    });
    if let Err(error) = session_manager
        .update_tool_request_meta(session_id, first_tool_call_id, patch)
        .await
    {
        warn!(
            "tool chain summary: persist failed for chain anchored at {first_tool_call_id}: {error}",
        );
    }

    Some(chain_summary)
}

fn prepare_tool_chain_steps(tool_requests: &[ToolRequest]) -> Vec<(String, String)> {
    tool_requests
        .iter()
        .filter_map(|request| {
            let tool_call = request.tool_call.as_ref().ok()?;
            let arguments = tool_call
                .arguments
                .as_ref()
                .map(|arguments| {
                    let serialized = serde_json::to_string(arguments).unwrap_or_default();
                    if serialized.len() > TOOL_CHAIN_ARGUMENTS_MAX_LENGTH {
                        format!(
                            "{}…",
                            safe_truncate(&serialized, TOOL_CHAIN_ARGUMENTS_MAX_LENGTH)
                        )
                    } else {
                        serialized
                    }
                })
                .unwrap_or_default();
            Some((tool_call.name.to_string(), arguments))
        })
        .collect()
}

async fn generate_tool_title_with_provider(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    tool_request: &ToolRequest,
) -> Option<String> {
    let tool_call = tool_request.tool_call.as_ref().ok()?;
    let name = &tool_call.name;
    let args_json = tool_call
        .arguments
        .as_ref()
        .map(|arguments| {
            let serialized = serde_json::to_string(arguments).unwrap_or_default();
            if serialized.len() > TOOL_TITLE_ARGUMENTS_MAX_LENGTH {
                format!(
                    "{}…",
                    safe_truncate(&serialized, TOOL_TITLE_ARGUMENTS_MAX_LENGTH)
                )
            } else {
                serialized
            }
        })
        .unwrap_or_default();
    let message = Message::user().with_text(format!("Tool: {name}\nArguments: {args_json}"));

    complete_label(
        provider,
        model_config,
        session_id,
        TOOL_TITLE_SYSTEM_PROMPT,
        &message,
    )
    .await
}

async fn generate_tool_chain_summary_with_provider(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    steps: &[(String, String)],
) -> Option<String> {
    let mut user_text = String::from("Tool call sequence:\n");
    for (index, (name, args)) in steps.iter().enumerate() {
        user_text.push_str(&format!("Step {}: {} {}\n", index + 1, name, args));
    }
    let message = Message::user().with_text(user_text);

    complete_label(
        provider,
        model_config,
        session_id,
        TOOL_CHAIN_SUMMARY_SYSTEM_PROMPT,
        &message,
    )
    .await
}

async fn complete_label(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    system_prompt: &str,
    message: &Message,
) -> Option<String> {
    for attempt in 0..LABEL_GENERATION_MAX_ATTEMPTS {
        if let Ok((response, _)) = crate::model_config::complete_one_shot(
            provider,
            model_config,
            session_id,
            system_prompt,
            from_ref(message),
            &[],
        )
        .await
        {
            let label = response
                .content
                .iter()
                .filter_map(MessageContent::as_text)
                .collect::<String>()
                .trim()
                .to_string();
            if !label.is_empty() {
                return Some(label);
            }
        }

        if attempt + 1 < LABEL_GENERATION_MAX_ATTEMPTS {
            sleep(LABEL_GENERATION_RETRY_DELAY).await;
        }
    }

    None
}

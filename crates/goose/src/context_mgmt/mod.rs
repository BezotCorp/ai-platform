//mod.rs need to have only module declarations and public exports. So review and extract
use crate::{config::Config, token_counter::create_token_counter};
use anyhow::Result;
use goose_context_management::DEFAULT_COMPACTION_THRESHOLD;
use goose_context_management::format_message_for_compacting;
use bcaip_provider_types::base::Provider;
use bcaip_provider_types::conversations::MessageMetadata;
use bcaip_provider_types::conversations::ProviderUsage;
use bcaip_provider_types::conversations::{Conversation, merge_consecutive_messages};
use bcaip_provider_types::conversations::{Message, MessageContent};
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::model::ModelConfig;
use indoc::indoc;
use rmcp::model::Role;
use std::sync::Arc;
use tokio::task::JoinHandle;
use tracing::{info, log::warn};

pub(crate) const TOOLCALL_SUMMARIZATION_BATCH_SIZE: usize = 10;

pub(crate) fn tool_pair_summarization_enabled() -> bool {
    Config::global()
        .get_param::<bool>("GOOSE_TOOL_PAIR_SUMMARIZATION")
        .unwrap_or(false)
}

const CONVERSATION_CONTINUATION_TEXT: &str =
    "Your context was compacted. The previous message contains a summary of the conversation so far.
Do not mention that you read a summary or that conversation summarization occurred.
Just continue the conversation naturally based on the summarized context.";

const TOOL_LOOP_CONTINUATION_TEXT: &str =
    "Your context was compacted. The previous message contains a summary of the conversation so far.
Do not mention that you read a summary or that conversation summarization occurred.
Continue calling tools as necessary to complete the task.";

const MANUAL_COMPACT_CONTINUATION_TEXT: &str =
    "Your context was compacted at the user's request. The previous message contains a summary of the conversation so far.
Do not mention that you read a summary or that conversation summarization occurred.
Just continue the conversation naturally based on the summarized context.";

pub struct CompactionResult {
    pub conversation: Conversation,
    /// Billable usage of the summarization call, counting the raw model
    /// output even when it is rewritten to the rendered structured summary.
    pub usage: ProviderUsage,
    /// Estimated tokens of the agent-visible context retained after
    /// compaction. Smaller than the billable output when the raw response was
    /// rewritten to the rendered structured summary.
    pub retained_context_tokens: i32,
}

/// Compact messages by summarizing them
///
/// This function performs the actual compaction by summarizing messages and updating
/// their visibility metadata. It does not check thresholds - use `check_if_compaction_needed`
/// first to determine if compaction is necessary.
///
/// # Arguments
/// * `provider` - The provider to use for summarization
/// * `session_id` - The session to use for summarization
/// * `conversation` - The current conversation history
/// * `manual_compact` - If true, this is a manual compaction (don't preserve user message)
pub async fn compact_messages(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    conversation: &Conversation,
    manual_compact: bool,
) -> Result<CompactionResult> {
    info!("Performing message compaction");

    let messages = conversation.messages();

    let has_text_only = |msg: &Message| {
        let has_text = msg
            .content
            .iter()
            .any(|c| matches!(c, MessageContent::Text(_)));
        let has_tool_content = msg.content.iter().any(|c| {
            matches!(
                c,
                MessageContent::ToolRequest(_) | MessageContent::ToolResponse(_)
            )
        });
        has_text && !has_tool_content
    };

    // Turn-context events are agent-appended, never the message to preserve.
    let (preserved_user_message, preserved_idx, is_most_recent) = if !manual_compact {
        let found_msg = messages.iter().enumerate().rev().find_map(|(idx, msg)| {
            if !msg.is_agent_visible()
                || msg.is_turn_context()
                || !matches!(msg.role, rmcp::model::Role::User)
            {
                return None;
            }

            let projected = msg.agent_visible_content();
            if !has_text_only(&projected) {
                return None;
            }

            let preserved = projected
                .content
                .into_iter()
                .filter(|content| matches!(content, MessageContent::Text(_)))
                .fold(
                    Message::user().with_metadata(MessageMetadata::agent_only()),
                    Message::with_content,
                );
            Some((idx, preserved))
        });

        if let Some((idx, msg)) = found_msg {
            let is_last = messages[idx + 1..].iter().all(Message::is_turn_context);
            (Some(msg), Some(idx), is_last)
        } else {
            (None, None, false)
        }
    } else {
        (None, None, false)
    };

    let messages_to_compact = messages.as_slice();

    let (summary_message, summarization_usage) =
        do_compact(provider, model_config, session_id, messages_to_compact).await?;

    // Create the final message list with updated visibility metadata:
    // 1. Original messages become user_visible but not agent_visible
    // 2. Summary message becomes agent_visible but not user_visible
    // 3. Assistant messages to continue the conversation are also agent_visible but not user_visible
    let mut final_messages = Vec::new();

    for msg in messages_to_compact {
        let updated_metadata = msg.metadata.clone().with_agent_invisible();
        let updated_msg = msg.clone().with_metadata(updated_metadata);
        final_messages.push(updated_msg);
    }

    let summary_msg = summary_message.with_metadata(MessageMetadata::agent_only());

    let mut continuation_messages = vec![summary_msg];

    let continuation_text = if manual_compact {
        MANUAL_COMPACT_CONTINUATION_TEXT
    } else if is_most_recent {
        CONVERSATION_CONTINUATION_TEXT
    } else {
        TOOL_LOOP_CONTINUATION_TEXT
    };

    let continuation_msg = Message::assistant()
        .with_text(continuation_text)
        .with_metadata(MessageMetadata::agent_only());
    let continuation_created = continuation_msg.created;
    continuation_messages.push(continuation_msg);

    let (merged_continuation, _issues) = merge_consecutive_messages(continuation_messages);
    final_messages.extend(merged_continuation);

    if let Some(mut user_msg) = preserved_user_message {
        user_msg.created = continuation_created;
        final_messages.push(user_msg);
    }

    // Carry the turn's own context event (it follows the preserved prompt) so
    // a mid-turn retry keeps it; anything earlier belongs to a previous turn.
    if let Some(carry_from) = preserved_idx.map(|idx| idx + 1)
        && let Some(turn_context) = messages_to_compact[carry_from..]
            .iter()
            .rev()
            .find(|msg| msg.is_turn_context() && msg.is_agent_visible())
    {
        let mut carried = turn_context.clone();
        carried.id = None;
        // Storage reloads order by created_timestamp; the copy must keep
        // its appended position, not resurface at the original event's time.
        if let Some(latest) = final_messages.iter().map(|msg| msg.created).max() {
            carried.created = carried.created.max(latest);
        }
        final_messages.push(carried);
    }

    let conversation = Conversation::new_unvalidated(final_messages);
    let retained_context_tokens = match count_context_tokens(conversation.messages()).await {
        Ok(tokens) => tokens,
        Err(error) => {
            warn!("Failed to count retained context tokens, using billable output tokens: {error}");
            summarization_usage.usage.output_tokens.unwrap_or(0)
        }
    };

    Ok(CompactionResult {
        conversation,
        usage: summarization_usage,
        retained_context_tokens,
    })
}

/// Estimate the tokens of the agent-visible messages, counted the same way
/// as the fallback estimation in `check_if_compaction_needed`.
pub(crate) async fn count_context_tokens(messages: &[Message]) -> Result<i32> {
    let counter = create_token_counter()
        .await
        .map_err(|error| anyhow::anyhow!("Failed to create token counter: {error}"))?;
    let total: usize = messages
        .iter()
        .filter(|message| message.is_agent_visible())
        .map(|message| counter.count_chat_tokens("", std::slice::from_ref(message), &[]))
        .sum();
    Ok(total.try_into()?)
}

/// Check if messages exceed the auto-compaction threshold
pub async fn check_if_compaction_needed(
    provider: &dyn Provider,
    conversation: &Conversation,
    threshold_override: Option<f64>,
    session: &crate::session::Session,
) -> Result<bool> {
    if provider.manages_own_context() {
        return Ok(false);
    }

    let messages = conversation.messages();
    let config = Config::global();
    let threshold = threshold_override.unwrap_or_else(|| {
        config
            .get_param::<f64>("GOOSE_AUTO_COMPACT_THRESHOLD")
            .unwrap_or(DEFAULT_COMPACTION_THRESHOLD)
    });

    let model_config = session
        .model_config
        .clone()
        .unwrap_or_else(|| ModelConfig::new("unknown"));
    let context_limit =
        crate::context_limit::get_context_limit(provider, &model_config.model_name).await?;

    let (current_tokens, _token_source) = match session.usage.total_tokens {
        Some(tokens) => (tokens as usize, "session metadata"),
        None => {
            let token_counter = create_token_counter()
                .await
                .map_err(|e| anyhow::anyhow!("Failed to create token counter: {}", e))?;

            let token_counts: Vec<_> = messages
                .iter()
                .filter(|m| m.is_agent_visible())
                .map(|msg| token_counter.count_chat_tokens("", std::slice::from_ref(msg), &[]))
                .collect();

            (token_counts.iter().sum(), "estimated")
        }
    };

    let usage_ratio = current_tokens as f64 / context_limit as f64;

    let needs_compaction = if threshold <= 0.0 || threshold >= 1.0 {
        false // Auto-compact is disabled.
    } else {
        usage_ratio > threshold
    };
    Ok(needs_compaction)
}

struct GooseCompactionModel<'a> {
    provider: &'a dyn Provider,
    model_config: &'a ModelConfig,
    session_id: &'a str,
}

#[async_trait::async_trait]
impl goose_context_management::CompactionModel for GooseCompactionModel<'_> {
    async fn complete(
        &self,
        system: &str,
        messages: &[Message],
    ) -> Result<(Message, ProviderUsage), ProviderError> {
        crate::model_config::complete_one_shot(
            self.provider,
            self.model_config,
            self.session_id,
            system,
            messages,
            &[],
        )
        .await
    }
}

struct GooseTokenEstimator;

#[async_trait::async_trait]
impl goose_context_management::TokenEstimator for GooseTokenEstimator {
    async fn count_chat_tokens(&self, system: &str, messages: &[Message]) -> usize {
        match create_token_counter().await {
            Ok(counter) => counter.count_chat_tokens(system, messages, &[]),
            Err(error) => {
                warn!("Failed to create token counter: {error}");
                0
            }
        }
    }

    async fn count_text_tokens(&self, text: &str) -> usize {
        match create_token_counter().await {
            Ok(counter) => counter.count_tokens(text),
            Err(error) => {
                warn!("Failed to create token counter: {error}");
                0
            }
        }
    }
}

fn compaction_templates() -> Result<goose_context_management::Templates> {
    Ok(goose_context_management::Templates {
        compaction: crate::prompt_template::template_source("compaction.md")?,
        summary: crate::prompt_template::template_source("compaction_summary.md")?,
    })
}

async fn do_compact(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    messages: &[Message],
) -> Result<(Message, ProviderUsage), anyhow::Error> {
    // Keep stale per-turn state out of the summary.
    let agent_visible_messages = Conversation::new_unvalidated(
        messages
            .iter()
            .filter(|msg| !msg.is_turn_context())
            .cloned(),
    )
    .agent_visible_messages();

    let model = GooseCompactionModel {
        provider,
        model_config,
        session_id,
    };
    let summary = goose_context_management::summarize(
        &model,
        Some(&GooseTokenEstimator),
        &compaction_templates()?,
        &agent_visible_messages,
    )
    .await?;

    Ok((summary.message, summary.usage))
}

pub fn compute_tool_call_cutoff(context_limit: usize, compaction_threshold: f64) -> usize {
    let threshold = if compaction_threshold > 0.0 && compaction_threshold <= 1.0 {
        compaction_threshold
    } else {
        DEFAULT_COMPACTION_THRESHOLD
    };
    let effective_limit = (context_limit as f64 * threshold) as usize;
    (3 * effective_limit / 20_000).clamp(10, 500)
}

pub fn tool_ids_to_summarize(
    conversation: &Conversation,
    cutoff: usize,
    protect_last_n: usize,
) -> Vec<String> {
    let messages = conversation.messages();

    let mut tool_call_ids: Vec<String> = Vec::new();

    for msg in messages.iter() {
        if !msg.is_agent_visible() {
            continue;
        }

        for content in &msg.content {
            if let MessageContent::ToolRequest(req) = content {
                tool_call_ids.push(req.id.clone());
            }
        }
    }

    // Never summarize the last N tool calls (current turn)
    let eligible = tool_call_ids.len().saturating_sub(protect_last_n);
    if eligible <= cutoff + TOOLCALL_SUMMARIZATION_BATCH_SIZE {
        return Vec::new();
    }

    tool_call_ids
        .into_iter()
        .take(TOOLCALL_SUMMARIZATION_BATCH_SIZE)
        .collect()
}

fn agent_visible_tool_pair(conversation: &Conversation, tool_id: &str) -> Result<Vec<Message>> {
    let matching_messages = conversation
        .messages()
        .iter()
        .filter(|m| {
            m.content.iter().any(|c| match c {
                MessageContent::ToolRequest(req) => req.id == tool_id,
                MessageContent::ToolResponse(resp) => resp.id == tool_id,
                _ => false,
            })
        })
        .cloned()
        .collect::<Vec<_>>();
    let matching_messages =
        Conversation::new_unvalidated(matching_messages).agent_visible_messages();

    let has_request = matching_messages.iter().any(|message| {
        message.content.iter().any(
            |content| matches!(content, MessageContent::ToolRequest(request) if request.id == tool_id),
        )
    });
    let has_response = matching_messages.iter().any(|message| {
        message.content.iter().any(
            |content| matches!(content, MessageContent::ToolResponse(response) if response.id == tool_id),
        )
    });
    if !has_request || !has_response {
        return Err(anyhow::anyhow!(
            "No agent-visible tool pair found for tool id: {}",
            tool_id
        ));
    }
    Ok(matching_messages)
}

pub async fn summarize_tool_call(
    provider: &dyn Provider,
    model_config: &ModelConfig,
    session_id: &str,
    conversation: &Conversation,
    tool_id: &str,
) -> Result<Message> {
    let matching_messages = agent_visible_tool_pair(conversation, tool_id)?;

    let formatted = matching_messages
        .iter()
        .map(format_message_for_compacting)
        .collect::<Vec<_>>()
        .join("\n");

    let user_message = Message::user().with_text(formatted);
    let summarization_request = vec![user_message];

    let system_prompt = indoc! {r#"
                Your task is to summarize a tool call & response pair to save tokens.

                Reply with a single message that describes what happened. Typically a tool call
                asks for something using a bunch of parameters and then the result is also some
                structured output. So the tool might ask to look up something on github and the
                reply might be a json document. So you could reply with something like:

                "A call to github was made to get the project status"

                if that is what it was.
            "#};

    let (mut response, _) = crate::model_config::complete_one_shot(
        provider,
        model_config,
        session_id,
        system_prompt,
        &summarization_request,
        &[],
    )
    .await?;

    response.role = Role::User;
    response.created = matching_messages.last().unwrap().created;
    response.metadata = MessageMetadata::agent_only();

    Ok(response.with_generated_id())
}

pub fn maybe_summarize_tool_pairs(
    provider: Arc<dyn Provider>,
    model_config: ModelConfig,
    session_id: String,
    conversation: Conversation,
    cutoff: usize,
    protect_last_n: usize,
) -> Option<JoinHandle<Vec<(Message, String)>>> {
    if !tool_pair_summarization_enabled() || provider.manages_own_context() {
        return None;
    }

    let tool_ids = tool_ids_to_summarize(&conversation, cutoff, protect_last_n);
    if tool_ids.is_empty() {
        return None;
    }

    // A request/response message can contain multiple parallel tool calls.
    // Summarization formats whole messages, so sibling IDs in the same pair
    // must be compacted as one group or we'd issue duplicate summary calls.
    let mut seen_message_pairs = std::collections::HashSet::new();
    let mut grouped_tool_ids = Vec::new();
    for tool_id in tool_ids {
        let pair = match agent_visible_tool_pair(&conversation, &tool_id) {
            Ok(pair) => pair,
            Err(error) => {
                warn!("Failed to identify tool pair for summarization: {}", error);
                continue;
            }
        };
        if pair.len() != 2 {
            warn!(
                "Expected a tool request/response pair for '{}', found {} messages",
                tool_id,
                pair.len()
            );
            continue;
        }

        let request_ids: std::collections::HashSet<&str> = pair
            .iter()
            .flat_map(|message| message.get_tool_request_ids())
            .collect();
        let response_ids: std::collections::HashSet<&str> = pair
            .iter()
            .flat_map(|message| message.get_tool_response_ids())
            .collect();
        if request_ids != response_ids {
            warn!(
                "Tool pair for '{}' has siblings answered elsewhere; skipping",
                tool_id
            );
            continue;
        }

        let mut message_ids = pair
            .iter()
            .filter_map(|message| message.id.clone())
            .collect::<Vec<_>>();
        if message_ids.len() != 2 {
            warn!(
                "Expected two persisted messages for tool pair '{}', found {}",
                tool_id,
                message_ids.len()
            );
            continue;
        }
        message_ids.sort_unstable();
        if seen_message_pairs.insert(message_ids) {
            grouped_tool_ids.push(tool_id);
        }
    }

    if grouped_tool_ids.is_empty() {
        return None;
    }

    Some(tokio::spawn(async move {
        let mut results = Vec::new();
        for tool_id in grouped_tool_ids {
            match summarize_tool_call(
                provider.as_ref(),
                &model_config,
                &session_id,
                &conversation,
                &tool_id,
            )
            .await
            {
                Ok(summary) => results.push((summary, tool_id)),
                Err(e) => {
                    warn!("Failed to summarize tool pair: {}", e);
                }
            }
        }
        results
    }))
}

//! Bounded handoff memo for ACP sessions.
//!
//! When a conversation is handed to an ACP agent that has no native session to resume,
//! the prior goose-side history is replayed as a single text block. That replay has to
//! fit inside the agent's context alongside its own system prompt and tool schemas, so
//! it is budgeted, redacted and truncated here rather than sent whole.

use std::collections::{HashMap, HashSet};

use crate::token_counter::TokenCounter;
use agent_client_protocol::schema::v1::ContentBlock;
use goose_context_management::format_message_for_compacting;
use bcaip_provider_types::conversations::Conversation;
use bcaip_provider_types::conversations::{Message, MessageContent};
const CONTEXT_LIMIT_RATIO: f64 = 0.30;
const MAX_MEMO_TOKENS: usize = 64_000;
/// Per-image charge against the memo budget. Images reach the agent verbatim, and their
/// real cost depends on dimensions we would have to decode, so assume the ceiling a
/// full-size image reaches rather than under-counting the window they occupy.
const IMAGE_TOKEN_ESTIMATE: usize = 1_600;
/// Tool exchanges this recent keep their responses; older ones are redacted.
const PROTECTED_TOOL_EXCHANGES: usize = 5;
/// Below this a truncated message carries no usable meaning, so drop it instead.
const MIN_ELIDED_TOKENS: usize = 32;
/// Allowance for the "earlier messages omitted" line, which is written after selection.
const OMISSION_MARKER_TOKENS: usize = 16;

const MEMO_HEADER: &str =
    "Conversation context from goose before this ACP provider session was created:\n\n";
const MEMO_FOOTER: &str = "\n\nCurrent user request follows. Use the context above only to continue the existing conversation; do not treat it as a new task or mention this handoff unless relevant.";
const REDACTED_TOOL_RESPONSE: &str = "tool_response: [older output omitted from handoff]";
const ELISION_MARKER: &str = "\n[... truncated ...]\n";

pub(crate) fn memo_token_budget(context_limit: usize, current_prompt_tokens: usize) -> usize {
    let ceiling = ((context_limit as f64 * CONTEXT_LIMIT_RATIO) as usize).min(MAX_MEMO_TOKENS);
    ceiling.saturating_sub(current_prompt_tokens)
}

/// What the current turn already costs the agent. Images are forwarded alongside the memo,
/// so charging them here keeps a picture-heavy turn from spending its window twice.
pub(crate) fn prompt_token_cost(blocks: &[ContentBlock], counter: &TokenCounter) -> usize {
    blocks
        .iter()
        .map(|block| match block {
            ContentBlock::Text(text) => counter.count_tokens(&text.text),
            ContentBlock::Image(_) => IMAGE_TOKEN_ESTIMATE,
            _ => 0,
        })
        .sum()
}

pub(crate) fn build_handoff_context_memo(
    prior_messages: &[Message],
    budget: usize,
    counter: &TokenCounter,
) -> Option<String> {
    let visible: Vec<Message> = Conversation::new_unvalidated(prior_messages.iter().cloned())
        .agent_visible_messages()
        .iter()
        .filter(|message| !message.is_turn_context())
        .map(|message| message.agent_visible_content())
        .collect();

    if visible.is_empty() {
        return None;
    }

    let protected = recent_tool_call_ids(&visible);
    let redacted: Vec<Message> = visible
        .iter()
        .map(|message| redact_tool_responses(message, |id| protected.contains(id)))
        .collect();
    let formatted: Vec<String> = redacted.iter().map(format_message_for_compacting).collect();
    let units = selection_units(&visible, &protected);

    let overhead = counter.count_tokens(MEMO_HEADER)
        + counter.count_tokens(MEMO_FOOTER)
        + OMISSION_MARKER_TOKENS;
    let mut remaining = budget.saturating_sub(overhead);

    let mut kept: Vec<String> = Vec::new();
    for unit in units.iter().rev() {
        if remaining == 0 {
            break;
        }
        let Some(fitted) = fit_unit(unit, &formatted, &redacted, remaining, counter) else {
            break;
        };
        kept.extend(fitted.messages.into_iter().rev());
        match fitted.cost {
            Some(cost) => remaining -= cost,
            None => remaining = 0,
        }
    }

    if kept.is_empty() {
        return None;
    }

    kept.reverse();
    let omitted = formatted.len() - kept.len();
    let mut body = String::new();
    if omitted > 0 {
        body.push_str(&format!("[{omitted} earlier messages omitted]\n"));
    }
    body.push_str(&kept.join("\n"));

    Some(format!("{MEMO_HEADER}{body}{MEMO_FOOTER}"))
}

/// Ids of the most recent tool exchanges, keyed by response so parallel and batched
/// calls are protected individually rather than by message position.
fn recent_tool_call_ids(messages: &[Message]) -> HashSet<String> {
    let mut ids: Vec<&str> = Vec::new();
    for message in messages {
        for content in &message.content {
            if let MessageContent::ToolResponse(response) = content {
                ids.push(&response.id);
            }
        }
    }
    ids.into_iter()
        .rev()
        .take(PROTECTED_TOOL_EXCHANGES)
        .map(str::to_string)
        .collect()
}

/// Contiguous message groups that are kept or dropped together. A protected tool response
/// travels with the request that produced it, so a tight budget can never leave one half of
/// an exchange orphaned.
fn selection_units(messages: &[Message], protected: &HashSet<String>) -> Vec<Vec<usize>> {
    let mut earliest: Vec<usize> = (0..messages.len()).collect();
    let mut request_at: HashMap<&str, usize> = HashMap::new();
    for (index, message) in messages.iter().enumerate() {
        for content in &message.content {
            match content {
                MessageContent::ToolRequest(request) => {
                    request_at.insert(request.id.as_str(), index);
                }
                MessageContent::ToolResponse(response) if protected.contains(&response.id) => {
                    if let Some(&request_index) = request_at.get(response.id.as_str()) {
                        earliest[index] = earliest[index].min(request_index);
                    }
                }
                _ => {}
            }
        }
    }

    let mut units: Vec<Vec<usize>> = Vec::new();
    let mut end = messages.len();
    while end > 0 {
        let mut start = end - 1;
        loop {
            let extended = earliest[start..end].iter().copied().min().unwrap_or(start);
            if extended == start {
                break;
            }
            start = extended;
        }
        units.push((start..end).collect());
        end = start;
    }
    units.reverse();
    units
}

struct FittedUnit {
    messages: Vec<String>,
    /// `None` when the unit had to be degraded to fit, which ends selection.
    cost: Option<usize>,
}

/// Fit a whole unit into `budget`, degrading it only in ways that keep every exchange
/// it holds complete.
fn fit_unit(
    unit: &[usize],
    formatted: &[String],
    redacted: &[Message],
    budget: usize,
    counter: &TokenCounter,
) -> Option<FittedUnit> {
    let members: Vec<String> = unit.iter().map(|&index| formatted[index].clone()).collect();
    let cost = unit_cost(&members, counter);
    if cost <= budget {
        return Some(FittedUnit {
            messages: members,
            cost: Some(cost),
        });
    }

    // Eliding a message that carries protected responses would cut individual calls out of
    // the middle of a batch. Degrade the exchange the way a stale one is degraded instead —
    // requests intact, responses replaced whole — so nothing is left half-reported.
    let degraded = if unit
        .iter()
        .any(|&index| holds_tool_response(&redacted[index]))
    {
        unit.iter()
            .map(|&index| {
                format_message_for_compacting(&redact_tool_responses(&redacted[index], |_| false))
            })
            .collect()
    } else {
        members
    };

    if unit_cost(&degraded, counter) <= budget {
        return Some(FittedUnit {
            messages: degraded,
            cost: None,
        });
    }

    elide_unit_to_budget(degraded, budget, counter).map(|messages| FittedUnit {
        messages,
        cost: None,
    })
}

/// Shrink the largest members until the whole unit fits. Nothing here carries a protected
/// response any more, so an oversized tool request is truncated rather than taking the
/// entire memo down with it.
fn elide_unit_to_budget(
    mut members: Vec<String>,
    budget: usize,
    counter: &TokenCounter,
) -> Option<Vec<String>> {
    for _ in 0..members.len() {
        let costs: Vec<usize> = members
            .iter()
            .map(|message| counter.count_tokens(message) + 1)
            .collect();
        let total: usize = costs.iter().sum();
        if total <= budget {
            return Some(members);
        }
        let (index, largest) = costs
            .iter()
            .enumerate()
            .max_by_key(|(_, cost)| **cost)
            .map(|(index, cost)| (index, *cost))?;
        let room = budget.checked_sub(total - largest + 1)?;
        members[index] = elide_to_budget(&members[index], room, counter)?;
    }
    (unit_cost(&members, counter) <= budget).then_some(members)
}

fn unit_cost(members: &[String], counter: &TokenCounter) -> usize {
    members
        .iter()
        .map(|message| counter.count_tokens(message) + 1)
        .sum()
}

fn holds_tool_response(message: &Message) -> bool {
    message
        .content
        .iter()
        .any(|content| matches!(content, MessageContent::ToolResponse(_)))
}

fn redact_tool_responses(message: &Message, keep: impl Fn(&str) -> bool) -> Message {
    let should_redact = |content: &MessageContent| matches!(content, MessageContent::ToolResponse(response) if !keep(&response.id));
    if !message.content.iter().any(should_redact) {
        return message.clone();
    }

    let content = message
        .content
        .iter()
        .map(|content| {
            if should_redact(content) {
                MessageContent::text(REDACTED_TOOL_RESPONSE)
            } else {
                content.clone()
            }
        })
        .collect();

    Message {
        content,
        ..message.clone()
    }
}

/// Middle-elide `text` so it fits in `budget` tokens, keeping its head and tail.
fn elide_to_budget(text: &str, budget: usize, counter: &TokenCounter) -> Option<String> {
    if budget < MIN_ELIDED_TOKENS {
        return None;
    }

    let total = counter.count_tokens(text).max(1);
    let mut ratio = budget as f64 / total as f64;
    for _ in 0..6 {
        // Bytes to keep, so the floor below is a floor on the head and tail worth emitting
        // rather than a token count.
        let keep = ((text.len() as f64 * ratio * 0.9) as usize).min(text.len());
        if keep < 2 * MIN_ELIDED_TOKENS {
            return None;
        }
        let head_end = floor_char_boundary(text, keep / 2);
        let tail_start = ceil_char_boundary(text, text.len() - (keep - keep / 2));
        if tail_start <= head_end {
            return None;
        }
        let candidate = format!(
            "{}{ELISION_MARKER}{}",
            text.get(..head_end)?,
            text.get(tail_start..)?
        );
        if counter.count_tokens(&candidate) <= budget {
            return Some(candidate);
        }
        ratio *= 0.7;
    }
    None
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

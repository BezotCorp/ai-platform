use crate::conversations::{
    InvalidConversation, Message, MessageContentBlock, MessageMetadata, effective_role,
};
use crate::mcp_utils::extract_text_from_resource;
use rmcp::model::{ContentBlock, Role};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Conversation(Vec<Message>);

impl Conversation {
    pub fn new<I>(messages: I) -> Result<Self, InvalidConversation>
    where
        I: IntoIterator<Item = Message>,
    {
        Self::new_unvalidated(messages).validate()
    }

    pub fn new_unvalidated<I>(messages: I) -> Self
    where
        I: IntoIterator<Item = Message>,
    {
        Self(messages.into_iter().collect())
    }

    pub fn empty() -> Self {
        Self::new_unvalidated([])
    }

    pub fn messages(&self) -> &Vec<Message> {
        &self.0
    }

    pub fn messages_mut(&mut self) -> &mut Vec<Message> {
        &mut self.0
    }

    pub fn push(&mut self, message: Message) {
        let output_token_limit_reached = message.metadata.output_token_limit_reached;
        if message.content.is_empty()
            && (message.metadata.inference.is_some() || output_token_limit_reached)
        {
            if let Some(existing) = self.0.iter_mut().rev().find(|existing| {
                existing.role == message.role
                    && existing.is_user_visible()
                    && (!output_token_limit_reached
                        || (message.id.is_some()
                            && existing.id.as_deref() == message.id.as_deref()))
            }) {
                if let Some(inference) = message.metadata.inference.clone() {
                    existing.metadata.inference = Some(inference);
                }
                existing.metadata.output_token_limit_reached |= output_token_limit_reached;
                return;
            }

            if output_token_limit_reached {
                self.0.push(message.with_visibility(true, false));
            }
            return;
        }

        if let Some(last) = self
            .0
            .last_mut()
            .filter(|m| m.id.is_some() && m.id == message.id)
        {
            if message.metadata.inference.is_some() {
                last.metadata.inference = message.metadata.inference.clone();
            }
            last.metadata.output_token_limit_reached |= message.metadata.output_token_limit_reached;
            match (last.content.last_mut(), message.content.last()) {
                (Some(MessageContentBlock::Text(last)), Some(MessageContentBlock::Text(new)))
                    if message.content.len() == 1
                        && last.annotations.as_ref().and_then(|a| a.audience.as_ref())
                            == new.annotations.as_ref().and_then(|a| a.audience.as_ref()) =>
                {
                    last.text.push_str(&new.text);
                }
                (
                    Some(MessageContentBlock::Thinking(last)),
                    Some(MessageContentBlock::Thinking(new)),
                ) if message.content.len() == 1
                    && (last.signature.is_empty() || new.signature == last.signature) =>
                {
                    // Merge cases:
                    //   - `last` is still unsigned (block in progress) — append
                    //     and adopt `new.signature` if it's the closing delta.
                    //   - signatures match — same block continuing.
                    // An unsigned delta arriving after a signed block belongs
                    // to the next block (signature-at-end streams emit the
                    // first text of block N+1 before its signature), so the
                    // outer match arm falls through to push it separately.
                    last.thinking.push_str(&new.thinking);
                    if !new.signature.is_empty() {
                        last.signature = new.signature.clone();
                    }
                }
                (_, _) => {
                    last.content.extend(message.content);
                }
            }
        } else {
            self.0.push(message);
        }
    }

    pub fn last(&self) -> Option<&Message> {
        self.0.last()
    }

    pub fn first(&self) -> Option<&Message> {
        self.0.first()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = Message>,
    {
        for message in iter {
            self.push(message);
        }
    }

    pub fn iter(&self) -> std::slice::Iter<'_, Message> {
        self.0.iter()
    }

    pub fn pop(&mut self) -> Option<Message> {
        self.0.pop()
    }

    pub fn remove(&mut self, index: usize) -> Message {
        self.0.remove(index)
    }

    pub fn truncate(&mut self, len: usize) {
        self.0.truncate(len);
    }

    pub fn clear(&mut self) {
        self.0.clear();
    }

    pub fn filtered_messages<F>(&self, filter: F) -> Vec<Message>
    where
        F: Fn(&MessageMetadata) -> bool,
    {
        self.0
            .iter()
            .filter(|msg| filter(&msg.metadata))
            .cloned()
            .collect()
    }

    pub fn agent_visible_messages(&self) -> Vec<Message> {
        self.0
            .iter()
            .filter(|message| message.metadata.agent_visible)
            .map(Message::agent_visible_content)
            .filter(|message| !message.content.is_empty())
            .collect()
    }

    pub fn user_visible_messages(&self) -> Vec<Message> {
        self.0
            .iter()
            .filter(|message| message.metadata.user_visible)
            .map(Message::user_visible_content)
            .filter(|message| !message.content.is_empty())
            .collect()
    }

    fn validate(self) -> Result<Self, InvalidConversation> {
        let (_messages, issues) = fix_messages(self.0.clone());
        if !issues.is_empty() {
            let reason = issues.join("\n");
            Err(InvalidConversation::new(reason, self))
        } else {
            Ok(self)
        }
    }
}

impl Default for Conversation {
    fn default() -> Self {
        Self::empty()
    }
}

impl IntoIterator for Conversation {
    type Item = Message;
    type IntoIter = std::vec::IntoIter<Message>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
impl<'a> IntoIterator for &'a Conversation {
    type Item = &'a Message;
    type IntoIter = std::slice::Iter<'a, Message>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// Fix a conversation that we're about to send to an LLM. So the first and last
/// messages should always be from the user.
pub fn fix_conversation(conversation: Conversation) -> (Conversation, Vec<String>) {
    let all_messages = conversation.messages();

    // Create a shadow map: track each message as either Visible or NonVisible with its index
    enum MessageSlot {
        Visible(usize),      // Index into agent_visible_messages
        NonVisible(Message), // Non-visible messages pass through unchanged
    }

    let mut agent_visible_messages = Vec::new();
    let shadow_map: Vec<MessageSlot> = all_messages
        .iter()
        .map(|msg| {
            if msg.metadata.agent_visible {
                let idx = agent_visible_messages.len();
                agent_visible_messages.push(msg.clone());
                MessageSlot::Visible(idx)
            } else {
                MessageSlot::NonVisible(msg.clone())
            }
        })
        .collect();

    // Fix only the agent-visible messages
    let (fixed_visible, issues) = fix_messages(agent_visible_messages);

    // Reconstruct using shadow map: replace Visible slots with fixed messages
    let final_messages: Vec<Message> = shadow_map
        .into_iter()
        .filter_map(|slot| match slot {
            MessageSlot::Visible(idx) => fixed_visible.get(idx).cloned(),
            MessageSlot::NonVisible(msg) => Some(msg),
        })
        .collect();

    (Conversation::new_unvalidated(final_messages), issues)
}

fn fix_messages(messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    [
        merge_text_content_items,
        trim_assistant_text_whitespace,
        remove_empty_messages,
        fix_empty_tool_results,
        fix_tool_calling,
        merge_consecutive_messages,
        dedupe_signed_thinking,
        fix_lead_trail,
        populate_if_empty,
    ]
    .into_iter()
    .fold(
        (messages, Vec::new()),
        |(msgs, mut all_issues), processor| {
            let (new_msgs, issues) = processor(msgs);
            all_issues.extend(issues);
            (new_msgs, all_issues)
        },
    )
}

fn merge_text_content_in_message(mut msg: Message) -> Message {
    if msg.role != Role::Assistant {
        return msg;
    }
    msg.content = msg
        .content
        .into_iter()
        .fold(Vec::new(), |mut content, item| {
            match item {
                MessageContentBlock::Text(text) => match content.last_mut() {
                    Some(MessageContentBlock::Text(last))
                        if last.annotations.as_ref().and_then(|a| a.audience.as_ref())
                            == text.annotations.as_ref().and_then(|a| a.audience.as_ref()) =>
                    {
                        last.text.push_str(&text.text);
                    }
                    _ => content.push(MessageContentBlock::Text(text)),
                },
                other => content.push(other),
            }
            content
        });
    msg
}

fn merge_text_content_items(messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    messages.into_iter().fold(
        (Vec::new(), Vec::new()),
        |(mut messages, mut issues), message| {
            let content_len = message.content.len();
            let message = merge_text_content_in_message(message);
            if content_len != message.content.len() {
                issues.push(String::from("Merged text content"))
            }
            messages.push(message);
            (messages, issues)
        },
    )
}

fn trim_assistant_text_whitespace(messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    let mut issues = Vec::new();

    let fixed_messages = messages
        .into_iter()
        .map(|mut message| {
            if message.role == Role::Assistant {
                for content in &mut message.content {
                    if let MessageContentBlock::Text(text) = content {
                        let trimmed = text.text.trim_end();
                        if trimmed.len() != text.text.len() {
                            issues.push(
                                "Trimmed trailing whitespace from assistant message".to_string(),
                            );
                            text.text = trimmed.to_string();
                        }
                    }
                }
            }
            message
        })
        .collect();

    (fixed_messages, issues)
}

fn remove_empty_messages(messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    let mut issues = Vec::new();
    let filtered_messages = messages
        .into_iter()
        .filter(|msg| {
            if msg
                .content
                .iter()
                .all(|c| c.as_text().is_some_and(str::is_empty))
            {
                issues.push("Removed empty message".to_string());
                false
            } else {
                true
            }
        })
        .collect();
    (filtered_messages, issues)
}

/// Checks whether tool result content has any meaningful payload.
/// Text and resources must contain non-empty strings; images are always meaningful.
fn has_tool_result_content(content: &[ContentBlock]) -> bool {
    content.iter().any(|c| {
        if let Some(t) = c.as_text() {
            return !t.text.is_empty();
        }
        if let Some(r) = c.as_resource() {
            return !extract_text_from_resource(&r.resource).is_empty();
        }
        c.as_image().is_some()
    })
}

/// Fix tool results that would be empty when formatted for LLM APIs.
/// Some APIs (like Anthropic) reject tool_result blocks with empty content.
/// This adds a placeholder message for tool results that have no extractable text.
fn fix_empty_tool_results(messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    let mut issues = Vec::new();

    let fixed_messages = messages
        .into_iter()
        .map(|mut message| {
            for content in &mut message.content {
                if let MessageContentBlock::ToolResponse(tool_response) = content
                    && let Ok(ref mut result) = tool_response.tool_result
                    && !has_tool_result_content(&result.content)
                {
                    // Add a placeholder text content so the tool result isn't empty
                    result.content.push(ContentBlock::text("(empty result)"));
                    issues.push(format!(
                        "Added placeholder to empty tool result '{}'",
                        tool_response.id
                    ));
                }
            }
            message
        })
        .collect();

    (fixed_messages, issues)
}

fn fix_tool_calling(mut messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    let mut issues = Vec::new();
    let mut pending_tool_requests: HashSet<String> = HashSet::new();

    for message in &mut messages {
        let mut content_to_remove = Vec::new();

        match message.role {
            Role::User => {
                for (idx, content) in message.content.iter().enumerate() {
                    match content {
                        MessageContentBlock::ToolRequest(req) => {
                            content_to_remove.push(idx);
                            issues.push(format!(
                                "Removed tool request '{}' from user message",
                                req.id
                            ));
                        }
                        MessageContentBlock::ToolConfirmationRequest(req) => {
                            content_to_remove.push(idx);
                            issues.push(format!(
                                "Removed tool confirmation request '{}' from user message",
                                req.id
                            ));
                        }
                        MessageContentBlock::Thinking(_)
                        | MessageContentBlock::RedactedThinking(_) => {
                            content_to_remove.push(idx);
                            issues.push("Removed thinking content from user message".to_string());
                        }
                        MessageContentBlock::ToolResponse(resp) => {
                            if pending_tool_requests.contains(&resp.id) {
                                pending_tool_requests.remove(&resp.id);
                            } else {
                                content_to_remove.push(idx);
                                issues
                                    .push(format!("Removed orphaned tool response '{}'", resp.id));
                            }
                        }
                        _ => {}
                    }
                }
            }
            Role::Assistant => {
                for (idx, content) in message.content.iter().enumerate() {
                    match content {
                        MessageContentBlock::ToolResponse(resp) => {
                            content_to_remove.push(idx);
                            issues.push(format!(
                                "Removed tool response '{}' from assistant message",
                                resp.id
                            ));
                        }
                        MessageContentBlock::ToolRequest(req) => {
                            pending_tool_requests.insert(req.id.clone());
                        }
                        _ => {}
                    }
                }
            }
        }

        for &idx in content_to_remove.iter().rev() {
            message.content.remove(idx);
        }
    }

    for message in &mut messages {
        if message.role == Role::Assistant {
            let mut content_to_remove = Vec::new();
            for (idx, content) in message.content.iter().enumerate() {
                if let MessageContentBlock::ToolRequest(req) = content
                    && pending_tool_requests.contains(&req.id)
                {
                    content_to_remove.push(idx);
                    issues.push(format!("Removed orphaned tool request '{}'", req.id));
                }
            }
            for &idx in content_to_remove.iter().rev() {
                message.content.remove(idx);
            }
        }
    }
    let (messages, empty_removed) = remove_empty_messages(messages);
    issues.extend(empty_removed);
    (messages, issues)
}

/// Never merges across visibility or turn-context boundaries, so the result
/// is safe to persist.
pub fn merge_consecutive_messages(messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    merge_consecutive(messages, false)
}

/// Merges regardless of visibility, for providers that require strict role
/// alternation. Never persist the result.
pub fn merge_consecutive_messages_for_request(messages: Vec<Message>) -> Vec<Message> {
    merge_consecutive(messages, true).0
}

fn merge_consecutive(
    messages: Vec<Message>,
    across_visibility: bool,
) -> (Vec<Message>, Vec<String>) {
    let mut issues = Vec::new();
    let mut merged_messages: Vec<Message> = Vec::new();

    for message in messages {
        if let Some(last) = merged_messages.last_mut() {
            let effective = effective_role(&message);
            if effective_role(last) == effective
                && (across_visibility
                    || (last.metadata.user_visible == message.metadata.user_visible
                        && last.metadata.turn_context == message.metadata.turn_context))
            {
                last.content.extend(message.content);
                issues.push(format!("Merged consecutive {} messages", effective));
                continue;
            }
        }
        merged_messages.push(message);
    }

    (merged_messages, issues)
}

/// Signed thinking carries a signature; redacted thinking is always signed.
/// Signed blocks must be replayed exactly; unsigned reasoning summaries need not.
fn is_signed_thinking(content: &MessageContentBlock) -> bool {
    match content {
        MessageContentBlock::Thinking(t) => !t.signature.is_empty(),
        MessageContentBlock::RedactedThinking(_) => true,
        _ => false,
    }
}

/// Drops duplicate signed thinking blocks, keeping the first occurrence. Some
/// signed-replay APIs (like Anthropic) reject a request that repeats the same
/// signed block more than once.
///
/// Duplicates arise two ways, both handled here:
///   - Within one assistant message, when a standalone thinking message is
///     merged with a tool-call message that re-embedded the same thinking.
///   - Across assistant messages, when the agent splits one provider turn into
///     several tool-call messages (interleaved with tool results) that each
///     carry a copy of the turn's signed thinking.
///
/// The `seen` set spans the whole conversation. A signed block carries a
/// cryptographic signature unique to its generation, so an exact (text +
/// signature) match can only be the same turn's thinking copied onto split
/// messages — never two genuinely distinct thoughts. Unsigned reasoning
/// summaries are left untouched, since providers like Kimi/DeepSeek require
/// them echoed on every tool-call message.
///
/// This runs before any provider formatter, so it covers every Claude transport
/// (direct Anthropic, Bedrock, Databricks, Vertex) in one place.
fn dedupe_signed_thinking(messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    let mut issues = Vec::new();
    let mut seen: Vec<MessageContentBlock> = Vec::new();

    let fixed_messages = messages
        .into_iter()
        .map(|mut message| {
            if message.role != Role::Assistant {
                return message;
            }

            let original_len = message.content.len();
            let mut deduped: Vec<MessageContentBlock> = Vec::with_capacity(original_len);
            for content in &message.content {
                let is_signed = is_signed_thinking(content);
                if is_signed && seen.contains(content) {
                    continue;
                }
                if is_signed {
                    seen.push(content.clone());
                }
                deduped.push(content.clone());
            }

            if deduped.len() != original_len {
                issues.push("Removed duplicate signed thinking block".to_string());
                message.content = deduped;
            }
            message
        })
        .collect();

    (fixed_messages, issues)
}

pub const TURN_CONTEXT_TAG: &str = "turn-context";
pub const CURRENT_TIME_TAG: &str = "current-time";
pub const WORKING_DIRECTORY_TAG: &str = "working-directory";

fn fix_lead_trail(mut messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    let mut issues = Vec::new();

    if let Some(first) = messages.first()
        && first.role == Role::Assistant
    {
        messages.remove(0);
        issues.push("Removed leading assistant message".to_string());
    }

    if let Some(last) = messages.last()
        && last.role == Role::Assistant
    {
        messages.pop();
        issues.push("Removed trailing assistant message".to_string());
    }

    (messages, issues)
}

const PLACEHOLDER_USER_MESSAGE: &str = "Hello";

fn populate_if_empty(mut messages: Vec<Message>) -> (Vec<Message>, Vec<String>) {
    let mut issues = Vec::new();

    if messages.is_empty() {
        issues.push("Added placeholder user message to empty conversation".to_string());
        messages.push(Message::user().with_text(PLACEHOLDER_USER_MESSAGE));
    }
    (messages, issues)
}

pub fn debug_conversation_fix(
    messages: &[Message],
    fixed: &[Message],
    issues: &[String],
) -> String {
    let mut output = String::new();

    output.push_str("=== CONVERSATION FIX DEBUG ===\n\n");

    output.push_str("BEFORE:\n");
    for (i, msg) in messages.iter().enumerate() {
        output.push_str(&format!("  [{}] {}\n", i, msg.debug()));
    }

    output.push_str("\nISSUES FOUND:\n");
    if issues.is_empty() {
        output.push_str("  (none)\n");
    } else {
        for issue in issues {
            output.push_str(&format!("  - {}\n", issue));
        }
    }

    output.push_str("\nAFTER:\n");
    for (i, msg) in fixed.iter().enumerate() {
        output.push_str(&format!("  [{}] {}\n", i, msg.debug()));
    }

    output.push_str("\n==============================\n");
    output
}

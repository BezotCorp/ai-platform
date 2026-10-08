use anyhow::Result;
use bcaip_provider_types::conversations::{ProviderUsage, Usage as TokenUsage};

use crate::agents::state_machine::BcaipEffect;
use crate::providers::canonical_cost::resolve_usage_cost;
use crate::session::{Session, SessionManager};
use bcaip_agent::operation::ConversationEffect;
use bcaip_provider_types::conversations::{Conversation, MessageUsage};

fn attach_to_last_assistant(effects: &mut [BcaipEffect], usage: &ProviderUsage) {
    let Some(message) = effects.iter_mut().rev().find_map(|effect| match effect {
        BcaipEffect::Conversation(ConversationEffect::AppendMessage(message))
            if message.role == rmcp::model::Role::Assistant && message.error_kind().is_none() =>
        {
            Some(message)
        }
        _ => None,
    }) else {
        return;
    };
    message.metadata.usage = Some(Box::new(MessageUsage::from_provider_usage(usage, false)));
}

pub(crate) fn enrich(session: &Session, effects: &mut [BcaipEffect]) {
    for index in 0..effects.len() {
        let (usage, replaces_conversation) = match &effects[index] {
            BcaipEffect::RecordUsage(usage) => (usage.clone(), false),
            BcaipEffect::CompactConversation {
                usage: Some(usage), ..
            } => (usage.clone(), true),
            _ => continue,
        };
        let (cost, cost_source) = resolve_usage_cost(session.provider_name.as_deref(), &usage);

        let mut enriched = usage.clone();
        enriched.cost = cost;
        enriched.cost_source = cost_source;

        if !replaces_conversation {
            attach_to_last_assistant(effects, &enriched);
        }
        match &mut effects[index] {
            BcaipEffect::RecordUsage(usage) => *usage = enriched,
            BcaipEffect::CompactConversation { usage, .. } => *usage = Some(enriched),
            _ => {}
        }
    }
}

pub(crate) async fn record(
    session_manager: &SessionManager,
    session: &Session,
    usage: &ProviderUsage,
    replaces_conversation: bool,
) -> Result<()> {
    let ledger = MessageUsage::from_provider_usage(usage, replaces_conversation);
    session_manager
        .record_usage_metrics(
            &session.id,
            session.schedule_id.clone(),
            usage.usage,
            &usage.model,
            &ledger,
        )
        .await?;
    Ok(())
}

pub(crate) async fn estimate_context(conversation: &Conversation) -> Result<TokenUsage> {
    let tokens = crate::context_mgmt::count_context_tokens(conversation.messages()).await?;
    Ok(TokenUsage::new(Some(tokens), None, Some(tokens)))
}

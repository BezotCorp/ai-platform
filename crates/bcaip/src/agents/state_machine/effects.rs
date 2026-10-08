use crate::recipe::Recipe;
use crate::session::ExtensionData;
use bcaip_agent::operation::{ConversationEffect, MachineEffect};
use bcaip_provider_types::conversations::ProviderUsage;
use bcaip_provider_types::conversations::{Conversation, Message};

pub enum BcaipEffect {
    Conversation(ConversationEffect),
    CompactConversation {
        conversation: Conversation,
        usage: Option<ProviderUsage>,
    },
    SetRecipe(Box<Option<Recipe>>),
    SetExtensionData(ExtensionData),
    RecordUsage(ProviderUsage),
}

impl MachineEffect for BcaipEffect {
    fn ensure_message_ids(&mut self) {
        match self {
            BcaipEffect::Conversation(effect) => effect.ensure_message_ids(),
            BcaipEffect::CompactConversation { conversation, .. } => {
                for message in conversation.messages_mut() {
                    if message.id.is_none() {
                        message.id = Some(format!("msg_{}", uuid::Uuid::new_v4()));
                    }
                }
            }
            _ => {}
        }
    }
}

impl From<ConversationEffect> for BcaipEffect {
    fn from(effect: ConversationEffect) -> Self {
        BcaipEffect::Conversation(effect)
    }
}

impl From<Message> for BcaipEffect {
    fn from(message: Message) -> Self {
        ConversationEffect::from(message).into()
    }
}

impl From<Conversation> for BcaipEffect {
    fn from(conversation: Conversation) -> Self {
        ConversationEffect::from(conversation).into()
    }
}

use crate::Conversation;
use thiserror::Error;
#[derive(Error, Debug)]
#[error("invalid conversation: {reason}")]
pub struct InvalidConversation {
    reason: String,
    conversation: Conversation,
}

impl InvalidConversation {
    pub fn new(reason: String, conversation: Conversation) -> Self {
        Self {
            reason,
            conversation,
        }
    }

    pub fn reason(&self) -> &str {
        &self.reason
    }

    pub fn conversation(&self) -> &Conversation {
        &self.conversation
    }
}

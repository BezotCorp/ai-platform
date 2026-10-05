use crate::Message;
use rmcp::model::Role;


#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EffectiveRole {
    User,
    Assistant,
    Tool,
}

impl std::fmt::Display for EffectiveRole {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::User => write!(f, "user"),
            Self::Assistant => write!(f, "assistant"),
            Self::Tool => write!(f, "tool"),
        }
    }
}

pub fn effective_role(message: &Message) -> EffectiveRole {
    if message.role == Role::User && Message::has_tool_response(message) {
        EffectiveRole::Tool
    } else {
        match message.role {
            Role::User => EffectiveRole::User,
            Role::Assistant => EffectiveRole::Assistant,
        }
    }
}

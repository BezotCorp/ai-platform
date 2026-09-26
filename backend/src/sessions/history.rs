use anyhow::{Result, bail};
use serde::Serialize;

use crate::sessions::{Message, Session};

#[derive(Clone, Debug, Serialize)]
pub(crate) struct History {
    pub(crate) session: Session,
    pub(crate) messages: Vec<Message>,
}

impl History {
    pub(crate) fn validate(messages: &[Message]) -> Result<()> {
        if messages.len() > 32 {
            bail!("Historique trop long");
        }
        let mut bytes = 0usize;
        for message in messages {
            if !matches!(message.role.as_str(), "user" | "assistant") {
                bail!("Rôle de message non autorisé");
            }
            if message.content.trim().is_empty() || message.content.len() > 32_768 {
                bail!("Contenu de message invalide");
            }
            bytes = bytes
                .checked_add(message.content.len())
                .ok_or_else(|| anyhow::anyhow!("Conversation trop volumineuse"))?;
        }
        if bytes > 131_072 {
            bail!("Conversation trop volumineuse");
        }

        Ok(())
    }
}

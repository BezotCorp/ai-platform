use anyhow::{Result, bail};
use serde::Deserialize;

use crate::{
    agents::ExecutionMode,
    sessions::{History, Message},
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunRequest {
    #[serde(rename = "type")]
    pub command: String,
    pub request_id: String,
    pub messages: Vec<Message>,
    pub mode: ExecutionMode,
}

impl RunRequest {
    pub(crate) fn validate_messages(&self) -> Result<()> {
        if self.command != "run.start"
            || self.request_id.is_empty()
            || self.request_id.len() > 128
            || self.messages.is_empty()
            || self.messages.len() > 32
        {
            bail!("Demande d'exécution invalide");
        }
        if self
            .messages
            .last()
            .is_none_or(|message| message.role != "user")
        {
            bail!("Le dernier message doit venir de l'utilisateur");
        }
        History::validate(&self.messages)?;
        Ok(())
    }

    pub(crate) fn into_mode(self) -> Result<ExecutionMode> {
        self.mode.validate()
    }
}

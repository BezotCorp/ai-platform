use anyhow::{Result, bail};
use serde::Deserialize;

use crate::{
    agents::orchestration::{
        aggregation::Aggregation,
        agent_layer::AgentLayer,
        execution_mode::ExecutionMode,
        mixture::Mixture,
    },
    sessions::message::Message,
};

use super::{
    agent_spec::AgentSpec,
    run_mode::RunMode,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct RunRequest {
    #[serde(rename = "type")]
    pub command: String,

    pub request_id: String,
    pub messages: Vec<Message>,
    pub mode: RunMode,
}

impl RunRequest {
    pub(crate) fn validate_messages(
        &self,
    ) -> Result<()> {
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
            bail!(
                "Le dernier message doit venir de l'utilisateur"
            );
        }

        let mut total = 0usize;

        for message in &self.messages {
            if !matches!(
                message.role.as_str(),
                "user" | "assistant"
            ) {
                bail!("Rôle de message non autorisé");
            }

            if message.content.trim().is_empty()
                || message.content.len() > 32_768
            {
                bail!("Contenu du message invalide");
            }

            total = total
                .checked_add(message.content.len())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Conversation trop volumineuse"
                    )
                })?;
        }

        if total > 131_072 {
            bail!("Conversation trop volumineuse");
        }

        Ok(())
    }

    pub(crate) fn into_mode(
        self,
    ) -> Result<ExecutionMode> {
        match self.mode {
            RunMode::Single { agent } => {
                Ok(
                    ExecutionMode::Single(
                        agent.into_config()?
                    )
                )
            }

            RunMode::Mixture { mixture } => {
                if mixture.layers.is_empty()
                    || mixture.layers.len() > 6
                {
                    bail!("Nombre de couches MoA invalide");
                }

                let mut layers = Vec::new();

                for specs in mixture.layers {
                    if specs.len() > 8 {
                        bail!(
                            "Trop d'agents dans une couche"
                        );
                    }

                    let agents = specs
                        .into_iter()
                        .map(AgentSpec::into_config)
                        .collect::<Result<Vec<_>>>()?;

                    let layer = AgentLayer::new(agents)
                        .map_err(anyhow::Error::msg)?;

                    layers.push(layer);
                }

                let aggregation = Aggregation {
                    agent: mixture
                        .aggregator
                        .into_config()?,
                };

                let mixture = Mixture::new(
                    layers,
                    aggregation,
                )
                .map_err(anyhow::Error::msg)?;

                Ok(ExecutionMode::Mixture(mixture))
            }
        }
    }
}

use anyhow::{Result, bail};
use serde::Deserialize;

use crate::{
    agents::{
        AgentLayer, Aggregation, ExecutionMode, LayeredMoa, MultiAgent, MultiAgentStrategy,
        Supervised,
    },
    sessions::{History, Message},
    websocket::{CoordinationSpec, RunMode},
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
        match self.mode {
            RunMode::Single { agent } => Ok(ExecutionMode::Single(agent)),
            RunMode::MultiAgent {
                coordination,
                population,
            } => {
                let strategy = match coordination {
                    CoordinationSpec::LayeredMoa {
                        layers: specs,
                        aggregator,
                    } => {
                        if specs.is_empty() || specs.len() > 6 {
                            bail!("Nombre de couches MoA invalide");
                        }
                        let mut layers = Vec::with_capacity(specs.len());
                        for agents in specs {
                            if agents.len() > 8 {
                                bail!("Trop d'agents dans une couche");
                            }
                            layers.push(AgentLayer::new(agents).map_err(anyhow::Error::msg)?);
                        }
                        let moa = LayeredMoa::new(
                            layers,
                            Aggregation { agent: aggregator },
                        )
                        .map_err(anyhow::Error::msg)?;
                        MultiAgentStrategy::LayeredMoa(moa)
                    }
                    CoordinationSpec::Supervised {
                        supervisor,
                        workers,
                        max_delegations,
                    } => {
                        let supervised = Supervised::new(
                            supervisor,
                            workers,
                            max_delegations,
                        )
                        .map_err(anyhow::Error::msg)?;
                        MultiAgentStrategy::Supervised(supervised)
                    }
                };
                let multi = MultiAgent::new(strategy, population.into())
                    .map_err(anyhow::Error::msg)?;
                Ok(ExecutionMode::MultiAgent(multi))
            }
        }
    }
}

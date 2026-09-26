use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::agents::{
    AgentConfig, LayeredMoa, MultiAgent, MultiAgentStrategy, Population, Supervised,
};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum ExecutionMode {
    Single { agent: AgentConfig },
    MultiAgent(MultiAgent),
}

impl ExecutionMode {
    pub(crate) fn validate(self) -> Result<Self> {
        match self {
            Self::Single { .. } => Ok(self),
            Self::MultiAgent(multi) => {
                let strategy = match multi.strategy {
                    MultiAgentStrategy::LayeredMoa(config) => {
                        if config.layers.is_empty()
                            || config.layers.len() > 6
                            || config.layers.iter().any(|layer| layer.agents.len() > 8)
                        {
                            bail!("Invalid layered MoA configuration");
                        }
                        let config = LayeredMoa::new(config.layers, config.aggregation)
                            .map_err(anyhow::Error::msg)?;
                        MultiAgentStrategy::LayeredMoa(config)
                    }
                    MultiAgentStrategy::Supervised(config) => {
                        let config = Supervised::new(
                            config.supervisor,
                            config.workers,
                            config.max_delegations,
                        )
                        .map_err(anyhow::Error::msg)?;
                        MultiAgentStrategy::Supervised(config)
                    }
                    MultiAgentStrategy::Population(config) => {
                        let config = Population::new(
                            config.agents,
                            config.facilitator,
                            config.rounds,
                            config.participation,
                        )
                        .map_err(anyhow::Error::msg)?;
                        MultiAgentStrategy::Population(config)
                    }
                };
                Ok(Self::MultiAgent(MultiAgent::new(strategy)))
            }
        }
    }
}

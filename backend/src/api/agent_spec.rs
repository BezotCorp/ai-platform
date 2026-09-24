use anyhow::{Result, bail};
use serde::Deserialize;

use crate::{
    agents::{
        agent_config::AgentConfig,
        agent_identity::AgentIdentity,
        agent_role::AgentRole,
    },
    providers::model::Model,
};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentSpec {
    pub id: String,
    pub role: String,
    pub instructions: String,
    pub provider: String,
    pub model: String,
}

impl AgentSpec {
    pub(crate) fn into_config(
        self,
    ) -> Result<AgentConfig> {
        if self.provider != "ollama" {
            bail!("Fournisseur non connecté");
        }

        if self.instructions.len() > 8192 {
            bail!("Instructions trop volumineuses");
        }

        Ok(AgentConfig {
            identity: AgentIdentity::new(self.id)
                .map_err(anyhow::Error::msg)?,

            role: AgentRole::new(
                self.role,
                self.instructions,
            )
            .map_err(anyhow::Error::msg)?,

            model: Model::new(
                self.provider,
                self.model,
            )
            .map_err(anyhow::Error::msg)?,
        })
    }
}

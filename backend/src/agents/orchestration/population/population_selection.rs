use std::collections::HashSet;

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::agents::AgentConfig;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PopulationSelection {
    pub agent_ids: Vec<String>,
}

impl PopulationSelection {
    pub(crate) fn parse(
        response: &str,
        agents: &[AgentConfig],
        minimum: usize,
        maximum: usize,
    ) -> Result<Self> {
        if response.len() > 4096 {
            bail!("Population selection response too large");
        }

        let selection: Self = serde_json::from_str(response)?;

        if selection.agent_ids.len() < minimum
            || selection.agent_ids.len() > maximum
        {
            bail!("Adaptive population size outside configured bounds");
        }

        let available: HashSet<&str> = agents
            .iter()
            .map(|agent| agent.identity.id.as_str())
            .collect();

        let mut selected = HashSet::new();

        for identifier in &selection.agent_ids {
            if !available.contains(identifier.as_str()) {
                bail!("Unknown population participant");
            }

            if !selected.insert(identifier.as_str()) {
                bail!("Duplicate selected participant");
            }
        }

        Ok(selection)
    }
}

use anyhow::{Result, bail};

use crate::agents::{AgentConfig, ExecutionMode, MultiAgentStrategy};

pub(crate) struct Scheduler;

impl Scheduler {
    /// A fixed execution plan for modes that support one. Future autonomous
    /// coordinators must use their own runtime, not emulate a layered MoA.
    pub(crate) fn plan(mode: &ExecutionMode) -> Result<Vec<Vec<&AgentConfig>>> {
        match mode {
            ExecutionMode::Single(agent) => Ok(vec![vec![agent]]),
            ExecutionMode::MultiAgent(multi) => match &multi.strategy {
                MultiAgentStrategy::LayeredMoa(moa) => {
                    let mut layers: Vec<Vec<&AgentConfig>> = moa
                        .layers
                        .iter()
                        .map(|layer| layer.agents.iter().collect())
                        .collect();
                    layers.push(vec![&moa.aggregation.agent]);
                    Ok(layers)
                }
                MultiAgentStrategy::Supervised(_) => {
                    bail!("Supervised orchestration has a dynamic runtime")
                }
            },
        }
    }
}

use super::{
    super::agent_config::AgentConfig,
    execution_mode::ExecutionMode,
};

pub(crate) struct Scheduler;

impl Scheduler {
    /// Returns ordered execution layers.
    ///
    /// Agents within a layer are independent.
    /// The aggregator runs after all proposal layers.
    ///
    /// This is a plan, not an execution engine.
    pub fn plan(
        mode: &ExecutionMode,
    ) -> Vec<Vec<&AgentConfig>> {
        match mode {
            ExecutionMode::Single(agent) => {
                vec![vec![agent]]
            }

            ExecutionMode::Mixture(mixture) => {
                let mut layers: Vec<Vec<&AgentConfig>> =
                    mixture.layers.iter()
                        .map(|layer| layer.agents.iter().collect())
                        .collect();

                layers.push(vec![&mixture.aggregation.agent]);

                layers
            }
        }
    }
}

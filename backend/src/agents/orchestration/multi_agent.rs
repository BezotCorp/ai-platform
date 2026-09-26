use crate::agents::{MultiAgentStrategy, PopulationPolicy};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct MultiAgent {
    pub strategy: MultiAgentStrategy,
    pub population: PopulationPolicy,
}

impl MultiAgent {
    pub(crate) fn new(
        strategy: MultiAgentStrategy,
        population: PopulationPolicy,
    ) -> Result<Self, &'static str> {
        if !matches!(population, PopulationPolicy::Fixed) {
            return Err("Requested population policy is not implemented");
        }
        Ok(Self { strategy, population })
    }
}

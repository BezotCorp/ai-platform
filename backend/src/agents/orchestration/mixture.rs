use std::collections::HashSet;

use super::{
    agent_layer::AgentLayer,
    aggregation::Aggregation,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Mixture {
    pub layers: Vec<AgentLayer>,
    pub aggregation: Aggregation,
}

impl Mixture {
    pub fn new(
        layers: Vec<AgentLayer>,
        aggregation: Aggregation,
    ) -> Result<Self, &'static str> {
        if layers.is_empty() {
            return Err("A mixture requires at least one layer");
        }

        let mut identifiers = HashSet::new();

        for layer in &layers {
            if layer.agents.is_empty() {
                return Err("An agent layer cannot be empty");
            }

            for agent in &layer.agents {
                if !identifiers.insert(&agent.identity.id) {
                    return Err("Duplicate agent identifier");
                }
            }
        }

        if !identifiers.insert(&aggregation.agent.identity.id) {
            return Err("Duplicate aggregator identifier");
        }

        Ok(Self {
            layers,
            aggregation,
        })
    }
}

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::agents::{AgentLayer, Aggregation};

/// Fixed proposal layers followed by an aggregator. This is not a generic
/// name for every multi-agent coordination strategy.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LayeredMoa {
    pub layers: Vec<AgentLayer>,
    #[serde(rename = "aggregator")]
    pub aggregation: Aggregation,
}

impl LayeredMoa {
    pub(crate) fn new(
        layers: Vec<AgentLayer>,
        aggregation: Aggregation,
    ) -> Result<Self, &'static str> {
        if layers.is_empty() {
            return Err("A layered MoA requires at least one layer");
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

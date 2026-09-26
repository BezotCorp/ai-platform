use std::collections::HashSet;

use crate::agents::AgentConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Supervised {
    pub supervisor: AgentConfig,
    pub workers: Vec<AgentConfig>,
    pub max_delegations: usize,
}

impl Supervised {
    pub(crate) fn new(supervisor: AgentConfig, workers: Vec<AgentConfig>, max_delegations: usize) -> Result<Self, &'static str> {
        if workers.is_empty() || workers.len() > 8 { return Err("Supervised execution requires one to eight workers"); }
        if !(1..=12).contains(&max_delegations) { return Err("Invalid delegation limit"); }
        let mut identifiers = HashSet::new();
        identifiers.insert(&supervisor.identity.id);
        for worker in &workers {
            if !identifiers.insert(&worker.identity.id) { return Err("Duplicate agent identifier"); }
        }
        Ok(Self { supervisor, workers, max_delegations })
    }
}

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::agents::{
    AgentConfig,
    ParticipationPolicy,
};

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
)]
#[serde(deny_unknown_fields)]
pub(crate) struct Population {
    pub agents: Vec<AgentConfig>,
    pub facilitator: AgentConfig,
    pub rounds: usize,

    #[serde(default)]
    pub participation: ParticipationPolicy,
}

impl Population {
    pub(crate) fn new(
        agents: Vec<AgentConfig>,
        facilitator: AgentConfig,
        rounds: usize,
        participation: ParticipationPolicy,
    ) -> Result<Self, &'static str> {
        if !(2..=4).contains(&agents.len()) {
            return Err(
                "A population requires two to four agents"
            );
        }

        if !(1..=4).contains(&rounds) {
            return Err("Invalid population round count");
        }

        let mut identifiers = HashSet::new();
        identifiers.insert(&facilitator.identity.id);

        for agent in &agents {
            if !identifiers.insert(&agent.identity.id) {
                return Err(
                    "Duplicate population agent identifier"
                );
            }
        }

        if let ParticipationPolicy::Adaptive {
            min_agents,
            max_agents,
        } = &participation
        {
            if *min_agents == 0
                || min_agents > max_agents
                || *max_agents > agents.len()
            {
                return Err(
                    "Invalid adaptive population bounds"
                );
            }
        }

        Ok(Self {
            agents,
            facilitator,
            rounds,
            participation,
        })
    }
}

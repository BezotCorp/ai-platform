use serde::Deserialize;

use crate::agents::PopulationPolicy;

/// Future policies can be requested explicitly, but requests for them are
/// rejected until the matching runtime is implemented.
#[derive(Debug, Default, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum PopulationSpec {
    #[default]
    Fixed,
    Adaptive {
        min_agents: usize,
        max_agents: usize,
    },
    Evolutionary {
        population_size: usize,
        generations: usize,
    },
}

impl From<PopulationSpec> for PopulationPolicy {
    fn from(spec: PopulationSpec) -> Self {
        match spec {
            PopulationSpec::Fixed => Self::Fixed,
            PopulationSpec::Adaptive { min_agents, max_agents } => {
                Self::Adaptive { min_agents, max_agents }
            }
            PopulationSpec::Evolutionary { population_size, generations } => {
                Self::Evolutionary { population_size, generations }
            }
        }
    }
}

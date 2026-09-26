use serde::Deserialize;

use crate::{
    agents::AgentConfig,
    websocket::{CoordinationSpec, PopulationSpec},
};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum RunMode {
    Single {
        agent: AgentConfig,
    },
    MultiAgent {
        coordination: CoordinationSpec,
        #[serde(default)]
        population: PopulationSpec,
    },
}

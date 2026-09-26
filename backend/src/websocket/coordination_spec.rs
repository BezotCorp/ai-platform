use serde::Deserialize;

use crate::agents::AgentConfig;

/// Only concrete coordination strategies that the runtime supports are
/// accepted by the transport. Other strategies are added when implemented.
#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum CoordinationSpec {
    LayeredMoa {
        layers: Vec<Vec<AgentConfig>>,
        aggregator: AgentConfig,
    },
    Supervised {
        supervisor: AgentConfig,
        workers: Vec<AgentConfig>,
        max_delegations: usize,
    },
}

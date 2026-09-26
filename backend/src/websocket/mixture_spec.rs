use serde::Deserialize;

use crate::agents::AgentConfig;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MixtureSpec {
    pub layers: Vec<Vec<AgentConfig>>,
    pub aggregator: AgentConfig,
}

use serde::Deserialize;

use crate::api::AgentSpec;

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MixtureSpec {
    pub layers: Vec<Vec<AgentSpec>>,
    pub aggregator: AgentSpec,
}

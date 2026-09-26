use serde::{Deserialize, Serialize};

use crate::agents::AgentConfig;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct Aggregation {
    pub agent: AgentConfig,
}

use serde::{Deserialize, Serialize};

use crate::agents::MultiAgentStrategy;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct MultiAgent {
    #[serde(rename = "coordination")]
    pub strategy: MultiAgentStrategy,
}

impl MultiAgent {
    pub(crate) fn new(strategy: MultiAgentStrategy) -> Self {
        Self { strategy }
    }
}

use serde::{Deserialize, Serialize};

use crate::agents::{LayeredMoa, Population, Supervised};

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum MultiAgentStrategy {
    LayeredMoa(LayeredMoa),
    Supervised(Supervised),
    Population(Population),
}

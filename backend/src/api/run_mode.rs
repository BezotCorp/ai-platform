use serde::Deserialize;

use crate::api::{AgentSpec, MixtureSpec};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RunMode {
    Single { agent: AgentSpec },
    Mixture { mixture: MixtureSpec },
}

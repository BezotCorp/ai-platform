use serde::Deserialize;

use super::{
    agent_spec::AgentSpec,
    mixture_spec::MixtureSpec,
};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RunMode {
    Single {
        agent: AgentSpec,
    },

    Mixture {
        mixture: MixtureSpec,
    },
}

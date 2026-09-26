use serde::Deserialize;

use crate::{agents::AgentConfig, websocket::MixtureSpec};

#[derive(Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum RunMode {
    Single { agent: AgentConfig },
    Mixture { mixture: MixtureSpec },
}

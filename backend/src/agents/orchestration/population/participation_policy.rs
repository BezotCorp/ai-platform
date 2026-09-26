use serde::{Deserialize, Serialize};

#[derive(
    Debug,
    Clone,
    Default,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub(crate) enum ParticipationPolicy {
    #[default]
    Fixed,

    Adaptive {
        min_agents: usize,
        max_agents: usize,
    },
}

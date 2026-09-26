use crate::agents::{LayeredMoa, Supervised};

/// Coordination is separate from population policy and resource scheduling.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MultiAgentStrategy {
    LayeredMoa(LayeredMoa),
    Supervised(Supervised),
}

use crate::agents::{AgentConfig, Mixture};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionMode {
    Single(AgentConfig),
    Mixture(Mixture),
}

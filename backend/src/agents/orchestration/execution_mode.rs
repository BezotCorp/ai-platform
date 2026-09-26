use crate::agents::{AgentConfig, MultiAgent};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionMode {
    Single(AgentConfig),
    MultiAgent(MultiAgent),
}

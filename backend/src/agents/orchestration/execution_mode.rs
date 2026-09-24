use super::{
    super::agent_config::AgentConfig,
    mixture::Mixture,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ExecutionMode {
    Single(AgentConfig),
    Mixture(Mixture),
}

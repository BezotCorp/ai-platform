use crate::{
    agents::{AgentIdentity, agent_role::AgentRole},
    providers::Model,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentConfig {
    pub identity: AgentIdentity,
    pub role: AgentRole,
    pub model: Model,
}

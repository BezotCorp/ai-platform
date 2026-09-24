use crate::providers::model::Model;

use super::{
    agent_identity::AgentIdentity,
    agent_role::AgentRole,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentConfig {
    pub identity: AgentIdentity,
    pub role: AgentRole,
    pub model: Model,
}

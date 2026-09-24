use super::super::agent_config::AgentConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Aggregation {
    pub agent: AgentConfig,
}

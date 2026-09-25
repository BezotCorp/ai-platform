use crate::agents::AgentConfig;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentLayer {
    pub agents: Vec<AgentConfig>,
}

impl AgentLayer {
    pub fn new(agents: Vec<AgentConfig>) -> Result<Self, &'static str> {
        if agents.is_empty() {
            return Err("An agent layer cannot be empty");
        }
        Ok(Self { agents })
    }
}

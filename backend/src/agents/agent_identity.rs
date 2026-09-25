#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct AgentIdentity {
    pub id: String,
}

impl AgentIdentity {
    pub fn new(id: String) -> Result<Self, &'static str> {
        if id.trim().is_empty() {
            return Err("Agent identifier cannot be empty");
        }
        Ok(Self { id })
    }
}

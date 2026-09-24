#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct AgentRole {
    pub name: String,
    pub instructions: String,
}

impl AgentRole {
    pub fn new(name: String, instructions: String) -> Result<Self, &'static str> {
        if name.trim().is_empty() {
            return Err("Agent role cannot be empty");
        }

        Ok(Self { name, instructions })
    }
}

use anyhow::{Result, bail};
use serde::Deserialize;

/// The supervisor may delegate one task or finish the run.
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum SupervisorDecision {
    Delegate { agent_id: String, task: String },
    Complete { answer: String },
}

impl SupervisorDecision {
    pub(crate) fn parse(input: &str) -> Result<Self> {
        if input.len() > 16 * 1024 {
            bail!("Supervisor response too large");
        }
        let decision: Self = serde_json::from_str(input)?;
        match &decision {
            Self::Delegate { agent_id, task } => {
                if agent_id.trim().is_empty() || task.trim().is_empty() || task.len() > 4096 {
                    bail!("Invalid delegation");
                }
            }
            Self::Complete { answer } => {
                if answer.trim().is_empty() || answer.len() > 8192 {
                    bail!("Invalid completion");
                }
            }
        }
        Ok(decision)
    }
}

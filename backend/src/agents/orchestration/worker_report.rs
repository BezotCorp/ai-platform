use serde::Serialize;

/// Unverified worker output returned to the supervisor.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct WorkerReport {
    pub agent_id: String,
    pub task: String,
    pub answer: String,
}

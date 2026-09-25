use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::{
    providers::Client,
    tools::ToolApprovalGate,
};

#[derive(Clone)]
pub(crate) struct ServerState {
    pub client: Client,
    pub token: Arc<str>,
    pub origin: Arc<str>,
    pub gpu: Arc<Semaphore>,
    pub project_root: Arc<std::path::PathBuf>,
    pub approvals: ToolApprovalGate,
    pub approve_reads: bool,
}

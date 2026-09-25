use std::sync::Arc;
use tokio::sync::{Mutex, Semaphore};

use crate::{agents::MemoryStore, providers::Client};

#[derive(Clone)]
pub(crate) struct ServerState {
    pub client: Client,
    pub token: Arc<str>,
    pub origin: Arc<str>,
    pub gpu: Arc<Semaphore>,
    pub project_root: Arc<std::path::PathBuf>,
    pub writes: Arc<Mutex<()>>,
    pub approve_reads: bool,
    pub memory: Option<MemoryStore>,
}

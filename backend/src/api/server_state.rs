use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::providers::ollama::client::Client;

#[derive(Clone)]
pub(crate) struct ServerState {
    pub client: Client,
    pub token: Arc<str>,
    pub origin: Arc<str>,
    pub gpu: Arc<Semaphore>,
}

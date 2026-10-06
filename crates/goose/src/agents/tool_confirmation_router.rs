use bcaip_provider_types::permission::PermissionConfirmation;
use std::collections::HashMap;
use tokio::sync::{Mutex, oneshot};
use tracing::warn;
pub(super) struct ToolConfirmationRouter {
    pending: Mutex<HashMap<(String, String), oneshot::Sender<PermissionConfirmation>>>,
}

impl ToolConfirmationRouter {
    pub(super) fn new() -> Self {
        Self {
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub(super) async fn register(
        &self,
        session_id: String,
        request_id: String,
    ) -> oneshot::Receiver<PermissionConfirmation> {
        let (tx, rx) = oneshot::channel();
        let mut pending = self.pending.lock().await;
        pending.retain(|_, sender| !sender.is_closed());
        pending.insert((session_id, request_id), tx);
        rx
    }

    pub(super) async fn deliver(
        &self,
        session_id: &str,
        request_id: &str,
        confirmation: PermissionConfirmation,
    ) -> bool {
        let key = (session_id.to_string(), request_id.to_string());
        if let Some(tx) = self.pending.lock().await.remove(&key) {
            if tx.send(confirmation).is_err() {
                warn!(
                    request_id = %request_id,
                    "Confirmation receiver was dropped (task cancelled)"
                );
                false
            } else {
                true
            }
        } else {
            false
        }
    }
}

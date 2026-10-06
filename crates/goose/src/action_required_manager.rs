use anyhow::Result;
use bcaip_provider_types::conversations::{Message, MessageContent};
use serde_json::Value;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock, mpsc};
use tokio::time::timeout;
use tracing::warn;
use uuid::Uuid;

const ACTION_REQUIRED_STREAM_CAPACITY: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ElicitationOutcome {
    Accept(Value),
    Decline,
    Cancel,
}

struct PendingRequest {
    session_id: String,
    response_tx: Option<tokio::sync::oneshot::Sender<ElicitationOutcome>>,
}

pub(crate) struct PendingResponseClaim {
    request_id: String,
    pending: OwnedMutexGuard<PendingRequest>,
}

impl PendingResponseClaim {
    pub(crate) fn submit(mut self, response: ElicitationOutcome) -> Result<()> {
        let tx = self
            .pending
            .response_tx
            .take()
            .ok_or_else(|| anyhow::anyhow!("Request already completed: {}", self.request_id))?;
        drop(self.pending);

        if tx.send(response).is_err() {
            return Err(anyhow::anyhow!("Response channel closed"));
        }

        Ok(())
    }
}

pub(crate) struct ActionRequiredManager {
    pending: Arc<RwLock<HashMap<String, Arc<Mutex<PendingRequest>>>>>,
    action_required_senders: Mutex<HashMap<(String, String), mpsc::Sender<Message>>>,
}

impl ActionRequiredManager {
    pub(crate) fn new() -> Self {
        Self {
            pending: Arc::new(RwLock::new(HashMap::new())),
            action_required_senders: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) async fn request_and_wait(
        &self,
        session_id: String,
        tool_call_request_id: String,
        message: String,
        schema: Value,
        timeout_duration: Duration,
    ) -> Result<ElicitationOutcome> {
        let sender = self
            .action_required_senders
            .lock()
            .await
            .get(&(session_id.clone(), tool_call_request_id.clone()))
            .cloned();

        let Some(sender) = sender else {
            return Err(anyhow::anyhow!(
                "Tool call request not found for elicitation: {}",
                tool_call_request_id
            ));
        };

        let id = Uuid::new_v4().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();
        let pending_request = PendingRequest {
            session_id: session_id.clone(),
            response_tx: Some(tx),
        };
        let pending_request = Arc::new(Mutex::new(pending_request));

        self.pending
            .write()
            .await
            .insert(id.clone(), Arc::clone(&pending_request));

        let action_required_message = Message::assistant().with_content(
            MessageContent::action_required_elicitation(id.clone(), message, schema),
        );
        if let Err(error) = sender.try_send(action_required_message) {
            self.pending.write().await.remove(&id);
            let message = match error {
                mpsc::error::TrySendError::Full(_) => "stream is full",
                mpsc::error::TrySendError::Closed(_) => "stream closed",
            };
            return Err(anyhow::anyhow!(
                "Tool call action-required {message}: {tool_call_request_id}"
            ));
        }

        let result = self
            .wait_for_response(&id, pending_request, rx, timeout_duration)
            .await;

        self.pending.write().await.remove(&id);

        result
    }

    pub(crate) async fn claim_response(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<PendingResponseClaim> {
        let pending_arc = self.pending_request(request_id).await?;
        let mut pending = pending_arc.lock_owned().await;

        if pending.session_id != session_id {
            return Err(anyhow::anyhow!(
                "Request {} belongs to session {}, not {}",
                request_id,
                pending.session_id,
                session_id
            ));
        }

        let tx = pending
            .response_tx
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Request already completed: {}", request_id))?;
        if tx.is_closed() {
            pending.response_tx.take();
            return Err(anyhow::anyhow!("Response channel closed"));
        }

        Ok(PendingResponseClaim {
            request_id: request_id.to_string(),
            pending,
        })
    }

    async fn pending_request(&self, request_id: &str) -> Result<Arc<Mutex<PendingRequest>>> {
        let pending = self.pending.read().await;
        pending
            .get(request_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Request not found: {}", request_id))
    }

    async fn wait_for_response(
        &self,
        request_id: &str,
        pending_request: Arc<Mutex<PendingRequest>>,
        mut rx: tokio::sync::oneshot::Receiver<ElicitationOutcome>,
        timeout_duration: Duration,
    ) -> Result<ElicitationOutcome> {
        match timeout(timeout_duration, &mut rx).await {
            Ok(response) => Self::finish_waiting(request_id, response),
            Err(_) => {
                let mut pending = pending_request.lock().await;
                if pending.response_tx.is_some() {
                    pending.response_tx.take();
                    warn!("Timeout waiting for response: {}", request_id);
                    return Err(anyhow::anyhow!("Timeout waiting for user response"));
                }
                drop(pending);

                Self::finish_waiting(request_id, rx.await)
            }
        }
    }

    fn finish_waiting(
        request_id: &str,
        response: Result<ElicitationOutcome, tokio::sync::oneshot::error::RecvError>,
    ) -> Result<ElicitationOutcome> {
        match response {
            Ok(user_data) => Ok(user_data),
            Err(_) => {
                warn!("Response channel closed for request: {}", request_id);
                Err(anyhow::anyhow!("Response channel closed"))
            }
        }
    }

    pub(crate) async fn register_action_required_stream(
        &self,
        session_id: String,
        tool_call_request_id: String,
    ) -> mpsc::Receiver<Message> {
        let (tx, rx) = mpsc::channel(ACTION_REQUIRED_STREAM_CAPACITY);
        self.action_required_senders
            .lock()
            .await
            .insert((session_id, tool_call_request_id), tx);
        rx
    }

    pub(crate) async fn has_action_required_stream(
        &self,
        session_id: &str,
        tool_call_request_id: &str,
    ) -> bool {
        self.action_required_senders
            .lock()
            .await
            .contains_key(&(session_id.to_string(), tool_call_request_id.to_string()))
    }

    pub(crate) async fn unregister_action_required_stream(
        &self,
        session_id: &str,
        tool_call_request_id: &str,
    ) {
        self.action_required_senders
            .lock()
            .await
            .remove(&(session_id.to_string(), tool_call_request_id.to_string()));
    }
}

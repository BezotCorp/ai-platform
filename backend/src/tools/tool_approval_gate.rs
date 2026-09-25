use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Result, bail};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use crate::api::Event;

#[derive(Clone, Default)]
pub(crate) struct ToolApprovalGate {
    pending: Arc<Mutex<HashMap<String, oneshot::Sender<bool>>>>,
}

impl ToolApprovalGate {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn key(request_id: &str, call_id: &str) -> String {
        format!("{request_id}:{call_id}")
    }

    pub(crate) async fn resolve(&self, request_id: &str, call_id: &str, approved: bool) -> bool {
        let key = Self::key(request_id, call_id);
        let pending = self.pending.lock().await.remove(&key);
        match pending {
            Some(sender) => sender.send(approved).is_ok(),
            None => false,
        }
    }

    pub(crate) async fn request(
        &self,
        request_id: &str,
        agent_id: &str,
        call_id: &str,
        tool: &str,
        arguments: &Value,
        outbound: &mpsc::Sender<Event>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let key = Self::key(request_id, call_id);
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            if pending.insert(key.clone(), sender).is_some() {
                bail!("Identifiant d'autorisation dupliqué");
            }
        }
        let notification = Event::new(
            "approval.required",
            request_id,
            json!({
                "agent_id": agent_id,
                "call_id": call_id,
                "tool": tool,
                "arguments": arguments,
            }),
        );
        let result = async {
            outbound.send(notification).await?;
            let decision = tokio::select! {
                () = cancel.cancelled() => {
                    bail!("Exécution annulée");
                }
                result = tokio::time::timeout(
                    Duration::from_secs(120),
                    receiver,
                ) => {
                    result??
                }
            };
            if !decision {
                bail!("Autorisation refusée");
            }
            Ok(())
        }
        .await;
        self.pending.lock().await.remove(&key);
        result
    }
}

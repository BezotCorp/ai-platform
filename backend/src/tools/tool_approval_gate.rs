use std::{collections::HashMap, sync::Arc, time::Duration};

use anyhow::{Result, bail};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{
    select,
    sync::{Mutex, mpsc, oneshot},
    time,
};
use tokio_util::sync::CancellationToken;

use crate::event::Event;

type Pending = (Option<String>, oneshot::Sender<bool>);

#[derive(Clone, Default)]
pub(crate) struct ToolApprovalGate {
    pending: Arc<Mutex<HashMap<String, Pending>>>,
}

impl ToolApprovalGate {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    fn key(request_id: &str, call_id: &str) -> String {
        format!("{request_id}:{call_id}")
    }

    pub(crate) fn preview_sha256(preview: &Value) -> Result<String> {
        let encoded = serde_json::to_vec(preview)?;
        Ok(Sha256::digest(encoded)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect())
    }

    pub(crate) async fn resolve(
        &self,
        request_id: &str,
        call_id: &str,
        approved: bool,
        preview_sha256: Option<&str>,
    ) -> bool {
        let key = Self::key(request_id, call_id);
        let mut pending = self.pending.lock().await;
        let Some((expected, _)) = pending.get(&key) else {
            return false;
        };
        if approved && expected.as_deref() != preview_sha256 {
            return false;
        }
        let Some((_, sender)) = pending.remove(&key) else {
            return false;
        };
        drop(pending);
        sender.send(approved).is_ok()
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn request(
        &self,
        request_id: &str,
        agent_id: &str,
        call_id: &str,
        tool: &str,
        arguments: &Value,
        preview_sha256: Option<&str>,
        outbound: &mpsc::Sender<Event>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let key = Self::key(request_id, call_id);
        let (sender, receiver) = oneshot::channel();
        {
            let mut pending = self.pending.lock().await;
            if pending.contains_key(&key) {
                bail!("Identifiant d'autorisation dupliqué");
            }
            pending.insert(key.clone(), (preview_sha256.map(str::to_owned), sender));
        }
        let notification = Event::new(
            "approval.required",
            request_id,
            json!({
                "agent_id": agent_id,
                "call_id": call_id,
                "tool": tool,
                "arguments": arguments,
                "preview_sha256": preview_sha256,
            }),
        );
        let result = async {
            outbound.send(notification).await?;
            let decision = select! {
                () = cancel.cancelled() => {
                    bail!("Exécution annulée");
                }
                result = time::timeout(
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

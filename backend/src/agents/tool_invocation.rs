use std::{path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::{
    event::Event,
    tools::{self, ToolApprovalGate, WriteProposal},
};

pub(crate) struct ToolInvocation;

impl ToolInvocation {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn execute(
        name: &str,
        arguments: &Value,
        project_root: &Path,
        approvals: &ToolApprovalGate,
        approve_reads: bool,
        writes: &Arc<Mutex<()>>,
        request_id: &str,
        agent_id: &str,
        call_id: &str,
        outbound: &mpsc::Sender<Event>,
        cancel: &CancellationToken,
    ) -> Result<Value> {
        let result = if WriteProposal::is_write(name) {
            let proposal = WriteProposal::prepare(project_root, name, arguments).await?;
            let preview = proposal.preview();
            let preview_sha256 = ToolApprovalGate::preview_sha256(&preview)?;
            outbound
                .send(Event::new(
                    "tool.preview",
                    request_id,
                    json!({
                        "agent_id":
                            agent_id,
                        "call_id": call_id,
                        "preview": preview,
                        "preview_sha256":
                            preview_sha256,
                    }),
                ))
                .await?;
            // Une écriture nécessite toujours
            // l'accord explicite du frontend.
            approvals
                .request(
                    request_id,
                    agent_id,
                    call_id,
                    name,
                    arguments,
                    Some(&preview_sha256),
                    outbound,
                    cancel,
                )
                .await?;
            let guard = writes.clone().lock_owned().await;
            if cancel.is_cancelled() {
                bail!("Exécution annulée");
            }
            // Une fois commencée, la
            // publication atomique ne
            // doit pas être interrompue.
            proposal.commit(guard).await
        } else {
            if approve_reads {
                approvals
                    .request(
                        request_id, agent_id, call_id, name, arguments, None, outbound, cancel,
                    )
                    .await?;
            }
            tokio::select! {
                () = cancel.cancelled() => {
                    bail!("Exécution annulée");
                }
                result = tools::execute(
                    project_root,
                    name,
                    arguments,
                ) => result,
            }
        };
        let payload = match result {
            Ok(value) => {
                outbound
                    .send(Event::new(
                        "tool.completed",
                        request_id,
                        json!({
                            "agent_id":
                                agent_id,
                            "call_id": call_id,
                            "tool": name,
                            "result": value,
                        }),
                    ))
                    .await?;
                json!({
                    "ok": true,
                    "result": value,
                })
            }
            Err(error) => {
                outbound
                    .send(Event::new(
                        "tool.failed",
                        request_id,
                        json!({
                            "agent_id":
                                agent_id,
                            "call_id": call_id,
                            "tool": name,
                            "error":
                                error.to_string(),
                        }),
                    ))
                    .await?;
                json!({
                    "ok": false,
                    "error":
                        error.to_string(),
                })
            }
        };
        Ok(payload)
    }
}

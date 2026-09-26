use std::{collections::HashMap, path::Path, sync::Arc};

use anyhow::{Context, Result, bail};
use serde_json::json;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::{
    agents::{
        MemoryStore, Supervised, SupervisorDecision, WorkerReport,
        agent_turn::AgentTurn, context::limits,
        orchestration::supervisor_turn::SupervisorTurn,
    },
    event::Event,
    providers::Client,
    sessions::Message,
    tools::{self, ToolApprovalGate},
};

pub(crate) struct SupervisedExecution;

impl SupervisedExecution {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn run(
        config: &Supervised,
        client: &Client,
        history: &[Message],
        request_id: &str,
        outbound: &mpsc::Sender<Event>,
        cancel: &CancellationToken,
        project_root: &Path,
        approvals: &ToolApprovalGate,
        approve_reads: bool,
        writes: &Arc<Mutex<()>>,
        memory: Option<&MemoryStore>,
    ) -> Result<()> {
        let (context_tokens, output_tokens) = limits()?;
        let definitions = tools::definitions();
        let tool_tokens = serde_json::to_vec(&definitions)?.len().saturating_add(256);
        let mut worker_histories: HashMap<String, Vec<Message>> = HashMap::new();
        let mut reports = Vec::<WorkerReport>::new();

        for step in 0..=config.max_delegations {
            if cancel.is_cancelled() {
                bail!("Exécution annulée");
            }
            outbound
                .send(Event::new(
                    "orchestration.deciding",
                    request_id,
                    json!({
                        "supervisor_id": config.supervisor.identity.id,
                        "step": step,
                    }),
                ))
                .await?;
            let decision = SupervisorTurn::decide(
                config,
                client,
                history,
                &reports,
                context_tokens,
                output_tokens,
                cancel,
            )
            .await?;
            match decision {
                SupervisorDecision::Complete { answer } => {
                    if let (Some(store), Some(prompt)) = (memory, history.last())
                        && let Err(error) = store.remember(&prompt.content, &answer).await
                    {
                        outbound
                            .send(Event::new(
                                "memory.failed",
                                request_id,
                                json!({ "error": error.to_string() }),
                            ))
                            .await?;
                    }
                    outbound
                        .send(Event::new(
                            "run.completed",
                            request_id,
                            json!({ "result": answer }),
                        ))
                        .await?;
                    return Ok(());
                }
                SupervisorDecision::Delegate { agent_id, task } => {
                    if step == config.max_delegations {
                        bail!("Limite des délégations atteinte sans conclusion");
                    }
                    let worker = config
                        .workers
                        .iter()
                        .find(|worker| worker.identity.id == agent_id)
                        .context("Le superviseur a sélectionné un agent inconnu")?;
                    outbound
                        .send(Event::new(
                            "orchestration.delegated",
                            request_id,
                            json!({
                                "supervisor_id": config.supervisor.identity.id,
                                "agent_id": agent_id,
                                "task": task,
                                "step": step,
                            }),
                        ))
                        .await?;
                    let agent_history = worker_histories
                        .entry(agent_id.clone())
                        .or_insert_with(|| history[..history.len() - 1].to_vec());
                    agent_history.push(Message {
                        role: "user".to_owned(),
                        content: format!(
                            "Demande originale : {}\n\nTa tâche actuelle : {}",
                            history.last().context("Conversation vide")?.content,
                            task,
                        ),
                    });
                    let answer = AgentTurn::run(
                        worker,
                        step,
                        client,
                        agent_history,
                        &[],
                        request_id,
                        outbound,
                        cancel,
                        project_root,
                        approvals,
                        approve_reads,
                        writes,
                        memory,
                        context_tokens,
                        output_tokens,
                        tool_tokens,
                        &definitions,
                    )
                    .await?;
                    agent_history.push(Message {
                        role: "assistant".to_owned(),
                        content: answer.clone(),
                    });
                    reports.push(WorkerReport {
                        agent_id: agent_id.clone(),
                        task,
                        answer,
                    });
                    outbound
                        .send(Event::new(
                            "orchestration.reported",
                            request_id,
                            json!({
                                "supervisor_id": config.supervisor.identity.id,
                                "agent_id": agent_id,
                                "step": step,
                            }),
                        ))
                        .await?;
                }
            }
        }
        bail!("Limite des délégations atteinte")
    }
}

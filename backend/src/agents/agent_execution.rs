use std::{path::Path, sync::Arc};

use anyhow::{Result, bail};
use serde_json::json;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;

use crate::{
    agents::{
        ExecutionMode, MemoryStore, MultiAgentStrategy, Scheduler, SupervisedExecution,
        agent_turn::AgentTurn, context::limits,
    },
    event::Event,
    providers::Client,
    sessions::Message,
    tools::{self, ToolApprovalGate},
};

pub(crate) struct AgentExecution;

impl AgentExecution {
    #[allow(clippy::too_many_arguments)]
    pub(crate) async fn run(
        client: &Client,
        mode: &ExecutionMode,
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
        if let ExecutionMode::MultiAgent(multi) = mode
            && let MultiAgentStrategy::Supervised(config) = &multi.strategy
        {
            return SupervisedExecution::run(
                config, client, history, request_id, outbound, cancel, project_root,
                approvals, approve_reads, writes, memory,
            )
            .await;
        }
        let (context_tokens, output_tokens) = limits()?;
        let definitions = tools::definitions();
        let tool_tokens = serde_json::to_vec(&definitions)?.len().saturating_add(256);
        let mut previous_layer: Vec<(String, String)> = Vec::new();
        for (layer_index, agents) in Scheduler::plan(mode)?.iter().enumerate() {
            let mut current_layer = Vec::new();
            for agent in agents {
                let answer = AgentTurn::run(
                    agent,
                    layer_index,
                    client,
                    history,
                    &previous_layer,
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
                current_layer.push((agent.identity.id.clone(), answer));
            }
            previous_layer = current_layer;
        }
        if cancel.is_cancelled() {
            bail!("Exécution annulée");
        }
        if let (Some(store), Some((_, answer)), Some(prompt)) =
            (memory, previous_layer.last(), history.last())
            && let Err(error) = store.remember(&prompt.content, answer).await
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
                json!({
                    "result": previous_layer
                        .last()
                        .map(|(_, answer)| answer),
                }),
            ))
            .await?;
        Ok(())
    }
}

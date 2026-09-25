use anyhow::{Result, bail};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    agents::{ExecutionMode, Scheduler},
    api::Event,
    context::{assemble, limits},
    providers::{Client, stream},
    sessions::Message,
};

pub(crate) struct AgentExecution;

impl AgentExecution {
    pub(crate) async fn run(
        client: &Client,
        mode: &ExecutionMode,
        history: &[Message],
        request_id: &str,
        outbound: &mpsc::Sender<Event>,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let (context_tokens, output_tokens) = limits()?;
        let mut previous_layer: Vec<(String, String)> = Vec::new();
        for (layer_index, agents) in Scheduler::plan(mode).iter().enumerate() {
            let mut current_layer = Vec::new();
            for agent in agents {
                if cancel.is_cancelled() {
                    bail!("Exécution annulée");
                }
                outbound
                    .send(Event::new(
                        "agent.started",
                        request_id,
                        json!({
                            "agent_id": agent.identity.id,
                            "role": agent.role.name,
                            "layer": layer_index,
                            "model": agent.model.name,
                        }),
                    ))
                    .await?;
                let assembled = assemble(
                    &agent.role.instructions,
                    &previous_layer,
                    history,
                    context_tokens,
                    output_tokens,
                )?;
                outbound
                    .send(Event::new(
                        "context.prepared",
                        request_id,
                        json!({
                            "agent_id": agent.identity.id,
                            "context_tokens": context_tokens,
                            "output_tokens": output_tokens,
                            "estimated_input_tokens":
                                assembled.estimated_input_tokens,
                            "retained_history_messages":
                                assembled.retained_history,
                            "omitted_history_messages":
                                assembled.omitted_history,
                            "counting": "utf8_byte_estimate",
                        }),
                    ))
                    .await?;
                let messages = assembled.messages;
                let agent_id = agent.identity.id.clone();
                let events = outbound.clone();
                let correlation = request_id.to_owned();
                let answer = stream(
                    client,
                    &agent.model.name,
                    &messages,
                    context_tokens,
                    output_tokens,
                    cancel,
                    move |delta| {
                        let events = events.clone();
                        let agent_id = agent_id.clone();
                        let correlation = correlation.clone();
                        async move {
                            events
                                .send(Event::new(
                                    "agent.delta",
                                    &correlation,
                                    json!({
                                        "agent_id": agent_id,
                                        "text": delta,
                                    }),
                                ))
                                .await?;

                            Ok(())
                        }
                    },
                )
                .await?;
                outbound
                    .send(Event::new(
                        "agent.completed",
                        request_id,
                        json!({
                            "agent_id": agent.identity.id,
                            "layer": layer_index,
                        }),
                    ))
                    .await?;
                current_layer.push((agent.identity.id.clone(), answer));
            }
            previous_layer = current_layer;
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

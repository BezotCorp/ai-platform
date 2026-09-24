use anyhow::{Result, bail};
use serde_json::json;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::{
    api::event::Event,
    providers::ollama::{
        chat,
        client::Client,
    },
    sessions::message::Message,
};

use super::orchestration::{
    execution_mode::ExecutionMode,
    scheduler::Scheduler,
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
        let mut previous_layer:
            Vec<(String, String)> = Vec::new();

        for (layer_index, agents) in
            Scheduler::plan(mode).iter().enumerate()
        {
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

                let mut messages = Vec::new();

                if !agent.role.instructions
                    .trim()
                    .is_empty()
                {
                    messages.push(Message {
                        role: "system".into(),
                        content:
                            agent.role.instructions.clone(),
                    });
                }

                if !previous_layer.is_empty() {
                    let references = previous_layer
                        .iter()
                        .map(|(id, result)| {
                            format!("Agent {id} — proposition non vérifiée :\n{result}")
                        })
                        .collect::<Vec<_>>()
                        .join("\n\n");

                    if references.len() > 65_536 {
                        bail!(
                            "Résultats précédents trop volumineux"
                        );
                    }

                    messages.push(Message {
                        role: "user".into(),
                        content: format!("Propositions non vérifiées de la couche précédente :\n{references}"),
                    });
                }

                messages.extend_from_slice(history);

                let agent_id = agent.identity.id.clone();
                let events = outbound.clone();
                let correlation = request_id.to_owned();

                let answer = chat::stream(
                    client,
                    &agent.model.name,
                    &messages,
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

                current_layer.push((
                    agent.identity.id.clone(),
                    answer,
                ));
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

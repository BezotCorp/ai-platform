use std::{
    path::PathBuf,
    sync::Arc,
    time::Duration,
};

use axum::extract::ws::{Message as WsMessage, WebSocket};

use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};

use tokio::{
    sync::{Semaphore, mpsc},
    task::JoinHandle,
};

use tokio_util::sync::CancellationToken;

use crate::{
    agents::AgentExecution,
    api::{Command, Event, RunRequest, models::list},
    providers::Client,
    tools::ToolApprovalGate,
};

fn match_token(provided: &str, expected: &str) -> bool {
    let left = provided.as_bytes();
    let right = expected.as_bytes();

    if left.len() != right.len() {
        return false;
    }

    let mut different = 0u8;

    for (&a, &b) in left.iter().zip(right) {
        different |= a ^ b;
    }

    different == 0
}

pub(crate) async fn serve(
    mut socket: WebSocket,
    client: Client,
    expected_token: Arc<str>,
    gpu: Arc<Semaphore>,
    project_root: Arc<PathBuf>,
    approvals: ToolApprovalGate,
    approve_reads: bool,
) {
    let first = tokio::time::timeout(Duration::from_secs(5), socket.recv()).await;

    let Ok(Some(Ok(WsMessage::Text(text)))) = first else {
        return;
    };

    let Ok(Command::Authenticate { token }) = serde_json::from_str::<Command>(&text) else {
        return;
    };

    if !match_token(&token, &expected_token) {
        return;
    }

    let (mut sink, mut stream) = socket.split();

    let (tx, mut rx) = mpsc::channel::<Event>(128);

    let writer = tokio::spawn(async move {
        while let Some(event) = rx.recv().await {
            let Ok(encoded) = serde_json::to_string(&event) else {
                break;
            };

            if sink.send(WsMessage::Text(encoded.into())).await.is_err() {
                break;
            }
        }
    });

    let _ = tx.send(Event::new("authenticated", "", json!({}))).await;

    let mut active: Option<(String, CancellationToken, JoinHandle<()>)> = None;

    while let Some(frame) = stream.next().await {
        let Ok(WsMessage::Text(text)) = frame else {
            break;
        };

        if text.len() > 256 * 1024 {
            let _ = tx
                .send(Event::new(
                    "error",
                    "",
                    json!({
                        "error": "Message trop volumineux",
                    }),
                ))
                .await;

            continue;
        }

        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            let _ = tx
                .send(Event::new(
                    "error",
                    "",
                    json!({
                        "error": "JSON invalide",
                    }),
                ))
                .await;

            continue;
        };

        if value.get("type").and_then(Value::as_str) == Some("run.start") {
            if active
                .as_ref()
                .is_some_and(|(_, _, handle)| !handle.is_finished())
            {
                let _ = tx
                    .send(Event::new(
                        "error",
                        "",
                        json!({
                            "error":
                                "Exécution déjà en cours",
                        }),
                    ))
                    .await;

                continue;
            }

            let request: RunRequest = match serde_json::from_value(value) {
                Ok(request) => request,

                Err(error) => {
                    let _ = tx
                        .send(Event::new(
                            "error",
                            "",
                            json!({
                                "error":
                                    error.to_string(),
                            }),
                        ))
                        .await;

                    continue;
                }
            };

            if let Err(error) = request.validate_messages() {
                let _ = tx
                    .send(Event::new(
                        "error",
                        &request.request_id,
                        json!({
                            "error": error.to_string(),
                        }),
                    ))
                    .await;

                continue;
            }

            let request_id = request.request_id.clone();
            let history = request.messages.clone();

            let mode = match request.into_mode() {
                Ok(mode) => mode,

                Err(error) => {
                    let _ = tx
                        .send(Event::new(
                            "error",
                            &request_id,
                            json!({
                                "error":
                                    error.to_string(),
                            }),
                        ))
                        .await;

                    continue;
                }
            };

            let cancel = CancellationToken::new();

            let task_cancel = cancel.clone();
            let event_tx = tx.clone();
            let task_client = client.clone();
            let task_gpu = gpu.clone();
            let task_root = project_root.clone();
            let task_approvals = approvals.clone();
            let id = request_id.clone();

            let handle = tokio::spawn(async move {
                let _ = event_tx
                    .send(Event::new("run.queued", &id, json!({})))
                    .await;

                let permit = tokio::select! {
                    () = task_cancel.cancelled() => {
                        return;
                    }

                    result = task_gpu.acquire_owned() => {
                        match result {
                            Ok(permit) => permit,
                            Err(_) => return,
                        }
                    }
                };

                let _ = event_tx
                    .send(Event::new("run.started", &id, json!({})))
                    .await;

                let result = AgentExecution::run(
                    &task_client,
                    &mode,
                    &history,
                    &id,
                    &event_tx,
                    &task_cancel,
                    &task_root,
                    &task_approvals,
                    approve_reads,
                )
                .await;

                if let Err(error) = result {
                    let kind = if task_cancel.is_cancelled() {
                        "run.cancelled"
                    } else {
                        "run.failed"
                    };

                    let _ = event_tx
                        .send(Event::new(
                            kind,
                            &id,
                            json!({
                                "error":
                                    error.to_string(),
                            }),
                        ))
                        .await;
                }

                drop(permit);
            });

            active = Some((request_id, cancel, handle));
        } else {
            match serde_json::from_value::<Command>(value) {
                Ok(Command::ModelsList { request_id }) => {
                    let event = list(&client, &request_id).await.unwrap_or_else(|error| {
                        Event::new(
                            "error",
                            &request_id,
                            json!({
                                "error":
                                    error.to_string(),
                            }),
                        )
                    });

                    if tx.send(event).await.is_err() {
                        break;
                    }
                }

                Ok(Command::RunCancel { request_id }) => match &active {
                    Some((id, cancel, handle)) if id == &request_id && !handle.is_finished() => {
                        cancel.cancel();
                    }

                    _ => {
                        let _ = tx
                            .send(Event::new(
                                "error",
                                &request_id,
                                json!({
                                    "error":
                                        "Aucune exécution active correspondante",
                                }),
                            ))
                            .await;
                    }
                },

                Ok(Command::ApprovalResolve {
                    request_id,
                    call_id,
                    approved,
                }) => {
                    let belongs_to_run = active
                        .as_ref()
                        .is_some_and(|(id, _, handle)| {
                            id == &request_id
                                && !handle.is_finished()
                        });

                    let resolved = if belongs_to_run {
                        approvals.resolve(
                            &request_id,
                            &call_id,
                            approved,
                        ).await
                    } else {
                        false
                    };

                    let _ = tx.send(Event::new(
                        "approval.resolved",
                        &request_id,
                        json!({
                            "call_id": call_id,
                            "accepted": resolved,
                            "approved": if resolved {
                                Some(approved)
                            } else {
                                None
                            },
                        }),
                    )).await;
                }

                _ => {
                    let _ = tx
                        .send(Event::new(
                            "error",
                            "",
                            json!({
                                "error": "Commande inconnue",
                            }),
                        ))
                        .await;
                }
            }
        }
    }

    if let Some((_, token, handle)) = active {
        token.cancel();
        handle.abort();
    }

    drop(tx);
    writer.abort();
}

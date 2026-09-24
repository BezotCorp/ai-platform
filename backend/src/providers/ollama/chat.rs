use std::future::Future;

use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::sessions::message::Message;

use super::client::Client;

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_ANSWER_BYTES: usize = 8 * 1024 * 1024;

pub(crate) async fn stream<F, Fut>(
    client: &Client,
    model: &str,
    messages: &[Message],
    cancel: &CancellationToken,
    mut on_delta: F,
) -> Result<String>
where
    F: FnMut(String) -> Fut,
    Fut: Future<Output = Result<()>>,
{
    let request = json!({
        "model": model,
        "messages": messages,
        "stream": true,
    });

    let response = tokio::select! {
        () = cancel.cancelled() => {
            bail!("Exécution annulée");
        }

        result = client.chat(&request) => result?,
    };

    let mut stream = response.bytes_stream();

    let mut frame = Vec::new();
    let mut answer = String::new();
    let mut completed = false;

    loop {
        let part = tokio::select! {
            () = cancel.cancelled() => {
                bail!("Exécution annulée");
            }

            part = stream.next() => part,
        };

        let Some(part) = part else {
            break;
        };

        let bytes = part?;

        for byte in bytes.iter().copied() {
            if byte == b'\n' {
                if frame.is_empty() {
                    continue;
                }

                let packet: Value =
                    serde_json::from_slice(&frame)
                        .context("Trame Ollama invalide")?;

                frame.clear();

                if let Some(error) = packet.get("error") {
                    bail!("Erreur Ollama : {error}");
                }

                if let Some(message) = packet.get("message") {
                    let has_tool_calls = message
                        .get("tool_calls")
                        .and_then(Value::as_array)
                        .is_some_and(|calls| !calls.is_empty());

                    if has_tool_calls {
                        bail!(
                            "Le modèle a demandé un outil, "
                        );
                    }

                    if let Some(delta) = message
                        .get("content")
                        .and_then(Value::as_str)
                    {
                        if answer
                            .len()
                            .saturating_add(delta.len())
                            > MAX_ANSWER_BYTES
                        {
                            bail!("Réponse trop volumineuse");
                        }

                        if !delta.is_empty() {
                            answer.push_str(delta);
                            on_delta(delta.to_owned()).await?;
                        }
                    }
                }

                if packet
                    .get("done")
                    .and_then(Value::as_bool)
                    == Some(true)
                {
                    completed = true;
                    break;
                }
            } else {
                frame.push(byte);

                if frame.len() > MAX_FRAME_BYTES {
                    bail!("Trame Ollama trop volumineuse");
                }
            }
        }

        if completed {
            break;
        }
    }

    if !completed {
        bail!("Flux Ollama interrompu avant sa fin");
    }

    Ok(answer)
}

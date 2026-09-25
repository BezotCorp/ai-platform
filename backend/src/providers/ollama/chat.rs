use anyhow::{Context, Result, bail};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{collections::BTreeMap, future::Future};
use tokio_util::sync::CancellationToken;

use crate::providers::{ChatTurn, Client};

const MAX_FRAME_BYTES: usize = 1024 * 1024;
const MAX_ANSWER_BYTES: usize = 8 * 1024 * 1024;
const MAX_TOOL_CALLS: usize = 8;

pub(crate) struct Chat<'a> {
    pub client: &'a Client,
    pub model: &'a str,
    pub messages: &'a [Value],
    pub tools: &'a [Value],
    pub context_tokens: usize,
    pub output_tokens: usize,
    pub cancel: &'a CancellationToken,
}

impl Chat<'_> {
    pub(crate) async fn stream<F, Fut>(&self, mut on_delta: F) -> Result<ChatTurn>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<()>>,
    {
        let request = json!({
            "model": self.model,
            "messages": self.messages,
            "tools": self.tools,
            "stream": true,
            "options": {
                "num_ctx": self.context_tokens,
                "num_predict": self.output_tokens,
            },
        });
        let response = tokio::select! {
            () = self.cancel.cancelled() => {
                bail!("Exécution annulée");
            }
            result = self.client.chat(&request) => result?,
        };
        let mut stream = response.bytes_stream();
        let mut frame = Vec::new();
        let mut content = String::new();
        let mut indexed = BTreeMap::<u64, Value>::new();
        let mut unindexed = Vec::<Value>::new();
        let mut completed = false;
        loop {
            let part = tokio::select! {
                () = self.cancel.cancelled() => {
                    bail!("Exécution annulée");
                }
                part = stream.next() => part,
            };
            let Some(part) = part else {
                break;
            };
            let bytes = part?;
            for byte in bytes.iter().copied() {
                if byte != b'\n' {
                    frame.push(byte);
                    if frame.len() > MAX_FRAME_BYTES {
                        bail!("Trame Ollama trop volumineuse");
                    }
                    continue;
                }
                if frame.is_empty() {
                    continue;
                }
                let packet: Value =
                    serde_json::from_slice(&frame).context("Trame Ollama invalide")?;

                frame.clear();
                if let Some(error) = packet.get("error") {
                    bail!("Erreur Ollama : {error}");
                }
                if let Some(message) = packet.get("message") {
                    if let Some(delta) = message.get("content").and_then(Value::as_str) {
                        if content.len().saturating_add(delta.len()) > MAX_ANSWER_BYTES {
                            bail!("Réponse trop volumineuse");
                        }
                        if !delta.is_empty() {
                            content.push_str(delta);
                            on_delta(delta.to_owned()).await?;
                        }
                    }
                    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
                        for call in calls {
                            let index = call
                                .pointer("/function/index")
                                .or_else(|| call.get("index"))
                                .and_then(Value::as_u64);
                            if let Some(index) = index {
                                match indexed.get_mut(&index) {
                                    Some(previous) => {
                                        let old_arguments =
                                            previous.pointer("/function/arguments").cloned();
                                        let new_arguments =
                                            call.pointer("/function/arguments").cloned();
                                        if let (
                                            Some(Value::String(old)),
                                            Some(Value::String(new)),
                                        ) = (old_arguments, new_arguments)
                                        {
                                            let mut merged = call.clone();
                                            merged["function"]["arguments"] =
                                                Value::String(old + &new);
                                            *previous = merged;
                                        } else {
                                            *previous = call.clone();
                                        }
                                    }
                                    None => {
                                        indexed.insert(index, call.clone());
                                    }
                                }
                            } else {
                                unindexed.push(call.clone());
                            }
                            if indexed.len() + unindexed.len() > MAX_TOOL_CALLS {
                                bail!("Trop d'appels d'outils");
                            }
                        }
                    }
                }
                if packet.get("done").and_then(Value::as_bool) == Some(true) {
                    completed = true;
                    break;
                }
            }
            if completed {
                break;
            }
        }
        if !completed {
            bail!("Flux Ollama interrompu avant sa fin");
        }
        let mut tool_calls = indexed.into_values().collect::<Vec<_>>();
        tool_calls.extend(unindexed);
        Ok(ChatTurn {
            content,
            tool_calls,
        })
    }
}

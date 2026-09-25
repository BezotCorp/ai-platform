use crate::{agents::relevance, sessions::Message};

use anyhow::{Result, anyhow};

const MESSAGE_OVERHEAD: usize = 32;

struct Exchange {
    start: usize,
    end: usize,
    cost: usize,
    relevance: usize,
}

fn exchange_cost(messages: &[Message]) -> Result<usize> {
    messages.iter().try_fold(0usize, |total, message| {
        total
            .checked_add(message.content.len())
            .and_then(|value| value.checked_add(MESSAGE_OVERHEAD))
            .ok_or_else(|| anyhow!("Estimation de l'historique impossible"))
    })
}

/// Sélectionne des échanges complets, dans leur ordre d'origine.
/// Les indices sont relatifs à l'historique complet, dernier message exclu.
pub(crate) fn select_history(
    history: &[Message],
    query: &str,
    role_instructions: &str,
    capacity: usize,
) -> Result<Vec<usize>> {
    let mut exchanges = Vec::new();
    let mut start = None;
    for (index, message) in history.iter().enumerate() {
        if message.role == "user" {
            if let Some(previous) = start {
                let group = &history[previous..index];
                exchanges.push(Exchange {
                    start: previous,
                    end: index,
                    cost: exchange_cost(group)?,
                    relevance: 4 * relevance(
                        query,
                        &group
                            .iter()
                            .map(|message| message.content.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ) + relevance(
                        role_instructions,
                        &group
                            .iter()
                            .map(|message| message.content.as_str())
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ),
                });
            }
            start = Some(index);
        }
    }
    if let Some(previous) = start {
        let group = &history[previous..];
        exchanges.push(Exchange {
            start: previous,
            end: history.len(),
            cost: exchange_cost(group)?,
            relevance: 4 * relevance(
                query,
                &group
                    .iter()
                    .map(|message| message.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ) + relevance(
                role_instructions,
                &group
                    .iter()
                    .map(|message| message.content.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
            ),
        });
    }
    // Pertinence d'abord, récence ensuite. Si aucun échange ne partage
    // de termes avec la question, les échanges récents sont prioritaires.
    exchanges.sort_by(|left, right| {
        right
            .relevance
            .cmp(&left.relevance)
            .then_with(|| right.start.cmp(&left.start))
    });
    let mut remaining = capacity;
    let mut selected = Vec::new();
    for exchange in exchanges {
        if exchange.cost <= remaining {
            remaining -= exchange.cost;
            selected.extend(exchange.start..exchange.end);
        }
    }
    selected.sort_unstable();
    Ok(selected)
}

use anyhow::{Result, bail};

use crate::{context::ContextBudget, sessions::Message};

// Estimation fondée sur les octets UTF-8.
// Le comptage exact dépend du tokenizer du modèle.
const MESSAGE_OVERHEAD: usize = 32;
const TEMPLATE_RESERVE: usize = 128;

pub(crate) struct AssembledContext {
    pub messages: Vec<Message>,
    pub retained_history: usize,
    pub omitted_history: usize,
    pub estimated_input_tokens: usize,
}

fn estimated_tokens(message: &Message) -> Result<usize> {
    message
        .content
        .len()
        .checked_add(MESSAGE_OVERHEAD)
        .ok_or_else(|| anyhow::anyhow!("Estimation du contexte impossible"))
}

pub(crate) fn assemble(
    instructions: &str,
    previous_layer: &[(String, String)],
    history: &[Message],
    capacity: usize,
    output_tokens: usize,
    tool_tokens: usize,
) -> Result<AssembledContext> {
    let Some(last) = history.last() else {
        bail!("Conversation vide");
    };
    if last.role != "user" {
        bail!("Le dernier message doit venir de l'utilisateur");
    }
    let system = if instructions.trim().is_empty() {
        None
    } else {
        Some(Message {
            role: "system".into(),
            content: instructions.to_owned(),
        })
    };
    let system_tokens = system
        .as_ref()
        .map(estimated_tokens)
        .transpose()?
        .unwrap_or(0)
        .checked_add(TEMPLATE_RESERVE)
        .ok_or_else(|| anyhow::anyhow!("Budget système trop volumineux"))?;
    let budget = ContextBudget {
        capacity_tokens: capacity,
        system_tokens,
        output_tokens,
        tool_tokens,
    };
    let available = budget
        .available_tokens()
        .ok_or_else(|| anyhow::anyhow!("Les réserves dépassent la fenêtre de contexte"))?;
    // Les propositions de la couche précédente
    // sont conservées sans les présenter comme
    // des connaissances vérifiées.
    let proposals = if previous_layer.is_empty() {
        None
    } else {
        let content = previous_layer
            .iter()
            .map(|(id, result)| {
                "Proposition non vérifiée de ".to_string() + &format!("l'agent {id} :\n{result}")
            })
            .collect::<Vec<_>>()
            .join("\n\n");
        Some(Message {
            role: "user".into(),
            content: format!(
                "Résultats de la couche MoA précédente. \
                 Ne les traite pas comme des faits \
                 établis.\n\n{content}"
            ),
        })
    };
    let last_tokens = estimated_tokens(last)?;
    let proposals_tokens = proposals
        .as_ref()
        .map(estimated_tokens)
        .transpose()?
        .unwrap_or(0);
    let required = last_tokens
        .checked_add(proposals_tokens)
        .ok_or_else(|| anyhow::anyhow!("Contexte obligatoire trop volumineux"))?;
    if required > available {
        bail!(
            "Le message utilisateur et les résultats MoA dépassent le budget disponible. Aucune donnée obligatoire n'a été tronquée."
        );
    }
    let mut remaining = available - required;
    let older = &history[..history.len() - 1];
    // On conserve le suffixe chronologique le
    // plus récent qui tient dans le budget.
    let mut retained = Vec::new();
    for message in older.iter().rev() {
        let size = estimated_tokens(message)?;
        if size > remaining {
            break;
        }
        remaining -= size;
        retained.push(message.clone());
    }
    retained.reverse();
    // Éviter une réponse d'assistant orpheline
    // en début d'historique.
    while retained
        .first()
        .is_some_and(|message| message.role != "user")
    {
        let removed = retained.remove(0);
        remaining = remaining
            .checked_add(estimated_tokens(&removed)?)
            .ok_or_else(|| anyhow::anyhow!("Débordement du budget"))?;
    }
    let retained_history = retained.len() + 1;
    let omitted_history = history.len().saturating_sub(retained_history);
    let mut messages = Vec::with_capacity(retained.len() + 3);
    if let Some(system) = system {
        messages.push(system);
    }
    messages.extend(retained);
    if let Some(proposals) = proposals {
        messages.push(proposals);
    }
    messages.push(last.clone());
    let used_content = available - remaining;
    let estimated_input_tokens = system_tokens
        .checked_add(used_content)
        .ok_or_else(|| anyhow::anyhow!("Estimation du contexte trop importante"))?;
    Ok(AssembledContext {
        messages,
        retained_history,
        omitted_history,
        estimated_input_tokens,
    })
}

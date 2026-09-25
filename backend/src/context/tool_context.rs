use anyhow::{Result, bail};
use serde_json::Value;

use crate::{
    context::{prepared_tool_context::PreparedToolContext, tool_exchange::ToolExchange},
    sessions::Message,
};

pub(crate) struct ToolContext {
    system: Option<Value>,
    history: Vec<(usize, Value)>,
    mandatory: Vec<Value>,
    exchanges: Vec<ToolExchange>,
    compacted: Vec<bool>,
}

impl ToolContext {
    pub(crate) fn new(
        messages: Vec<Message>,
        history_indices: Vec<usize>,
    ) -> Result<Self> {
        let mut messages = messages.into_iter();
        let first = messages.next().ok_or_else(|| {
            anyhow::anyhow!("Contexte obligatoire absent")
        })?;

        let mut remaining = Vec::new();
        let system = if first.role == "system" {
            Some(serde_json::to_value(first)?)
        } else {
            remaining.push(serde_json::to_value(first)?);
            None
        };
        remaining.extend(
            messages
                .map(serde_json::to_value)
                .collect::<serde_json::Result<Vec<_>>>()?,
        );

        if remaining.len() < history_indices.len() + 1 {
            bail!("Historique et provenance incohérents");
        }

        let mandatory = remaining.split_off(history_indices.len());
        if mandatory
            .last()
            .and_then(|message| message.get("role"))
            .and_then(Value::as_str)
            != Some("user")
        {
            bail!("Dernier message utilisateur absent");
        }

        Ok(Self {
            system,
            history: history_indices
                .into_iter()
                .zip(remaining)
                .collect(),
            mandatory,
            exchanges: Vec::new(),
            compacted: Vec::new(),
        })
    }

    pub(crate) fn append(
        &mut self,
        assistant: Value,
        results: Vec<Value>,
        round: usize,
    ) -> Result<()> {
        let exchange = ToolExchange::new(assistant, results, round)?;
        self.exchanges.push(exchange);
        self.compacted.push(false);
        Ok(())
    }

    fn render(&self) -> Vec<Value> {
        let mut messages = Vec::new();
        if let Some(system) = &self.system {
            messages.push(system.clone());
        }
        messages.extend(self.history.iter().map(|(_, message)| message.clone()));
        messages.extend(self.mandatory.iter().cloned());

        for (exchange, compacted) in self.exchanges.iter().zip(&self.compacted) {
            if *compacted {
                messages.push(exchange.summary.clone());
            } else {
                messages.push(exchange.assistant.clone());
                messages.extend(exchange.results.iter().cloned());
            }
        }
        messages
    }

    fn omit_oldest_exchange(&mut self) -> Vec<usize> {
        if self.history.is_empty() {
            return Vec::new();
        }

        let end = self
            .history
            .iter()
            .enumerate()
            .skip(1)
            .find(|(_, (_, message))| {
                message.get("role").and_then(Value::as_str) == Some("user")
            })
            .map(|(index, _)| index)
            .unwrap_or(self.history.len());

        self.history
            .drain(..end)
            .map(|(index, _)| index)
            .collect()
    }

    pub(crate) fn prepare(
        &mut self,
        capacity: usize,
        tool_tokens: usize,
        output_tokens: usize,
    ) -> Result<PreparedToolContext> {
        let reserve = tool_tokens
            .checked_add(output_tokens)
            .and_then(|value| value.checked_add(384))
            .ok_or_else(|| anyhow::anyhow!("Réserve de contexte excessive"))?;
        let available = capacity
            .checked_sub(reserve)
            .ok_or_else(|| anyhow::anyhow!("Réserves supérieures à la fenêtre de contexte"))?;

        let mut compacted_rounds = Vec::new();
        let mut omitted_history_indices = Vec::new();
        let mut latest_round_compacted = false;

        loop {
            let messages = self.render();
            if serde_json::to_vec(&messages)?.len() <= available {
                return Ok(PreparedToolContext {
                    messages,
                    compacted_rounds,
                    omitted_history_indices,
                    latest_round_compacted,
                });
            }

            // Condenser d'abord les anciennes lectures terminées.
            let earlier = self.exchanges.len().saturating_sub(1);
            if let Some(index) = (0..earlier).find(|&index| {
                !self.compacted[index] && !self.exchanges[index].contains_write
            }) {
                self.compacted[index] = true;
                compacted_rounds.push(index);
                continue;
            }

            // Les échanges historiques sont facultatifs ; la question,
            // le rôle et les propositions MoA ne le sont jamais.
            let omitted = self.omit_oldest_exchange();
            if !omitted.is_empty() {
                omitted_history_indices.extend(omitted);
                continue;
            }

            // N'omettre la dernière lecture que si elle seule empêche
            // la génération. Son résumé exige explicitement une relecture.
            if let Some(index) = self.exchanges.len().checked_sub(1) {
                if !self.compacted[index] && !self.exchanges[index].contains_write {
                    self.compacted[index] = true;
                    compacted_rounds.push(index);
                    latest_round_compacted = true;
                    continue;
                }
            }

            bail!(
                "Le contexte obligatoire ou les résultats d'écriture dépassent le budget ; aucune donnée obligatoire n'a été supprimée"
            );
        }
    }
}

use anyhow::{Context, Result};
use serde::Serialize;

use crate::providers::Client;

#[derive(Debug, Serialize)]
pub(crate) struct AvailableModel {
    pub name: String,
}

impl AvailableModel {
    pub(crate) async fn list(client: &Client) -> Result<Vec<AvailableModel>> {
        let response = client.get("tags").await?;
        let entries = response
            .get("models")
            .and_then(|value| value.as_array())
            .context("Liste des modèles Ollama invalide")?;
        let mut names = entries
            .iter()
            .map(|entry| {
                entry
                    .get("name")
                    .and_then(|value| value.as_str())
                    .map(str::to_owned)
                    .context("Modèle Ollama sans nom")
            })
            .collect::<Result<Vec<_>>>()?;
        names.sort();
        names.dedup();
        Ok(names
            .into_iter()
            .map(|name| AvailableModel { name })
            .collect())
    }
}

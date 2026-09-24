use anyhow::Result;
use serde_json::json;

use crate::providers::ollama::{
    client::Client,
    models,
};

use super::event::Event;

pub(crate) async fn list(
    client: &Client,
    request_id: &str,
) -> Result<Event> {
    let installed = models::list(client).await?;

    Ok(Event::new(
        "models.list",
        request_id,
        json!({
            "models": installed,
        }),
    ))
}

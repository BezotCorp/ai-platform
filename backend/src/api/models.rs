use anyhow::Result;
use serde_json::json;

use crate::{
    api::Event,
    providers::{AvailableModel, Client},
};

pub(crate) async fn list(client: &Client, request_id: &str) -> Result<Event> {
    let installed = AvailableModel::list(client).await?;
    Ok(Event::new(
        "models.list",
        request_id,
        json!({
            "models": installed,
        }),
    ))
}

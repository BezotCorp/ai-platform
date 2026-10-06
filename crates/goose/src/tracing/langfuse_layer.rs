use crate::tracing::observation_layer::{BatchManager, ObservationLayer, SpanTracker};
use chrono::Utc;
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{env, sync::Arc, time::Duration};
use tokio::sync::Mutex;
use url::Url;
use uuid::Uuid;
const DEFAULT_LANGFUSE_URL: &str = "http://localhost:3000";

#[derive(Debug, Serialize, Deserialize)]
struct LangfuseIngestionResponse {
    successes: Vec<LangfuseIngestionSuccess>,
    errors: Vec<LangfuseIngestionError>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LangfuseIngestionSuccess {
    id: String,
    status: i32,
}

#[derive(Debug, Serialize, Deserialize)]
struct LangfuseIngestionError {
    id: String,
    status: i32,
    message: Option<String>,
    error: Option<Value>,
}

#[derive(Debug, Clone)]
pub struct LangfuseBatchManager {
    pub batch: Vec<Value>,
    pub client: Client,
    pub base_url: String,
    pub public_key: String,
    pub secret_key: String,
}

impl LangfuseBatchManager {
    pub fn new(public_key: String, secret_key: String, base_url: String) -> Self {
        Self {
            batch: Vec::new(),
            client: Client::builder()
                .timeout(Duration::from_secs(10))
                .build()
                .expect("Failed to create HTTP client"),
            base_url,
            public_key,
            secret_key,
        }
    }

    pub fn spawn_sender(manager: Arc<Mutex<Self>>) {
        const BATCH_INTERVAL: Duration = Duration::from_secs(5);

        tokio::spawn(async move {
            loop {
                tokio::time::sleep(BATCH_INTERVAL).await;
                if let Err(e) = manager.lock().await.send() {
                    tracing::error!(
                        error.msg = %e,
                        error.type = %std::any::type_name_of_val(&e),
                        "Failed to send batch to Langfuse"
                    );
                }
            }
        });
    }

    pub async fn send_async(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        if self.batch.is_empty() {
            return Ok(());
        }

        let payload = json!({ "batch": self.batch });
        let base_url = Url::parse(&self.base_url).map_err(|e| format!("Invalid base URL: {e}"))?;
        let url = base_url
            .join("api/public/ingestion")
            .map_err(|e| format!("Failed to construct endpoint URL: {e}"))?;

        let response = self
            .client
            .post(url)
            .basic_auth(&self.public_key, Some(&self.secret_key))
            .json(&payload)
            .send()
            .await?;

        match response.status() {
            status if status.is_success() => {
                let response_body: LangfuseIngestionResponse = response.json().await?;

                for error in &response_body.errors {
                    tracing::error!(
                        id = %error.id,
                        status = error.status,
                        message = error.message.as_deref().unwrap_or("No message"),
                        error = ?error.error,
                        "Partial failure in batch ingestion"
                    );
                }

                if !response_body.successes.is_empty() {
                    self.batch.clear();
                }

                if response_body.successes.is_empty() && !response_body.errors.is_empty() {
                    Err("Langfuse ingestion failed for all items".into())
                } else {
                    Ok(())
                }
            }
            status @ (StatusCode::BAD_REQUEST
            | StatusCode::UNAUTHORIZED
            | StatusCode::FORBIDDEN
            | StatusCode::NOT_FOUND
            | StatusCode::METHOD_NOT_ALLOWED) => {
                let err_text = response.text().await.unwrap_or_default();
                Err(format!("Langfuse API error: {}: {}", status, err_text).into())
            }
            status => {
                let err_text = response.text().await.unwrap_or_default();
                Err(format!("Unexpected status code: {}: {}", status, err_text).into())
            }
        }
    }
}

impl BatchManager for LangfuseBatchManager {
    fn add_event(&mut self, event_type: &str, body: Value) {
        self.batch.push(json!({
            "id": Uuid::new_v4().to_string(),
            "timestamp": Utc::now().to_rfc3339(),
            "type": event_type,
            "body": body
        }));
    }

    fn send(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(self.send_async())
        })
    }

    fn is_empty(&self) -> bool {
        self.batch.is_empty()
    }
}

pub fn create_langfuse_observer() -> Option<ObservationLayer> {
    let public_key = env::var("LANGFUSE_PUBLIC_KEY")
        .or_else(|_| env::var("LANGFUSE_INIT_PROJECT_PUBLIC_KEY"))
        .unwrap_or_default(); // Use empty string if not found

    let secret_key = env::var("LANGFUSE_SECRET_KEY")
        .or_else(|_| env::var("LANGFUSE_INIT_PROJECT_SECRET_KEY"))
        .unwrap_or_default(); // Use empty string if not found

    // Return None if either key is empty
    if public_key.is_empty() || secret_key.is_empty() {
        return None;
    }

    let base_url = env::var("LANGFUSE_URL")
        .or_else(|_| env::var("LANGFUSE_BASE_URL"))
        .or_else(|_| env::var("LANGFUSE_HOST"))
        .unwrap_or_else(|_| DEFAULT_LANGFUSE_URL.to_string());

    let batch_manager = Arc::new(Mutex::new(LangfuseBatchManager::new(
        public_key, secret_key, base_url,
    )));

    if !cfg!(test) {
        LangfuseBatchManager::spawn_sender(batch_manager.clone());
    }

    Some(ObservationLayer {
        batch_manager,
        span_tracker: Arc::new(Mutex::new(SpanTracker::new())),
    })
}

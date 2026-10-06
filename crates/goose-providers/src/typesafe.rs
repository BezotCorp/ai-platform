use crate::api_client::ApiClient;
use crate::decision::{DecisionProvider, DecisionRequest, DecisionResponse};
use crate::http_status::read_json_response;
use crate::openai_compatible::handle_status;
use async_trait::async_trait;
use bcaip_provider_types::errors::ProviderError;
pub const TYPESAFE_DEFAULT_HOST: &str = "https://api.typesafe.ai";
pub const TYPESAFE_DEFAULT_MODEL: &str = "jev-latest";

pub struct TypeSafeProvider {
    api_client: ApiClient,
}

impl TypeSafeProvider {
    pub fn new(api_client: ApiClient) -> Self {
        Self { api_client }
    }
}

#[async_trait]
impl DecisionProvider for TypeSafeProvider {
    async fn create_decision(
        &self,
        request: &DecisionRequest,
    ) -> Result<DecisionResponse, ProviderError> {
        let payload = serde_json::to_value(request).map_err(|error| {
            ProviderError::RequestFailed(format!("Failed to serialize decision request: {error}"))
        })?;
        let response = self
            .api_client
            .request("v1/systemone")
            .response_post(&payload)
            .await?;
        let response = handle_status(response).await?;
        read_json_response(response).await
    }
}

use super::azureauth::{AuthError, AzureAuth};
use super::base::ProviderDef;
use anyhow::Result;
use async_trait::async_trait;
use bcaip_provider_types::base::{ConfigKey, ProviderMetadata};
use futures::future::BoxFuture;
use goose_providers::api_client::{ApiClient, AuthMethod, AuthProvider};
use goose_providers::openai_compatible::OpenAiCompatibleProvider;
const AZURE_PROVIDER_NAME: &str = "azure_openai";
pub const AZURE_DEFAULT_MODEL: &str = "gpt-4o";
pub const AZURE_DOC_URL: &str =
    "https://learn.microsoft.com/en-us/azure/ai-services/openai/concepts/models";
const AZURE_DEFAULT_API_VERSION: &str = "2024-10-21";
pub const AZURE_OPENAI_KNOWN_MODELS: &[&str] = &["gpt-4o", "gpt-4o-mini", "gpt-4"];

/// New-style Azure AI endpoints use `/v1/` paths and reject the `api-version` query param.
fn is_v1_endpoint(endpoint: &str) -> bool {
    let normalized = endpoint.trim_end_matches('/');
    normalized.ends_with("/v1") || endpoint.contains("/v1/")
}

pub struct AzureProvider;

// Custom auth provider that wraps AzureAuth
struct AzureAuthProvider {
    auth: AzureAuth,
}

#[async_trait]
impl AuthProvider for AzureAuthProvider {
    async fn get_auth_header(&self) -> Result<(String, String)> {
        let auth_token = self
            .auth
            .get_token()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to get authentication token: {}", e))?;

        match self.auth.credential_type() {
            super::azureauth::AzureCredentials::ApiKey(_) => {
                Ok(("api-key".to_string(), auth_token.token_value))
            }
            super::azureauth::AzureCredentials::BearerToken(_)
            | super::azureauth::AzureCredentials::DefaultCredential => Ok((
                "Authorization".to_string(),
                format!("Bearer {}", auth_token.token_value),
            )),
        }
    }
}

impl bcaip_provider_types::base::ProviderDescriptor for AzureProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            AZURE_PROVIDER_NAME,
            "Azure OpenAI",
            "Models through Azure OpenAI Service (supports API key, Entra ID bearer token, and Azure credential chain)",
            "gpt-4o",
            AZURE_OPENAI_KNOWN_MODELS.to_vec(),
            AZURE_DOC_URL,
            vec![
                ConfigKey::new("AZURE_OPENAI_ENDPOINT", true, false, None, true),
                ConfigKey::new("AZURE_OPENAI_DEPLOYMENT_NAME", true, false, None, true),
                ConfigKey::new("AZURE_OPENAI_API_VERSION", false, false, None, false),
                ConfigKey::new("AZURE_OPENAI_API_KEY", false, true, Some(""), true),
                ConfigKey::new("AZURE_OPENAI_AD_TOKEN", false, true, Some(""), true),
            ],
        )
        .with_setup(
            bcaip_provider_types::ProviderSetupMetadata::new(
                bcaip_provider_types::ProviderSetupCategory::Model,
                bcaip_provider_types::ProviderSetupMethod::ConfigFields,
                bcaip_provider_types::ProviderSetupGroup::Additional,
            )
            .with_field("AZURE_OPENAI_ENDPOINT", "Endpoint", Some("https://your-resource.openai.azure.com"), None)
            .with_field("AZURE_OPENAI_DEPLOYMENT_NAME", "Deployment", Some("gpt-4o"), None)
            .with_field("AZURE_OPENAI_API_KEY", "API Key", Some("Paste your API key"), None)
            .with_field("AZURE_OPENAI_AD_TOKEN", "Entra ID Token", Some("Optional: short-lived Microsoft Entra access token"), None),
        )
    }
}

impl ProviderDef for AzureProvider {
    type Provider = OpenAiCompatibleProvider;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<goose_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(async move {
            let config = crate::config::Config::global();
            let endpoint: String = config.get_param("AZURE_OPENAI_ENDPOINT")?;
            let deployment_name: String = config.get_param("AZURE_OPENAI_DEPLOYMENT_NAME")?;
            let api_version: Option<String> = config
                .get_param("AZURE_OPENAI_API_VERSION")
                .ok()
                .or_else(|| {
                    if is_v1_endpoint(&endpoint) {
                        None
                    } else {
                        Some(AZURE_DEFAULT_API_VERSION.to_string())
                    }
                });

            let api_key = config
                .get_secret("AZURE_OPENAI_API_KEY")
                .ok()
                .filter(|key: &String| !key.is_empty());
            let ad_token = config
                .get_secret("AZURE_OPENAI_AD_TOKEN")
                .ok()
                .filter(|token: &String| !token.is_empty());
            let auth = AzureAuth::new(api_key, ad_token).map_err(|e| match e {
                AuthError::Credentials(msg) => anyhow::anyhow!("Credentials error: {}", msg),
                AuthError::TokenExchange(msg) => anyhow::anyhow!("Token exchange error: {}", msg),
            })?;

            let auth_provider = AzureAuthProvider { auth };
            let host = format!("{}/openai", endpoint.trim_end_matches('/'));
            let mut api_client = ApiClient::new_with_tls(
                host,
                AuthMethod::Custom(Box::new(auth_provider)),
                tls_config,
            )?
            .with_request_builder(crate::session_context::session_id_request_builder());
            if let Some(version) = api_version {
                api_client = api_client.with_query(vec![("api-version".to_string(), version)]);
            }

            Ok(OpenAiCompatibleProvider::new(
                AZURE_PROVIDER_NAME.to_string(),
                api_client,
                format!("deployments/{}/", deployment_name),
            ))
        })
    }
}

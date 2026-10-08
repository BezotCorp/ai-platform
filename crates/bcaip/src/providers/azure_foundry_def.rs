use anyhow::Result;
use async_trait::async_trait;
use bcaip_provider_types::base::{ProviderDescriptor, ProviderMetadata};
use bcaip_providers::api_client::{AuthMethod, AuthProvider, TlsConfig};
use bcaip_providers::azure_foundry::{AzureFoundryProvider, EndpointKind, endpoint_kind};
use futures::future::BoxFuture;
use std::sync::Arc;

use crate::config::{Config, ExtensionConfig};
use crate::providers::azureauth::{AzureAuth, AzureCredentials};
use crate::providers::base::ProviderDef;
const AZURE_PROJECT_ENTRA_RESOURCE: &str = "https://ai.azure.com";
const AZURE_MAAS_ENTRA_RESOURCE: &str = "https://ml.azure.com";

enum AuthHeader {
    ApiKey,
    Bearer,
}

struct AzureFoundryAuthProvider {
    auth: Arc<AzureAuth>,
    header: AuthHeader,
}

fn auth_method(auth: &Arc<AzureAuth>, header: AuthHeader) -> AuthMethod {
    match (auth.credential_type(), header) {
        (AzureCredentials::ApiKey(key), AuthHeader::ApiKey) => AuthMethod::ApiKey {
            header_name: "api-key".to_string(),
            key: key.clone(),
        },
        (AzureCredentials::ApiKey(key), AuthHeader::Bearer) => AuthMethod::BearerToken(key.clone()),
        (_, header) => AuthMethod::Custom(Box::new(AzureFoundryAuthProvider {
            auth: Arc::clone(auth),
            header,
        })),
    }
}

#[async_trait]
impl AuthProvider for AzureFoundryAuthProvider {
    async fn get_auth_header(&self) -> Result<(String, String)> {
        let token = self.auth.get_token().await?;
        match &self.header {
            AuthHeader::ApiKey => Ok(("api-key".to_string(), token.token_value)),
            AuthHeader::Bearer => Ok((
                "Authorization".to_string(),
                format!("Bearer {}", token.token_value),
            )),
        }
    }

    async fn refresh_credentials(&self) -> Result<()> {
        self.auth.invalidate_token().await;
        Ok(())
    }
}

pub struct AzureFoundryProviderDef;

impl ProviderDescriptor for AzureFoundryProviderDef {
    fn metadata() -> ProviderMetadata {
        AzureFoundryProvider::metadata()
    }
}

impl ProviderDef for AzureFoundryProviderDef {
    type Provider = AzureFoundryProvider;

    fn from_env(
        _extensions: Vec<ExtensionConfig>,
        tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(from_env(tls_config))
    }
}

pub async fn from_env(tls_config: Option<TlsConfig>) -> Result<AzureFoundryProvider> {
    let config = Config::global();
    let endpoint: String = config.get_param("AZURE_FOUNDRY_ENDPOINT")?;
    let api_version = config.get_param("AZURE_FOUNDRY_API_VERSION").ok();
    let maas_model = config
        .get_param::<String>("AZURE_FOUNDRY_MODEL")
        .ok()
        .filter(|model| !model.trim().is_empty());
    let api_key = config
        .get_secret::<String>("AZURE_FOUNDRY_API_KEY")
        .ok()
        .filter(|key| !key.is_empty());
    let ad_token = config
        .get_secret::<String>("AZURE_FOUNDRY_AD_TOKEN")
        .ok()
        .filter(|token| !token.is_empty());
    let endpoint_kind = endpoint_kind(&endpoint);
    let resource = if endpoint_kind == EndpointKind::Maas {
        AZURE_MAAS_ENTRA_RESOURCE
    } else {
        AZURE_PROJECT_ENTRA_RESOURCE
    };
    let auth = Arc::new(AzureAuth::new_with_resource(
        api_key,
        ad_token,
        resource.to_string(),
    )?);
    let anthropic_auth = match auth.credential_type() {
        AzureCredentials::ApiKey(key) => AuthMethod::ApiKey {
            header_name: "x-api-key".to_string(),
            key: key.clone(),
        },
        _ => auth_method(&auth, AuthHeader::Bearer),
    };
    let api_key_auth_header = || match auth.credential_type() {
        AzureCredentials::ApiKey(_) => AuthHeader::ApiKey,
        _ => AuthHeader::Bearer,
    };
    let chat_auth_header = match auth.credential_type() {
        AzureCredentials::ApiKey(_) if endpoint_kind == EndpointKind::Maas => AuthHeader::Bearer,
        _ => api_key_auth_header(),
    };

    AzureFoundryProvider::create(
        endpoint,
        api_version,
        maas_model,
        auth_method(&auth, chat_auth_header),
        auth_method(&auth, api_key_auth_header()),
        anthropic_auth,
        auth_method(&auth, api_key_auth_header()),
        tls_config,
        Some(crate::session_context::session_id_request_builder()),
    )
}

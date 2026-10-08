use anyhow::Result;
use bcaip_provider_types::base::{ProviderDescriptor, ProviderMetadata};
use bcaip_providers::api_client::TlsConfig;
use bcaip_providers::google::{GOOGLE_API_HOST, GoogleProvider};
use futures::future::BoxFuture;

use crate::config::{Config, ExtensionConfig};
use crate::providers::base::ProviderDef;
pub struct GoogleProviderDef;

impl ProviderDescriptor for GoogleProviderDef {
    fn metadata() -> ProviderMetadata {
        GoogleProvider::metadata().with_setup(
            bcaip_provider_types::ProviderSetupMetadata::api_key(
                bcaip_provider_types::ProviderSetupGroup::Default,
            )
            .with_docs_url("https://aistudio.google.com/apikey"),
        )
    }
}

impl ProviderDef for GoogleProviderDef {
    type Provider = GoogleProvider;

    fn from_env(
        _extensions: Vec<ExtensionConfig>,
        tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(from_env(tls_config))
    }
}

pub async fn from_env(tls_config: Option<TlsConfig>) -> Result<GoogleProvider> {
    let config = Config::global();
    let api_key: String = config.get_secret("GOOGLE_API_KEY")?;
    let host: String = config
        .get_param("GOOGLE_HOST")
        .unwrap_or_else(|_| GOOGLE_API_HOST.to_string());

    let thinking_budget = config.get_param("GEMINI25_THINKING_BUDGET").ok();

    GoogleProvider::new(
        host,
        api_key,
        tls_config,
        Some(crate::session_context::session_id_request_builder()),
        thinking_budget,
    )
}

use crate::config::{Config, ConfigError, ExtensionConfig};
use crate::providers::base::ProviderDef;
use anyhow::Result;
use bcaip_provider_types::base::{ProviderDescriptor, ProviderMetadata};
use bcaip_provider_types::{
    ProviderSetupCategory, ProviderSetupGroup, ProviderSetupMetadata, ProviderSetupMethod,
};
use bcaip_providers::api_client::TlsConfig;
use bcaip_providers::databricks::DatabricksProvider;
use bcaip_providers::databricks_auth::{
    DatabricksAuth, DatabricksOauthTokenProvider, DatabricksRefreshHook,
    DatabricksSessionIdProvider, DatabricksTokenResolver,
};
use futures::future::BoxFuture;
use std::sync::Arc;
pub struct DatabricksProviderDef;

impl ProviderDescriptor for DatabricksProviderDef {
    fn metadata() -> ProviderMetadata {
        DatabricksProvider::metadata().with_setup(
            ProviderSetupMetadata::new(
                ProviderSetupCategory::Model,
                ProviderSetupMethod::HostWithOauthFallback,
                ProviderSetupGroup::Default,
            )
            .with_capabilities(false, true, false)
            .with_field(
                "DATABRICKS_HOST",
                "Host URL",
                Some("https://dbc-...cloud.databricks.com"),
                None,
            )
            .with_field(
                "DATABRICKS_TOKEN",
                "Access Token",
                Some("Paste your access token"),
                None,
            ),
        )
    }
}

impl ProviderDef for DatabricksProviderDef {
    type Provider = DatabricksProvider;

    fn from_env(
        _extensions: Vec<ExtensionConfig>,
        tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(from_env(tls_config))
    }
}

pub async fn from_env(tls_config: Option<TlsConfig>) -> Result<DatabricksProvider> {
    let config = Config::global();
    let host = load_host(config)?;
    let retry_config = DatabricksProvider::load_retry_config(|key| config.get_param(key).ok());
    let auth = if let Ok(api_key) = config.get_secret("DATABRICKS_TOKEN") {
        DatabricksAuth::token(api_key)
    } else {
        DatabricksAuth::oauth(host.clone())
    };

    DatabricksProvider::new(
        host,
        auth,
        retry_config,
        tls_config,
        Some(oauth_token_provider()),
        Some(token_resolver()),
        Some(crate::session_context::session_id_request_builder()),
        resolve_instance_id(),
        Some(refresh_hook()),
        Some(session_id_provider()),
    )
}

pub async fn cleanup() -> Result<()> {
    crate::providers::oauth::cleanup_oauth_cache()
}

fn load_host(config: &Config) -> Result<String> {
    let mut host: Result<String, ConfigError> = config.get_param("DATABRICKS_HOST");
    if host.is_err() {
        host = config.get_secret("DATABRICKS_HOST")
    }

    host.map_err(|_| {
        ConfigError::NotFound(
            "Did not find DATABRICKS_HOST in either config file or keyring".to_string(),
        )
        .into()
    })
}

fn resolve_instance_id() -> Option<String> {
    let enabled = Config::global()
        .get_param::<bool>("BCAIP_DATABRICKS_CLIENT_REQUEST_ID")
        .unwrap_or(false);
    enabled.then(|| crate::instance_id::get_instance_id().to_string())
}

fn token_resolver() -> DatabricksTokenResolver {
    Arc::new(|| {
        Config::global()
            .get_secret::<String>("DATABRICKS_TOKEN")
            .ok()
    })
}

fn refresh_hook() -> DatabricksRefreshHook {
    Arc::new(|| Config::global().invalidate_secrets_cache())
}

fn session_id_provider() -> DatabricksSessionIdProvider {
    Arc::new(crate::session_context::current_session_id)
}

fn oauth_token_provider() -> DatabricksOauthTokenProvider {
    Arc::new(|host, client_id, redirect_url, scopes| {
        Box::pin(async move {
            crate::providers::oauth::get_oauth_token_async(
                &host,
                &client_id,
                &redirect_url,
                &scopes,
            )
            .await
        })
    })
}

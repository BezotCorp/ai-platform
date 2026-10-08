use crate::{
    config::{Config, ConfigError, ExtensionConfig},
    providers::base::ProviderDef,
};
use anyhow::{Result, bail};
use bcaip_provider_types::base::{ProviderDescriptor, ProviderMetadata};
use bcaip_providers::api_client::{ApiClient, AuthMethod, TlsConfig};
use bcaip_providers::openrouter::OpenRouterProvider;
use futures::future::BoxFuture;
use serde_json::Value;
use std::collections::HashMap;

const OPENROUTER_PARAMETERS_CONFIG_KEY: &str = "OPENROUTER_PARAMETERS";

pub struct OpenRouterProviderDef;

impl ProviderDescriptor for OpenRouterProviderDef {
    fn metadata() -> ProviderMetadata {
        OpenRouterProvider::metadata()
            .with_setup(
                bcaip_provider_types::ProviderSetupMetadata::api_key(
                    bcaip_provider_types::ProviderSetupGroup::Default,
                )
                .with_docs_url("https://openrouter.ai/keys"),
            )
            .with_setup_steps(vec![
                "Go to https://openrouter.ai/settings/keys",
                "Click 'Create' or use an existing API key",
                "Copy the key and paste it above",
            ])
    }
}

impl ProviderDef for OpenRouterProviderDef {
    type Provider = OpenRouterProvider;

    fn from_env(
        _extensions: Vec<ExtensionConfig>,
        tls_config: Option<TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(from_env(tls_config))
    }
}

async fn from_env(tls_config: Option<TlsConfig>) -> Result<OpenRouterProvider> {
    let config = Config::global();
    let api_key: String = config.get_secret("OPENROUTER_API_KEY")?;
    let host: String = config
        .get_param("OPENROUTER_HOST")
        .unwrap_or_else(|_| "https://openrouter.ai".to_string());
    let configured_parameters = configured_openrouter_parameters(config)?;

    let api_client = ApiClient::new_with_tls(host, AuthMethod::BearerToken(api_key), tls_config)?
        .with_request_builder(crate::session_context::session_id_request_builder())
        .with_header("HTTP-Referer", "https://bcaip.bezotcorp.com")?
        .with_header("X-Title", "BCAIP")?
        .with_header("X-OpenRouter-Categories", "cli-agent,productivity")?;

    Ok(OpenRouterProvider::new(
        api_client,
        configured_parameters,
        Some(Box::new(crate::session_context::current_session_id)),
    ))
}

fn configured_openrouter_parameters(config: &Config) -> Result<Option<HashMap<String, Value>>> {
    match config.get_param::<Value>(OPENROUTER_PARAMETERS_CONFIG_KEY) {
        Ok(raw) => parse_openrouter_parameters(raw).map(Some),
        Err(ConfigError::NotFound(_)) => Ok(None),
        Err(err) => Err(err.into()),
    }
}

fn parse_openrouter_parameters(raw: Value) -> Result<HashMap<String, Value>> {
    match raw {
        Value::Object(params) => Ok(params.into_iter().collect()),
        Value::String(raw_json) => match serde_json::from_str::<Value>(&raw_json)? {
            Value::Object(params) => Ok(params.into_iter().collect()),
            _ => bail!("{OPENROUTER_PARAMETERS_CONFIG_KEY} must be a JSON object"),
        },
        _ => bail!("{OPENROUTER_PARAMETERS_CONFIG_KEY} must be a JSON object"),
    }
}

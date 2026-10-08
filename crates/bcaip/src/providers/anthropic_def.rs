use crate::config::Config;
use crate::providers::base::ProviderDef;
use crate::providers::command_auth::CommandAuthProvider;
use crate::providers::custom_provider_config::ConfigKeyResolver;
use crate::session_context::{
    session_id_request_builder, session_id_request_builder_with_header_override,
};
use anyhow::Result;
use bcaip_provider_types::base::{ProviderDescriptor, ProviderMetadata};
use bcaip_provider_types::formats::{AnthropicFormatOptions, PrefixMismatchBehavior};
use bcaip_provider_types::{ProviderSetupGroup, ProviderSetupMetadata};
use bcaip_providers::anthropic;
use bcaip_providers::anthropic::{
    ANTHROPIC_API_VERSION, AnthropicProvider, AnthropicProviderBuilder,
};
use bcaip_providers::api_client::{ApiClient, AuthMethod, TlsConfig};
use bcaip_providers::declarative::DeclarativeProviderConfig;
use futures::future::BoxFuture;

pub struct AnthropicProviderDef;

impl ProviderDescriptor for AnthropicProviderDef {
    fn metadata() -> ProviderMetadata {
        AnthropicProvider::metadata().with_setup(
            ProviderSetupMetadata::api_key(ProviderSetupGroup::Default)
                .with_docs_url("https://console.anthropic.com/settings/keys"),
        )
    }
}

impl ProviderDef for AnthropicProviderDef {
    type Provider = AnthropicProvider;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<bcaip_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(from_env(tls_config))
    }
}

async fn from_env(
    tls_config: Option<bcaip_providers::api_client::TlsConfig>,
) -> Result<AnthropicProvider> {
    let config = crate::config::Config::global();
    let api_key: String = config.get_secret("ANTHROPIC_API_KEY")?;
    let host: String = config
        .get_param("ANTHROPIC_HOST")
        .unwrap_or_else(|_| "https://api.anthropic.com".to_string());

    let timeout_secs: u64 = config
        .get_param("ANTHROPIC_TIMEOUT")
        .unwrap_or(bcaip_providers::api_client::DEFAULT_PROVIDER_TIMEOUT_SECS);

    let mut format_options = AnthropicFormatOptions::native();
    if let Ok(value) = config.get_param::<String>("ANTHROPIC_PREFIX_MISMATCH_BEHAVIOR") {
        format_options.prefix_mismatch_behavior =
            match value.as_str() {
                "off" => None,
                value => Some(value.parse::<PrefixMismatchBehavior>().map_err(|e| {
                    anyhow::anyhow!("invalid ANTHROPIC_PREFIX_MISMATCH_BEHAVIOR: {e}")
                })?),
            };
    }

    let auth = AuthMethod::ApiKey {
        header_name: "x-api-key".to_string(),
        key: api_key,
    };

    let api_client = ApiClient::with_timeout_and_tls(
        host,
        auth,
        std::time::Duration::from_secs(timeout_secs),
        tls_config,
    )?
    .with_request_builder(session_id_request_builder())
    .with_header("anthropic-version", ANTHROPIC_API_VERSION)?;

    Ok(AnthropicProviderBuilder::new(api_client)
        .format_options(format_options)
        .build())
}

pub fn from_custom_config(
    config: DeclarativeProviderConfig,
    tls_config: Option<TlsConfig>,
) -> Result<AnthropicProvider> {
    let auth_override = config.auth.clone();
    let request_builder = session_id_request_builder_with_header_override(
        config.session_id_header_override.as_deref(),
    )?;
    anthropic::from_declarative_config(config, tls_config, ConfigKeyResolver::new(Config::global()))
        .map(|builder| {
            builder
                .map_api_client(|api_client| {
                    let api_client = api_client.with_request_builder(request_builder);
                    match auth_override {
                        Some(auth_config) => api_client.with_auth(AuthMethod::Custom(Box::new(
                            CommandAuthProvider::new(&auth_config, "x-api-key", ""),
                        ))),
                        None => api_client,
                    }
                })
                .build()
        })
}

use crate::config::{self, Config};
use crate::providers::base::ProviderDef;
use crate::providers::command_auth::CommandAuthProvider;
use crate::providers::custom_provider_config::ConfigKeyResolver;
use crate::session_context::{
    session_id_request_builder, session_id_request_builder_with_header_override,
};
use anyhow::Result;
use bcaip_provider_types::base::ProviderDescriptor;
use bcaip_providers::api_client::{ApiClient, AuthMethod};
use bcaip_providers::declarative::DeclarativeProviderConfig;
use bcaip_providers::ollama;
use bcaip_providers::ollama::{
    OLLAMA_DEFAULT_CHUNK_TIMEOUT_SECS, OLLAMA_DEFAULT_PORT, OLLAMA_HOST, OLLAMA_PROVIDER_NAME,
    OLLAMA_TIMEOUT, OllamaOptions, OllamaProvider, OllamaProviderBuilder,
};
use futures::future::BoxFuture;
use std::time::Duration;
use url::Url;

pub struct OllamaProviderDef;

impl ProviderDescriptor for OllamaProviderDef {
    fn metadata() -> bcaip_provider_types::base::ProviderMetadata {
        OllamaProvider::metadata().with_setup(
            bcaip_provider_types::ProviderSetupMetadata::new(
                bcaip_provider_types::ProviderSetupCategory::Model,
                bcaip_provider_types::ProviderSetupMethod::ConfigFields,
                bcaip_provider_types::ProviderSetupGroup::Default,
            )
            .with_docs_url("https://ollama.com")
            .with_field(
                "OLLAMA_HOST",
                "Host",
                Some("localhost or http://localhost:11434"),
                Some("http://localhost:11434"),
            ),
        )
    }
}

impl ProviderDef for OllamaProviderDef {
    type Provider = OllamaProvider;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<bcaip_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<Self::Provider>> {
        Box::pin(from_env(tls_config))
    }
}

pub async fn from_env(
    tls_config: Option<bcaip_providers::api_client::TlsConfig>,
) -> Result<OllamaProvider> {
    let config = crate::config::Config::global();
    let host: String = config
        .get_param("OLLAMA_HOST")
        .unwrap_or_else(|_| OLLAMA_HOST.to_string());

    let timeout: Duration =
        Duration::from_secs(config.get_param("OLLAMA_TIMEOUT").unwrap_or(OLLAMA_TIMEOUT));

    let base = if host.starts_with("http://") || host.starts_with("https://") {
        host.clone()
    } else {
        format!("http://{}", host)
    };

    let mut base_url = Url::parse(&base).map_err(|e| anyhow::anyhow!("Invalid base URL: {e}"))?;

    let explicit_port = host.contains(':');
    let is_localhost = host == "localhost" || host == "127.0.0.1" || host == "::1";

    if base_url.port().is_none() && !explicit_port && !host.starts_with("http") && is_localhost {
        base_url
            .set_port(Some(OLLAMA_DEFAULT_PORT))
            .map_err(|_| anyhow::anyhow!("Failed to set default port"))?;
    }

    let api_client = ApiClient::with_timeout_and_tls(
        base_url.to_string(),
        AuthMethod::NoAuth,
        timeout,
        tls_config,
    )?
    .with_request_builder(session_id_request_builder());

    Ok(OllamaProviderBuilder::new(api_client)
        .name(OLLAMA_PROVIDER_NAME)
        .options(options_from_config())
        .build())
}

pub fn from_custom_config(
    config: DeclarativeProviderConfig,
    tls_config: Option<bcaip_providers::api_client::TlsConfig>,
) -> Result<OllamaProvider> {
    let auth_override = config.auth.clone();
    let request_builder = session_id_request_builder_with_header_override(
        config.session_id_header_override.as_deref(),
    )?;
    ollama::from_declarative_config(config, tls_config, ConfigKeyResolver::new(Config::global()))
        .map(|builder| {
            builder
                .map_api_client(|api_client| {
                    let api_client = api_client.with_request_builder(request_builder);
                    match auth_override {
                        Some(auth_config) => api_client.with_auth(AuthMethod::Custom(Box::new(
                            CommandAuthProvider::new(&auth_config, "Authorization", "Bearer "),
                        ))),
                        None => api_client,
                    }
                })
                .options(options_from_config())
                .build()
        })
}

pub fn options_from_config() -> OllamaOptions {
    let config = crate::config::Config::global();

    let input_limit = match config.get_param::<usize>("BCAIP_INPUT_LIMIT") {
        Ok(limit) if limit > 0 => Some(limit),
        Ok(_) => None,
        Err(crate::config::ConfigError::NotFound(_)) => None,
        Err(e) => {
            tracing::warn!("Invalid BCAIP_INPUT_LIMIT value: {}", e);
            None
        }
    };

    let stream_usage = match config.get_param::<bool>("OLLAMA_STREAM_USAGE") {
        Ok(val) => val,
        // Key not set: default to true. Ollama supports stream_options since
        // mid-2025 and most installs benefit from token usage tracking.
        Err(crate::config::ConfigError::NotFound(_)) => true,
        // Invalid value (e.g. "0", "yes", typo): warn and disable stream_options
        // so users who intended to opt out aren't silently left hanging.
        Err(e) => {
            tracing::warn!(
                "Invalid OLLAMA_STREAM_USAGE value ({}); disabling stream_options. \
                     Use true or false.",
                e
            );
            false
        }
    };

    OllamaOptions {
        input_limit,
        stream_usage,
        chunk_timeout_secs: resolve_ollama_chunk_timeout(config),
    }
}

/// Resolve the per-chunk stream timeout from config.
/// Priority: OLLAMA_STREAM_TIMEOUT > BCAIP_STREAM_TIMEOUT > OLLAMA_TIMEOUT > default (120s).
/// Zero values are treated as invalid and skipped, since a zero timeout would
/// cause every chunk after the first to be treated as a stall.
fn resolve_ollama_chunk_timeout(config: &config::Config) -> u64 {
    if let Ok(val) = config.get_param::<u64>("OLLAMA_STREAM_TIMEOUT")
        && val > 0
    {
        return val;
    }
    if let Ok(val) = config.get_param::<u64>("BCAIP_STREAM_TIMEOUT")
        && val > 0
    {
        return val;
    }
    match config.get_param::<u64>("OLLAMA_TIMEOUT") {
        Ok(val) if val > 0 => val,
        _ => OLLAMA_DEFAULT_CHUNK_TIMEOUT_SECS,
    }
}

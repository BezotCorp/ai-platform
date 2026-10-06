use crate::acp::{
    ACP_CURRENT_MODEL, AcpProvider, AcpProviderConfig, extension_configs_to_mcp_servers,
};
use crate::config::search_path::SearchPaths;
use crate::config::{Config, ConfigError};
use crate::providers::base::{ProviderDef, current_working_dir};
use anyhow::Result;
use futures::future::BoxFuture;
use bcaip_provider_types::ProviderSetupMetadata;
use bcaip_provider_types::base::{ProviderDescriptor, ProviderMetadata};
use bcaip_provider_types::goose_mode::GooseMode;
use std::{collections::HashMap, path::PathBuf};
pub(crate) const CODEX_ACP_PROVIDER_NAME: &str = "codex-acp";
const CODEX_ACP_DOC_URL: &str = "https://github.com/agentclientprotocol/codex-acp";

pub struct CodexAcpProvider;

fn resolve_goose_mode(
    configured_mode: Result<GooseMode, ConfigError>,
) -> Result<GooseMode, ConfigError> {
    match configured_mode {
        Ok(mode) => Ok(mode),
        Err(ConfigError::NotFound(_)) => Ok(GooseMode::Auto),
        Err(error) => Err(error),
    }
}

impl bcaip_provider_types::base::ProviderDescriptor for CodexAcpProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            CODEX_ACP_PROVIDER_NAME,
            "Codex ACP",
            "Use goose with ChatGPT Plus/Pro or OpenAI API credits via the codex-acp adapter.",
            ACP_CURRENT_MODEL,
            vec![],
            CODEX_ACP_DOC_URL,
            vec![],
        )
        .with_setup_steps(vec![
            "Verify `codex-acp --version` shows `@agentclientprotocol/codex-acp`",
            "If `--version` is rejected, remove `@agentclientprotocol/codex-acp`: `npm uninstall -g @agentclientprotocol/codex-acp`",
            "If `codex-acp` is missing or was removed, install `@agentclientprotocol/codex-acp`: `npm install -g @agentclientprotocol/codex-acp`",
            "Authenticate with OpenAI: run `codex` and follow the prompts",
        ])
        .with_setup(
            ProviderSetupMetadata::cli_agent(CODEX_ACP_PROVIDER_NAME, &["codex-acp", "codex_cli", "codex"])
                .with_acp()
                .with_docs_url("https://github.com/openai/codex")
                .with_capabilities(true, true, true),
        )
    }
}

impl ProviderDef for CodexAcpProvider {
    type Provider = AcpProvider;

    fn from_env(
        extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<goose_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<AcpProvider>> {
        Self::from_env_with_working_dir(extensions, current_working_dir(), tls_config)
    }

    fn from_env_with_working_dir(
        extensions: Vec<crate::config::ExtensionConfig>,
        working_dir: PathBuf,
        _tls_config: Option<goose_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<AcpProvider>> {
        Box::pin(async move {
            let config = Config::global();
            // with_npm() includes npm global bin dir (desktop app PATH may not)
            let resolved_command = SearchPaths::builder()
                .with_npm()
                .resolve(CODEX_ACP_PROVIDER_NAME)?;
            let goose_mode = resolve_goose_mode(config.get_goose_mode_strict())?;
            let mcp_servers = extension_configs_to_mcp_servers(&extensions);

            let mode_mapping = HashMap::from([
                (GooseMode::Auto, vec!["agent-full-access".to_string()]),
                (GooseMode::SmartApprove, vec!["agent".to_string()]),
                (GooseMode::Approve, vec!["read-only".to_string()]),
                (GooseMode::Chat, vec!["read-only".to_string()]),
            ]);

            let provider_config = AcpProviderConfig {
                command: resolved_command,
                args: vec![],
                env: vec![],
                env_remove: vec![],
                work_dir: working_dir,
                mcp_servers,
                session_mode_id: None,
                session_config_options: vec![],
                model_config_option_id: Some("model".to_string()),
                mode_mapping,
                notification_callback: None,
            };

            let metadata = Self::metadata();
            AcpProvider::connect(metadata.name, goose_mode, provider_config).await
        })
    }
}

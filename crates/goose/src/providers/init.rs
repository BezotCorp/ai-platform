use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use super::amp_acp::AmpAcpProvider;
use super::avian::AvianProvider;
use super::azure::AzureProvider;
#[cfg(feature = "aws-providers")]
use super::bedrock::BedrockProvider;
use super::chatgpt_codex::ChatGptCodexProvider;
use super::claude_acp::ClaudeAcpProvider;
use super::claude_code::ClaudeCodeProvider;
use super::codex::CodexProvider;
use super::codex_acp::CodexAcpProvider;
use super::copilot_acp::CopilotAcpProvider;
use super::cursor_agent::CursorAgentProvider;
use super::gcpvertexai::GcpVertexAIProvider;
use super::gemini_cli::GeminiCliProvider;
use super::gemini_oauth::GeminiOAuthProvider;
use super::githubcopilot::GithubCopilotProvider;
use super::gondola::GondolaProvider;
use super::huggingface::HuggingFaceProvider;
use super::kimicode::KimiCodeProvider;
use super::litellm::LiteLLMProvider;
use super::nanogpt::NanoGptProvider;
use super::pi_acp::PiAcpProvider;
use super::provider_registry::ProviderRegistry;
#[cfg(feature = "aws-providers")]
use super::sagemaker_tgi::SageMakerTgiProvider;
use super::snowflake_def::SnowflakeProviderDef;
use super::tetrate::TetrateProvider;
use super::xai::XaiProvider;
use super::xai_oauth::XaiOAuthProvider;
use crate::providers::databricks_def::{self, DatabricksProviderDef};
use crate::providers::databricks_v2_def::{self, DatabricksV2ProviderDef};
use crate::providers::google_def::GoogleProviderDef;
use crate::providers::muse_code_def::{MuseCodeProvider, MuseCodeProviderDef};
use crate::providers::{
    ollama_def::OllamaProviderDef, openai_def::OpenAiProviderDef,
    openrouter_def::OpenRouterProviderDef,
};
use crate::{
    config::ExtensionConfig,
    providers::{
        anthropic_def::AnthropicProviderDef, azure_foundry_def::AzureFoundryProviderDef,
        base::ProviderType,
    },
};
use crate::{
    config::declarative_providers::register_declarative_providers,
    providers::provider_registry::ProviderEntry,
};
use anyhow::Result;
#[cfg(feature = "local-inference")]
use bcaip_local_inference::LocalInferenceProvider;
use bcaip_provider_types::base::{Provider, ProviderMetadata};
use tokio::sync::OnceCell;
static REGISTRY: OnceCell<RwLock<ProviderRegistry>> = OnceCell::const_new();

async fn init_registry() -> RwLock<ProviderRegistry> {
    let tls_config =
        crate::config::tls::provider_tls_config_from_config(crate::config::Config::global())
            .expect("failed to load provider TLS config");
    let mut registry = ProviderRegistry::new(tls_config).with_providers(|registry| {
        use super::inventory::registrations;
        registry.register_with_inventory::<AmpAcpProvider>(
            false,
            Some(registrations::amp_acp_inventory()),
        );
        registry.register_with_inventory::<AnthropicProviderDef>(
            true,
            Some(registrations::anthropic_inventory()),
        );
        registry.register::<AvianProvider>(false);
        registry.register::<AzureProvider>(false);
        registry.register_with_inventory::<AzureFoundryProviderDef>(
            true,
            Some(registrations::azure_foundry_inventory()),
        );
        #[cfg(feature = "aws-providers")]
        registry.register::<BedrockProvider>(false);
        #[cfg(feature = "local-inference")]
        registry.register::<LocalInferenceProvider>(false);
        registry.register_with_inventory::<ChatGptCodexProvider>(
            true,
            Some(registrations::chatgpt_codex_inventory()),
        );
        registry.register_with_inventory::<ClaudeAcpProvider>(
            false,
            Some(registrations::claude_acp_inventory()),
        );
        registry.register::<ClaudeCodeProvider>(true);
        registry.register_with_inventory::<CodexAcpProvider>(
            false,
            Some(registrations::codex_acp_inventory()),
        );
        registry.register_with_inventory::<CopilotAcpProvider>(
            false,
            Some(registrations::copilot_acp_inventory()),
        );
        registry.register::<CodexProvider>(true);
        registry.register_with_inventory::<CursorAgentProvider>(
            false,
            Some(registrations::refresh_only()),
        );
        registry.register_with_inventory::<DatabricksProviderDef>(
            true,
            Some(registrations::refresh_only()),
        );
        registry.register_with_inventory::<DatabricksV2ProviderDef>(
            false,
            Some(registrations::refresh_only()),
        );
        registry.register_with_inventory::<GcpVertexAIProvider>(
            false,
            Some(registrations::refresh_only()),
        );
        registry.register::<GeminiCliProvider>(false);
        registry.register_with_inventory::<GeminiOAuthProvider>(
            false,
            Some(registrations::gemini_oauth_inventory()),
        );
        registry.register_with_inventory::<GithubCopilotProvider>(
            false,
            Some(registrations::refresh_only()),
        );
        registry.register::<GondolaProvider>(false);
        registry.register_with_inventory::<GoogleProviderDef>(
            true,
            Some(registrations::google_inventory()),
        );
        registry.register_with_inventory::<HuggingFaceProvider>(
            true,
            Some(registrations::huggingface_inventory()),
        );
        registry.register_with_inventory::<KimiCodeProvider>(
            true,
            Some(registrations::kimi_code_inventory()),
        );
        registry.register_with_inventory::<LiteLLMProvider>(
            false,
            Some(registrations::refresh_only().with_configured(|| {
                let config = crate::config::Config::global();
                config
                    .get_param::<serde_json::Value>("LITELLM_HOST")
                    .is_ok()
                    || config
                        .get_secret::<serde_json::Value>("LITELLM_API_KEY")
                        .is_ok()
            })),
        );
        registry
            .register_with_inventory::<NanoGptProvider>(true, Some(registrations::refresh_only()));
        registry.register_with_inventory::<MuseCodeProviderDef>(
            true,
            Some(registrations::muse_code_inventory()),
        );
        registry.register_with_inventory::<OllamaProviderDef>(
            true,
            Some(registrations::ollama_inventory()),
        );
        registry.register_with_inventory::<OpenAiProviderDef>(
            true,
            Some(registrations::openai_inventory()),
        );
        registry.register_with_inventory::<OpenRouterProviderDef>(
            true,
            Some(registrations::refresh_only().with_configured(|| {
                let config = crate::config::Config::global();
                config
                    .get_secret::<serde_json::Value>("OPENROUTER_API_KEY")
                    .is_ok()
            })),
        );
        registry.register_with_inventory::<PiAcpProvider>(
            false,
            Some(registrations::pi_acp_inventory()),
        );
        #[cfg(feature = "aws-providers")]
        registry.register::<SageMakerTgiProvider>(false);
        registry.register::<SnowflakeProviderDef>(false);
        registry
            .register_with_inventory::<TetrateProvider>(true, Some(registrations::refresh_only()));
        registry.register_with_inventory::<XaiProvider>(false, Some(registrations::refresh_only()));
        registry.register_with_inventory::<XaiOAuthProvider>(
            true,
            Some(registrations::xai_oauth_inventory()),
        );
    });
    // Register cleanup functions for providers with cached state
    registry.set_cleanup(
        "github_copilot",
        Arc::new(|| Box::pin(GithubCopilotProvider::cleanup())),
    );
    registry.set_cleanup(
        "databricks",
        Arc::new(|| Box::pin(databricks_def::cleanup())),
    );
    registry.set_cleanup(
        "databricks_v2",
        Arc::new(|| Box::pin(databricks_v2_def::cleanup())),
    );
    registry.set_cleanup(
        "kimi_code",
        Arc::new(|| Box::pin(KimiCodeProvider::cleanup())),
    );
    registry.set_cleanup(
        "muse_code",
        Arc::new(|| Box::pin(MuseCodeProvider::cleanup())),
    );
    registry.set_cleanup(
        "chatgpt_codex",
        Arc::new(|| Box::pin(ChatGptCodexProvider::cleanup())),
    );
    registry.set_cleanup(
        "gemini_oauth",
        Arc::new(|| Box::pin(GeminiOAuthProvider::cleanup())),
    );
    registry.set_cleanup(
        "xai_oauth",
        Arc::new(|| Box::pin(XaiOAuthProvider::cleanup())),
    );
    registry.set_cleanup(
        "huggingface",
        Arc::new(|| Box::pin(HuggingFaceProvider::cleanup())),
    );

    if let Err(e) = load_custom_providers_into_registry(&mut registry) {
        tracing::warn!("Failed to load custom providers: {}", e);
    }
    RwLock::new(registry)
}

fn load_custom_providers_into_registry(registry: &mut ProviderRegistry) -> Result<()> {
    register_declarative_providers(registry)
}

async fn get_registry() -> &'static RwLock<ProviderRegistry> {
    REGISTRY.get_or_init(init_registry).await
}

pub async fn providers() -> Vec<(ProviderMetadata, ProviderType)> {
    get_registry()
        .await
        .read()
        .unwrap()
        .all_metadata_with_types()
}

pub async fn refresh_custom_providers() -> Result<()> {
    let registry = get_registry().await;
    registry.write().unwrap().remove_custom_providers();

    if let Err(e) = load_custom_providers_into_registry(&mut registry.write().unwrap()) {
        tracing::warn!("Failed to refresh custom providers: {}", e);
        return Err(e);
    }

    tracing::info!("Custom providers refreshed");
    Ok(())
}

pub async fn get_from_registry(name: &str) -> Result<ProviderEntry> {
    let guard = get_registry().await.read().unwrap();
    guard
        .entries
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("Unknown provider: {}", name))
        .cloned()
}

pub async fn inventory_identity(name: &str) -> Result<super::inventory::InventoryIdentityInput> {
    get_from_registry(name).await?.inventory_identity()
}

pub async fn create(name: &str, extensions: Vec<ExtensionConfig>) -> Result<Arc<dyn Provider>> {
    let entry = get_from_registry(name).await?;
    entry.create(extensions).await
}

pub async fn create_with_working_dir(
    name: &str,
    extensions: Vec<ExtensionConfig>,
    working_dir: PathBuf,
) -> Result<Arc<dyn Provider>> {
    let entry = get_from_registry(name).await?;
    entry.create_with_working_dir(extensions, working_dir).await
}

pub async fn create_with_default_model(
    name: impl AsRef<str>,
    extensions: Vec<ExtensionConfig>,
) -> Result<Arc<dyn Provider>> {
    get_from_registry(name.as_ref())
        .await?
        .create_with_default_model(extensions)
        .await
}

pub async fn cleanup_provider(name: &str) -> Result<()> {
    let cleanup_fn = {
        let registry = get_registry().await.read().unwrap();
        registry
            .entries
            .get(name)
            .and_then(|entry| entry.cleanup.clone())
    };
    if let Some(cleanup) = cleanup_fn {
        return cleanup().await;
    }
    Ok(())
}

pub async fn create_with_named_model(
    provider_name: &str,
    extensions: Vec<ExtensionConfig>,
) -> Result<Arc<dyn Provider>> {
    create(provider_name, extensions).await
}

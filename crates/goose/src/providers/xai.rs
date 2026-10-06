use super::base::ProviderDef;
use anyhow::Result;
use bcaip_provider_types::base::{ConfigKey, ProviderMetadata};
use futures::future::BoxFuture;
use goose_providers::api_client::{ApiClient, AuthMethod};
use goose_providers::openai_compatible::OpenAiCompatibleProvider;
const XAI_PROVIDER_NAME: &str = "xai";
pub const XAI_API_HOST: &str = "https://api.x.ai/v1";
pub const XAI_DEFAULT_MODEL: &str = "grok-4.5";

pub const XAI_DOC_URL: &str = "https://docs.x.ai/docs/overview";

pub struct XaiProvider;

pub fn xai_known_model_info() -> Vec<bcaip_provider_types::base::ModelInfo> {
    bcaip_provider_types::base::known_models_from_registry(XAI_PROVIDER_NAME)
}

impl bcaip_provider_types::base::ProviderDescriptor for XaiProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::with_models(
            XAI_PROVIDER_NAME,
            "xAI",
            "Grok models from xAI, including reasoning and multimodal capabilities",
            XAI_DEFAULT_MODEL,
            xai_known_model_info(),
            XAI_DOC_URL,
            vec![
                ConfigKey::new("XAI_API_KEY", true, true, None, true),
                ConfigKey::new("XAI_HOST", false, false, Some(XAI_API_HOST), false),
            ],
        )
        .with_setup(bcaip_provider_types::ProviderSetupMetadata::api_key(
            bcaip_provider_types::ProviderSetupGroup::Additional,
        ))
    }
}

impl ProviderDef for XaiProvider {
    type Provider = OpenAiCompatibleProvider;

    fn from_env(
        _extensions: Vec<crate::config::ExtensionConfig>,
        tls_config: Option<goose_providers::api_client::TlsConfig>,
    ) -> BoxFuture<'static, Result<OpenAiCompatibleProvider>> {
        Box::pin(async move {
            let config = crate::config::Config::global();
            let api_key: String = config.get_secret("XAI_API_KEY")?;
            let host: String = config
                .get_param("XAI_HOST")
                .unwrap_or_else(|_| XAI_API_HOST.to_string());

            let api_client =
                ApiClient::new_with_tls(host, AuthMethod::BearerToken(api_key), tls_config)?
                    .with_request_builder(crate::session_context::session_id_request_builder());

            Ok(OpenAiCompatibleProvider::new(
                XAI_PROVIDER_NAME.to_string(),
                api_client,
                String::new(),
            ))
        })
    }
}

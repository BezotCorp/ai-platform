use std::time::{Duration, Instant};
use std::{collections::HashMap, sync::Mutex};

use crate::anthropic::{ANTHROPIC_API_VERSION, AnthropicProvider, AnthropicProviderBuilder};
use crate::api_client::{ApiClient, AuthMethod, RequestBuilderDecorator, TlsConfig};
use crate::openai::{OpenAiProvider, OpenAiProviderBuilder};
use crate::openai_compatible::{OpenAiCompatibleProvider, handle_response_openai_compat};
use anyhow::Result;
use async_trait::async_trait;
use bcaip_provider_types::base::{
    ConfigKey, MessageStream, ModelInfo, Provider, ProviderDescriptor, ProviderMetadata,
};
use bcaip_provider_types::context_limit::ContextLimitResolver;
use bcaip_provider_types::conversations::Message;
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::formats::{extract_reasoning_effort, is_openai_responses_model};
use bcaip_provider_types::maybe_get_canonical_model;
use bcaip_provider_types::model::ModelConfig;
use futures::StreamExt;
use rmcp::model::Tool;
pub const AZURE_FOUNDRY_PROVIDER_NAME: &str = "azure_foundry";
pub const AZURE_FOUNDRY_DEFAULT_MODEL: &str = "Phi-4";
pub const AZURE_FOUNDRY_DOC_URL: &str =
    "https://learn.microsoft.com/azure/ai-foundry/foundry-models/how-to/inference";

const DEPLOYMENT_METADATA_TIMEOUT_SECS: u64 = 5;
const DEPLOYMENT_METADATA_TTL_SECS: u64 = 60;

pub const AZURE_FOUNDRY_KNOWN_MODELS: &[&str] = &[
    "Phi-4",
    "Phi-4-mini",
    "Meta-Llama-3.3-70B-Instruct",
    "Mistral-large-2411",
    "Cohere-command-r-plus-08-2024",
    "AI21-Jamba-1.5-Large",
    "DeepSeek-R1",
    "DeepSeek-V3",
    "glm-4.7",
    "Kimi-K2-Instruct",
    "claude-sonnet-4-6",
    "claude-opus-4-6",
    "gpt-5",
    "o3",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointKind {
    Maas,
    Resource,
    Project,
}

pub fn endpoint_kind(endpoint: &str) -> EndpointKind {
    if endpoint.contains("/api/projects/") {
        EndpointKind::Project
    } else if endpoint
        .split_once("://")
        .map(|(_, rest)| rest)
        .unwrap_or(endpoint)
        .split('/')
        .next()
        .is_some_and(|host| host.ends_with(".services.ai.azure.com"))
    {
        EndpointKind::Resource
    } else {
        EndpointKind::Maas
    }
}

pub fn is_project_endpoint(endpoint: &str) -> bool {
    endpoint_kind(endpoint) == EndpointKind::Project
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPublisher {
    OpenAi,
    Anthropic,
    Partner,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DeploymentMetadata {
    publisher: ModelPublisher,
    model_name: String,
}

#[derive(Default)]
struct DeploymentCache {
    deployments: HashMap<String, DeploymentMetadata>,
    fetched_at: Option<Instant>,
    fetch_failed: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DeploymentMetadataLookup {
    ContextDiscovery,
    InferenceRouting,
}

impl DeploymentCache {
    fn applies_to(&self, lookup: DeploymentMetadataLookup) -> bool {
        self.fetched_at.is_some_and(|fetched_at| {
            fetched_at.elapsed() < Duration::from_secs(DEPLOYMENT_METADATA_TTL_SECS)
        }) && (lookup == DeploymentMetadataLookup::ContextDiscovery || !self.fetch_failed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InferenceRoute {
    MaasChatCompletions,
    ProjectChatCompletions,
    ProjectResponses,
    AnthropicMessages,
}

fn inference_route(
    project_endpoint: bool,
    publisher: ModelPublisher,
    underlying_model: &str,
) -> InferenceRoute {
    if !project_endpoint {
        return InferenceRoute::MaasChatCompletions;
    }
    match publisher {
        ModelPublisher::Anthropic => InferenceRoute::AnthropicMessages,
        ModelPublisher::OpenAi if is_openai_responses_model(underlying_model) => {
            InferenceRoute::ProjectResponses
        }
        ModelPublisher::OpenAi | ModelPublisher::Partner => InferenceRoute::ProjectChatCompletions,
    }
}

impl ModelPublisher {
    fn from_azure(value: &str) -> Self {
        match value.to_ascii_lowercase().as_str() {
            "openai" => Self::OpenAi,
            "anthropic" => Self::Anthropic,
            _ => Self::Partner,
        }
    }

    fn from_model_name(value: &str) -> Self {
        let value = value.to_ascii_lowercase();
        if value.starts_with("claude") {
            Self::Anthropic
        } else if is_openai_responses_model(&value) {
            Self::OpenAi
        } else {
            Self::Partner
        }
    }
}

pub struct AzureFoundryProvider {
    chat: OpenAiCompatibleProvider,
    responses: Option<OpenAiProvider>,
    anthropic: Option<AnthropicProvider>,
    deployments_client: ApiClient,
    endpoint: String,
    api_version: Option<String>,
    maas_model: Option<String>,
    deployments: Mutex<DeploymentCache>,
}

impl ProviderDescriptor for AzureFoundryProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            AZURE_FOUNDRY_PROVIDER_NAME,
            "Azure AI Foundry",
            "OpenAI, Anthropic, and partner models deployed through Azure AI Foundry",
            AZURE_FOUNDRY_DEFAULT_MODEL,
            AZURE_FOUNDRY_KNOWN_MODELS.to_vec(),
            AZURE_FOUNDRY_DOC_URL,
            vec![
                ConfigKey::new("AZURE_FOUNDRY_ENDPOINT", true, false, None, true),
                ConfigKey::new("AZURE_FOUNDRY_API_KEY", false, true, Some(""), true),
                ConfigKey::new("AZURE_FOUNDRY_AD_TOKEN", false, true, Some(""), false),
                ConfigKey::new("AZURE_FOUNDRY_MODEL", false, false, None, true),
                ConfigKey::new("AZURE_FOUNDRY_API_VERSION", false, false, None, false),
            ],
        )
    }
}

impl AzureFoundryProvider {
    #[allow(clippy::too_many_arguments)]
    pub fn create(
        endpoint: String,
        api_version: Option<String>,
        maas_model: Option<String>,
        chat_auth: AuthMethod,
        responses_auth: AuthMethod,
        anthropic_auth: AuthMethod,
        deployments_auth: AuthMethod,
        tls_config: Option<TlsConfig>,
        request_builder: Option<RequestBuilderDecorator>,
    ) -> Result<Self> {
        let endpoint = endpoint.trim_end_matches('/').to_string();
        let endpoint_kind = endpoint_kind(&endpoint);
        let native_inference = endpoint_kind != EndpointKind::Maas;
        let maas_model = if native_inference {
            None
        } else {
            Some(
                maas_model
                    .filter(|model| !model.trim().is_empty())
                    .ok_or_else(|| {
                        anyhow::anyhow!("AZURE_FOUNDRY_MODEL is required for MaaS endpoints")
                    })?
                    .trim()
                    .to_string(),
            )
        };
        let chat_prefix = if native_inference {
            "openai/v1/"
        } else {
            "v1/"
        };

        let chat_client = configured_client(
            endpoint.clone(),
            chat_auth,
            tls_config.clone(),
            request_builder.clone(),
        )?;
        let chat = OpenAiCompatibleProvider::new(
            AZURE_FOUNDRY_PROVIDER_NAME.to_string(),
            chat_client,
            chat_prefix.to_string(),
        );

        let (responses, anthropic) = if native_inference {
            let responses_client = configured_client(
                endpoint.clone(),
                responses_auth,
                tls_config.clone(),
                request_builder.clone(),
            )?;
            let responses = OpenAiProviderBuilder::new(responses_client)
                .name(AZURE_FOUNDRY_PROVIDER_NAME)
                .base_path("openai/v1/responses")
                .skip_canonical_filtering(true)
                .build();

            let hub = endpoint.split("/api/projects/").next().unwrap_or(&endpoint);
            let anthropic_client = configured_client(
                format!("{hub}/anthropic"),
                anthropic_auth,
                tls_config.clone(),
                request_builder.clone(),
            )?
            .with_header("anthropic-version", ANTHROPIC_API_VERSION)?;
            let anthropic = AnthropicProviderBuilder::new(anthropic_client)
                .name(AZURE_FOUNDRY_PROVIDER_NAME)
                .skip_canonical_filtering(true)
                .build();
            (Some(responses), Some(anthropic))
        } else {
            (None, None)
        };

        let deployments_client = configured_client(
            endpoint.clone(),
            deployments_auth,
            tls_config,
            request_builder,
        )?;

        Ok(Self {
            chat,
            responses,
            anthropic,
            deployments_client,
            endpoint,
            api_version,
            maas_model,
            deployments: Mutex::new(DeploymentCache::default()),
        })
    }

    async fn fetch_deployments(
        &self,
    ) -> Result<(Vec<String>, HashMap<String, DeploymentMetadata>), ProviderError> {
        let version = self
            .api_version
            .as_deref()
            .or_else(|| is_project_endpoint(&self.endpoint).then_some("v1"));
        let mut next = Some(match version {
            Some(version) => format!("deployments?api-version={version}"),
            None => "deployments".to_string(),
        });
        let mut models = Vec::new();
        let mut deployments = HashMap::new();

        while let Some(path) = next {
            let response = self
                .deployments_client
                .response_get(&path)
                .await
                .map_err(|error| ProviderError::NetworkError(error.to_string()))?;
            let json = handle_response_openai_compat(response).await?;
            if let Some(items) = json.get("value").and_then(|value| value.as_array()) {
                for item in items {
                    let Some(name) = item.get("name").and_then(|value| value.as_str()) else {
                        continue;
                    };
                    let model_name = item
                        .get("modelName")
                        .and_then(|value| value.as_str())
                        .unwrap_or(name);
                    let publisher = item
                        .get("modelPublisher")
                        .and_then(|value| value.as_str())
                        .map(ModelPublisher::from_azure)
                        .unwrap_or_else(|| ModelPublisher::from_model_name(model_name));
                    models.push(name.to_string());
                    deployments.insert(
                        name.to_string(),
                        DeploymentMetadata {
                            publisher,
                            model_name: model_name.to_string(),
                        },
                    );
                }
            }
            next = json
                .get("nextLink")
                .and_then(|value| value.as_str())
                .map(|link| with_api_version(link, version));
        }

        models.sort();
        models.dedup();
        Ok((models, deployments))
    }

    async fn deployment_for(
        &self,
        deployment_name: &str,
        lookup: DeploymentMetadataLookup,
    ) -> Option<DeploymentMetadata> {
        {
            let cache = self
                .deployments
                .lock()
                .expect("Azure Foundry deployment cache poisoned");
            if cache.applies_to(lookup) {
                return cache.deployments.get(deployment_name).cloned();
            }
        }

        let fetch_result = tokio::time::timeout(
            Duration::from_secs(DEPLOYMENT_METADATA_TIMEOUT_SECS),
            self.fetch_deployments(),
        )
        .await
        .ok()
        .and_then(Result::ok);
        let fetch_failed = fetch_result.is_none();
        let deployments = fetch_result
            .map(|(_, deployments)| deployments)
            .unwrap_or_default();
        let deployment = deployments.get(deployment_name).cloned();
        *self
            .deployments
            .lock()
            .expect("Azure Foundry deployment cache poisoned") = DeploymentCache {
            deployments,
            fetched_at: Some(Instant::now()),
            fetch_failed,
        };
        deployment
    }
}

fn with_api_version(link: &str, api_version: Option<&str>) -> String {
    let Some(api_version) = api_version else {
        return link.to_string();
    };
    if link.contains("api-version=") {
        return link.to_string();
    }
    let separator = if link.contains('?') { '&' } else { '?' };
    format!("{link}{separator}api-version={api_version}")
}

fn model_info_for_deployment(deployment_name: &str, model_name: &str) -> ModelInfo {
    let canonical = maybe_get_canonical_model("azure_foundry", model_name)
        .or_else(|| maybe_get_canonical_model("azure_foundry", &model_name.to_ascii_lowercase()))
        .or_else(|| {
            let (base_model, effort) = extract_reasoning_effort(model_name);
            effort.and_then(|_| {
                maybe_get_canonical_model("azure_foundry", &base_model).or_else(|| {
                    maybe_get_canonical_model("azure_foundry", &base_model.to_ascii_lowercase())
                })
            })
        });
    ModelInfo {
        name: deployment_name.to_string(),
        resolved_model: Some(model_name.to_string()),
        context_limit: canonical.as_ref().map(|model| model.limit.context),
        input_token_cost: None,
        output_token_cost: None,
        currency: None,
        supports_cache_control: None,
        reasoning: canonical
            .and_then(|model| model.reasoning)
            .unwrap_or_else(|| ModelConfig::new(model_name).is_reasoning_model()),
        thinking_preservation_format: None,
        request_params: None,
    }
}

fn configured_client(
    host: String,
    auth: AuthMethod,
    tls_config: Option<TlsConfig>,
    request_builder: Option<RequestBuilderDecorator>,
) -> Result<ApiClient> {
    let restrict_redirects = matches!(&auth, AuthMethod::ApiKey { .. });
    let mut client = ApiClient::new_with_tls(host, auth, tls_config)?;
    if restrict_redirects {
        client = client.with_same_origin_redirects()?;
    }
    if let Some(request_builder) = request_builder {
        client = client.with_request_builder(request_builder);
    }
    Ok(client)
}

/// Usage chunks carry the model name the inference surface echoes back — the
/// deployment alias, or a value the endpoint invented — but cost estimation
/// needs the underlying model the alias resolves to, so re-tag each chunk.
fn retag_usage_model(stream: MessageStream, model: &str) -> MessageStream {
    let model = model.to_string();
    Box::pin(stream.map(move |item| {
        item.map(|(message, usage)| {
            let usage = usage.map(|mut usage| {
                usage.model.clone_from(&model);
                usage
            });
            (message, usage)
        })
    }))
}

#[async_trait]
impl Provider for AzureFoundryProvider {
    fn get_name(&self) -> &str {
        AZURE_FOUNDRY_PROVIDER_NAME
    }

    fn skip_canonical_filtering(&self) -> bool {
        true
    }

    async fn refresh_credentials(&self) -> Result<(), ProviderError> {
        self.deployments_client
            .refresh_credentials()
            .await
            .map_err(|error| ProviderError::Authentication(error.to_string()))
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        if let Some(model) = &self.maas_model {
            return Ok(vec![model.clone()]);
        }
        if !is_project_endpoint(&self.endpoint) {
            return Ok(AZURE_FOUNDRY_KNOWN_MODELS
                .iter()
                .map(ToString::to_string)
                .collect());
        }
        let (models, deployments) = self.fetch_deployments().await?;
        *self
            .deployments
            .lock()
            .expect("Azure Foundry deployment cache poisoned") = DeploymentCache {
            deployments,
            fetched_at: Some(Instant::now()),
            fetch_failed: false,
        };
        Ok(models)
    }

    async fn fetch_supported_model_info(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        if let Some(model) = &self.maas_model {
            return Ok(vec![model_info_for_deployment(model, model)]);
        }
        if !is_project_endpoint(&self.endpoint) {
            return Ok(AZURE_FOUNDRY_KNOWN_MODELS
                .iter()
                .map(|model| model_info_for_deployment(model, model))
                .collect());
        }
        let (models, deployments) = self.fetch_deployments().await?;
        let model_info = models
            .iter()
            .filter_map(|name| {
                deployments
                    .get(name)
                    .map(|deployment| model_info_for_deployment(name, &deployment.model_name))
            })
            .collect();
        *self
            .deployments
            .lock()
            .expect("Azure Foundry deployment cache poisoned") = DeploymentCache {
            deployments,
            fetched_at: Some(Instant::now()),
            fetch_failed: false,
        };
        Ok(model_info)
    }

    async fn fetch_model_info(&self, model_name: &str) -> Result<ModelInfo, ProviderError> {
        let resolved_model = if let Some(model) = &self.maas_model {
            model.clone()
        } else if is_project_endpoint(&self.endpoint) {
            self.deployment_for(model_name, DeploymentMetadataLookup::ContextDiscovery)
                .await
                .map(|deployment| deployment.model_name)
                .unwrap_or_else(|| model_name.to_string())
        } else {
            model_name.to_string()
        };
        Ok(model_info_for_deployment(model_name, &resolved_model))
    }

    async fn get_context_limit(&self, model: &str, override_limit: Option<usize>) -> usize {
        ContextLimitResolver::new(self.get_name())
            .resolve(model, override_limit, || async {
                self.fetch_model_info(model)
                    .await
                    .map(|info| info.context_limit)
            })
            .await
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let maas_config = self.maas_model.as_ref().map(|model| {
            let mut config = model_config.clone();
            config.model_name = model.clone();
            config
        });
        let model_config = maas_config.as_ref().unwrap_or(model_config);
        let wire_model = self
            .maas_model
            .clone()
            .unwrap_or_else(|| model_config.model_name.clone());
        let deployment = if is_project_endpoint(&self.endpoint) {
            self.deployment_for(&wire_model, DeploymentMetadataLookup::InferenceRouting)
                .await
        } else {
            None
        };
        let publisher = deployment
            .as_ref()
            .map(|deployment| deployment.publisher)
            .unwrap_or_else(|| ModelPublisher::from_model_name(&model_config.model_name));
        let underlying_model = deployment
            .as_ref()
            .map(|deployment| deployment.model_name.as_str())
            .unwrap_or(&model_config.model_name);
        let route = inference_route(self.responses.is_some(), publisher, underlying_model);
        let capability_model = deployment
            .as_ref()
            .map(|deployment| deployment.model_name.as_str())
            .unwrap_or(&model_config.model_name);
        let mut capability_config = model_config.clone();
        capability_config.model_name = capability_model.to_string();
        let capability_config =
            capability_config.with_canonical_limits(AZURE_FOUNDRY_PROVIDER_NAME);
        let stream = match route {
            InferenceRoute::ProjectResponses => {
                self.responses
                    .as_ref()
                    .expect("checked above")
                    .stream_for_model(
                        &capability_config,
                        &wire_model,
                        capability_model,
                        system,
                        messages,
                        tools,
                    )
                    .await
            }
            InferenceRoute::AnthropicMessages => {
                self.anthropic
                    .as_ref()
                    .expect("checked above")
                    .stream_for_model(&capability_config, &wire_model, system, messages, tools)
                    .await
            }
            InferenceRoute::MaasChatCompletions | InferenceRoute::ProjectChatCompletions => {
                self.chat
                    .stream_for_model(
                        &capability_config,
                        &wire_model,
                        capability_model,
                        system,
                        messages,
                        tools,
                    )
                    .await
            }
        }?;
        Ok(retag_usage_model(stream, underlying_model))
    }
}

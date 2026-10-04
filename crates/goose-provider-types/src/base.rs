use async_trait::async_trait;
use futures::Stream;
use rmcp::model::Tool;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::pin::Pin;
use tokio::sync::watch;

use crate::{
    canonical::{
        CanonicalModelRegistry, catalog::ProviderSetupMetadata, map_to_canonical_model,
        recommended_models_from_registry,
    },
    conversation::{
        message::{Message, MessageContentBlock},
        token_usage::{ProviderUsage, Usage},
    },
    errors::ProviderError,
    goose_mode::GooseMode,
    maybe_send::{MaybeSend, MaybeSync},
    model::ModelConfig,
    permission::PermissionConfirmation,
    retry::RetryConfig,
    thinking::ThinkingEffortSupport,
};

/// Metadata about a provider's configuration requirements and capabilities
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderMetadata {
    /// The unique identifier for this provider
    pub name: String,
    /// Display name for the provider in UIs
    pub display_name: String,
    /// Description of the provider's capabilities
    pub description: String,
    /// The default/recommended model for this provider
    pub default_model: String,
    /// A list of currently known models with their capabilities
    pub known_models: Vec<ModelInfo>,
    /// Link to the docs where models can be found
    pub model_doc_link: String,
    /// Required configuration keys
    pub config_keys: Vec<ConfigKey>,
    /// step-by-step instructions for set up providers eg: api key
    #[serde(default)]
    pub setup_steps: Vec<String>,
    /// Setup information exposed to clients.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup: Option<ProviderSetupMetadata>,
    /// Structured deprecation information for providers kept for compatibility.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<ProviderDeprecation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderDeprecation {
    pub replacement: Option<String>,
}

impl ProviderMetadata {
    pub fn new(
        name: &str,
        display_name: &str,
        description: &str,
        default_model: &str,
        model_names: Vec<&str>,
        model_doc_link: &str,
        config_keys: Vec<ConfigKey>,
    ) -> Self {
        Self {
            name: name.to_string(),
            display_name: display_name.to_string(),
            description: description.to_string(),
            default_model: default_model.to_string(),
            known_models: model_names
                .iter()
                .map(|&model_name| model_info_for_provider_model(name, model_name))
                .collect(),
            model_doc_link: model_doc_link.to_string(),
            config_keys,
            setup_steps: vec![],
            setup: None,
            deprecated: None,
        }
    }

    pub fn with_models(
        name: &str,
        display_name: &str,
        description: &str,
        default_model: &str,
        models: Vec<ModelInfo>,
        model_doc_link: &str,
        config_keys: Vec<ConfigKey>,
    ) -> Self {
        Self {
            name: name.to_string(),
            display_name: display_name.to_string(),
            description: description.to_string(),
            default_model: default_model.to_string(),
            known_models: models,
            model_doc_link: model_doc_link.to_string(),
            config_keys,
            setup_steps: vec![],
            setup: None,
            deprecated: None,
        }
    }

    pub fn empty() -> Self {
        Self {
            name: "".to_string(),
            display_name: "".to_string(),
            description: "".to_string(),
            default_model: "".to_string(),
            known_models: vec![],
            model_doc_link: "".to_string(),
            config_keys: vec![],
            setup_steps: vec![],
            setup: None,
            deprecated: None,
        }
    }

    pub fn with_setup_steps(mut self, steps: Vec<&str>) -> Self {
        self.setup_steps = steps.into_iter().map(|s| s.to_string()).collect();
        self
    }

    pub fn with_setup(mut self, setup: ProviderSetupMetadata) -> Self {
        self.setup = Some(setup);
        self
    }

    pub fn deprecated(mut self, replacement: Option<&str>) -> Self {
        self.deprecated = Some(ProviderDeprecation {
            replacement: replacement.map(str::to_string),
        });
        self
    }
}

/// Configuration key metadata for provider setup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigKey {
    /// The name of the configuration key (e.g., "API_KEY")
    pub name: String,
    /// Whether this key is required for the provider to function
    pub required: bool,
    /// Whether this key should be stored securely (e.g., in keychain)
    pub secret: bool,
    /// Optional default value for the key
    pub default: Option<String>,
    /// Whether this key should be configured using an OAuth flow
    /// When true, the provider's configure_oauth() method will be called instead of prompting for manual input
    pub oauth_flow: bool,
    /// Whether this OAuth flow uses the device code grant (RFC 8628)
    /// When true, the user must enter a verification code in the browser
    #[serde(default)]
    pub device_code_flow: bool,
    /// Whether this key should be shown prominently during provider setup
    /// (onboarding, settings modal, CLI configure)
    #[serde(default)]
    pub primary: bool,
}

impl ConfigKey {
    /// Create a new ConfigKey
    pub fn new(
        name: &str,
        required: bool,
        secret: bool,
        default: Option<&str>,
        primary: bool,
    ) -> Self {
        Self {
            name: name.to_string(),
            required,
            secret,
            default: default.map(|s| s.to_string()),
            oauth_flow: false,
            device_code_flow: false,
            primary,
        }
    }

    /// Create a new ConfigKey that uses an OAuth flow for configuration
    ///
    /// This is used for providers that support OAuth authentication instead of manual API key entry.
    /// When oauth_flow is true, the configuration system will call the provider's configure_oauth() method.
    pub fn new_oauth(
        name: &str,
        required: bool,
        secret: bool,
        default: Option<&str>,
        primary: bool,
    ) -> Self {
        Self {
            name: name.to_string(),
            required,
            secret,
            default: default.map(|s| s.to_string()),
            oauth_flow: true,
            device_code_flow: false,
            primary,
        }
    }

    /// Create a new ConfigKey that uses OAuth device code flow (RFC 8628) for configuration
    ///
    /// Similar to new_oauth, but indicates the provider uses the device code grant where the user
    /// must enter a verification code in the browser.
    pub fn new_oauth_device_code(
        name: &str,
        required: bool,
        secret: bool,
        default: Option<&str>,
        primary: bool,
    ) -> Self {
        Self {
            name: name.to_string(),
            required,
            secret,
            default: default.map(|s| s.to_string()),
            oauth_flow: true,
            device_code_flow: true,
            primary,
        }
    }
}

/// How a model's thinking is replayed back to the provider on subsequent turns.
///
/// Cerebras rejects requests that replay `messages[].reasoning_content`, so such models
/// declare an inline `content` form instead.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingPreservationFormat {
    /// Prepend the thinking to the message content as plain text.
    ContentPrepend,
    /// Prepend the thinking to the message content wrapped in `<think>` tags.
    ContentXml,
    /// Replay in the separate `reasoning_content` field, the OpenAI-compatible default.
    ReasoningContent,
}

/// Information about a model's capabilities
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    /// The name of the model
    pub name: String,
    /// The underlying model resolved from provider metadata, when the configured model is an alias or endpoint.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_model: Option<String>,
    /// The maximum context length this model supports
    pub context_limit: Option<usize>,
    /// Cost per token for input in USD (optional)
    pub input_token_cost: Option<f64>,
    /// Cost per token for output in USD (optional)
    pub output_token_cost: Option<f64>,
    /// Currency for the costs (default: "$")
    pub currency: Option<String>,
    /// Whether this model supports cache control
    pub supports_cache_control: Option<bool>,
    /// Whether this model supports reasoning/thinking controls
    #[serde(default)]
    pub reasoning: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thinking_preservation_format: Option<ThinkingPreservationFormat>,
    /// Static params merged into the request body for this model.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_params: Option<HashMap<String, Value>>,
}

impl ModelInfo {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            resolved_model: None,
            context_limit: None,
            input_token_cost: None,
            output_token_cost: None,
            currency: None,
            supports_cache_control: None,
            reasoning: false,
            thinking_preservation_format: None,
            request_params: None,
        }
    }

    pub fn with_context_limit(mut self, context_limit: usize) -> Self {
        self.context_limit = Some(context_limit);
        self
    }

    pub fn with_optional_context_limit(mut self, context_limit: Option<usize>) -> Self {
        self.context_limit = context_limit;
        self
    }

    /// Create a new ModelInfo with cost information (per token)
    pub fn with_cost(
        name: impl Into<String>,
        context_limit: usize,
        input_cost: f64,
        output_cost: f64,
    ) -> Self {
        Self {
            name: name.into(),
            resolved_model: None,
            context_limit: Some(context_limit),
            input_token_cost: Some(input_cost),
            output_token_cost: Some(output_cost),
            currency: Some("$".to_string()),
            supports_cache_control: None,
            reasoning: false,
            thinking_preservation_format: None,
            request_params: None,
        }
    }
}

pub trait ProviderDescriptor {
    fn metadata() -> ProviderMetadata;
}

/// A message stream yields partial text content but complete tool calls, all within the Message object
/// So a message with text will contain potentially just a word of a longer response, but tool calls
/// messages will only be yielded once concatenated.
#[cfg(not(target_arch = "wasm32"))]
pub type MessageStream = Pin<
    Box<dyn Stream<Item = Result<(Option<Message>, Option<ProviderUsage>), ProviderError>> + Send>,
>;

#[cfg(target_arch = "wasm32")]
pub type MessageStream =
    Pin<Box<dyn Stream<Item = Result<(Option<Message>, Option<ProviderUsage>), ProviderError>>>>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PermissionRouting {
    ActionRequired,
    Noop,
}

pub fn model_info_for_provider_model(provider_name: &str, model_name: &str) -> ModelInfo {
    let registry = CanonicalModelRegistry::bundled().ok();
    let canonical = registry.as_ref().and_then(|registry| {
        let canonical_id = map_to_canonical_model(provider_name, model_name, registry)?;
        let (provider, model) = canonical_id.split_once('/')?;
        registry.get(provider, model)
    });

    let reasoning = canonical
        .as_ref()
        .and_then(|model| model.reasoning)
        .unwrap_or_else(|| ModelConfig::new(model_name).is_reasoning_model());

    ModelInfo {
        name: model_name.to_string(),
        resolved_model: None,
        context_limit: canonical.as_ref().map(|model| model.limit.context),
        input_token_cost: None,
        output_token_cost: None,
        currency: None,
        supports_cache_control: None,
        reasoning,
        thinking_preservation_format: None,
        request_params: None,
    }
}

/// Build `ModelInfo` for discovered model names, preferring metadata declared in
/// provider configuration over the canonical registry.
///
/// Configured entries are authoritative: a statically declared model carries its
/// own `context_limit`/`reasoning` values, which the canonical registry does not
/// know about. Names absent from `configured` fall back to registry metadata.
pub fn merge_configured_model_info(
    provider_name: &str,
    model_names: &[String],
    configured: &[ModelInfo],
) -> Vec<ModelInfo> {
    model_names
        .iter()
        .map(|model_name| {
            configured
                .iter()
                .find(|declared| declared.name == *model_name)
                .cloned()
                .unwrap_or_else(|| model_info_for_provider_model(provider_name, model_name))
        })
        .collect()
}

pub fn known_models_from_registry(provider: &str) -> Vec<ModelInfo> {
    recommended_models_from_registry(provider)
        .into_iter()
        .map(|name| model_info_for_provider_model(provider, &name))
        .collect()
}

/// Collect all chunks from a MessageStream into a single Message and ProviderUsage
pub async fn collect_stream(
    mut stream: MessageStream,
) -> Result<(Message, ProviderUsage), ProviderError> {
    use futures::StreamExt;

    let mut final_message: Option<Message> = None;
    let mut final_usage: Option<ProviderUsage> = None;

    while let Some(result) = stream.next().await {
        let (msg_opt, usage_opt) = result?;

        if let Some(msg) = msg_opt {
            final_message = Some(match final_message {
                Some(mut prev) => {
                    // A multi-block message is a complete unit from the
                    // provider (e.g. a closing Thinking block bundled with the
                    // start of subsequent content), not a raw incremental
                    // delta — mirror Conversation::push's `content.len() == 1`
                    // gate so thinking coalescing only fires for single-block
                    // deltas, never absorbing the first block of a multi-block
                    // chunk into the prior message's last block.
                    let is_single_block_delta = msg.content.len() == 1;
                    for new_content in msg.content {
                        match (&mut prev.content.last_mut(), &new_content) {
                            // Coalesce consecutive text blocks
                            (
                                Some(MessageContentBlock::Text(last_text)),
                                MessageContentBlock::Text(new_text),
                            ) if last_text
                                .annotations
                                .as_ref()
                                .and_then(|a| a.audience.as_ref())
                                == new_text
                                    .annotations
                                    .as_ref()
                                    .and_then(|a| a.audience.as_ref()) =>
                            {
                                last_text.text.push_str(&new_text.text);
                            }
                            // Coalesce consecutive thinking blocks, mirroring
                            // Conversation::push's signature rules: append while
                            // the previous block is unsigned or the incoming
                            // delta shares its signature; a signed block never
                            // absorbs a differently-signed delta.
                            (
                                Some(MessageContentBlock::Thinking(last_thinking)),
                                MessageContentBlock::Thinking(new_thinking),
                            ) if is_single_block_delta
                                && (last_thinking.signature.is_empty()
                                    || new_thinking.signature == last_thinking.signature) =>
                            {
                                last_thinking.thinking.push_str(&new_thinking.thinking);
                                if !new_thinking.signature.is_empty() {
                                    last_thinking.signature = new_thinking.signature.clone();
                                }
                            }
                            _ => {
                                prev.content.push(new_content);
                            }
                        }
                    }
                    prev
                }
                None => msg,
            });
        }

        if let Some(usage) = usage_opt {
            final_usage = Some(usage);
        }
    }

    match (final_message, final_usage) {
        (Some(msg), usage) => {
            let usage = usage
                .unwrap_or_else(|| ProviderUsage::new("unknown".to_string(), Usage::default()));
            Ok((msg, usage))
        }
        (None, Some(usage)) => Ok((
            Message::new(
                rmcp::model::Role::Assistant,
                chrono::Utc::now().timestamp(),
                Vec::new(),
            ),
            usage,
        )),
        (None, None) => Err(ProviderError::ExecutionError(
            "Stream yielded no message".to_string(),
        )),
    }
}

pub fn stream_from_single_message(message: Message, usage: ProviderUsage) -> MessageStream {
    let stream = futures::stream::once(async move { Ok((Some(message), Some(usage))) });
    Box::pin(stream)
}

/// Base trait for AI providers (OpenAI, Anthropic, etc)
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
pub trait Provider: MaybeSend + MaybeSync {
    /// Get the name of this provider instance
    fn get_name(&self) -> &str;

    fn provider_session_id(&self) -> Option<String> {
        None
    }

    async fn resume(&self, _session_id: &str) -> Result<(), ProviderError> {
        Ok(())
    }

    /// Primary streaming method that all providers must implement.
    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError>;

    async fn complete(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<(Message, ProviderUsage), ProviderError> {
        let stream = self.stream(model_config, system, messages, tools).await?;
        collect_stream(stream).await
    }

    /// Resolve the effective context limit for a model.
    ///
    /// `override_limit` is consumer policy and takes precedence over provider
    /// configuration and discovery. The method is infallible because providers
    /// fall through to canonical metadata and the global default.
    async fn get_context_limit(&self, model: &str, override_limit: Option<usize>) -> usize {
        crate::context_limit::ContextLimitResolver::new(self.get_name())
            .resolve(model, override_limit, || async { Ok(None) })
            .await
    }

    fn retry_config(&self) -> RetryConfig {
        RetryConfig::default()
    }

    async fn fetch_supported_models(&self) -> Result<Vec<String>, ProviderError> {
        Ok(vec![])
    }

    async fn fetch_supported_model_info(&self) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(self
            .fetch_supported_models()
            .await?
            .iter()
            .map(|model_name| model_info_for_provider_model(self.get_name(), model_name))
            .collect())
    }

    async fn fetch_model_info(&self, model_name: &str) -> Result<ModelInfo, ProviderError> {
        Ok(model_info_for_provider_model(self.get_name(), model_name))
    }

    fn skip_canonical_filtering(&self) -> bool {
        false
    }

    /// Fetch inventory models filtered by canonical registry and usability.
    ///
    /// When `toolshim` is true, models that lack native tool-call support are
    /// retained because the toolshim layer emulates tool calling.
    async fn fetch_recommended_models(&self, toolshim: bool) -> Result<Vec<String>, ProviderError> {
        let all_models = self.fetch_supported_models().await?;

        if self.skip_canonical_filtering() {
            return Ok(all_models);
        }

        let registry = CanonicalModelRegistry::bundled().map_err(|e| {
            ProviderError::ExecutionError(format!("Failed to load canonical registry: {}", e))
        })?;

        let provider_name = self.get_name();

        // Get all text-capable models with their release dates
        let mut models_with_dates: Vec<(String, Option<String>)> = all_models
            .iter()
            .filter_map(|model| {
                let canonical_model = map_to_canonical_model(provider_name, model, &registry)
                    .and_then(|canonical_id| {
                        let (provider, model_name) = canonical_id.split_once('/')?;
                        registry.get(provider, model_name)
                    });
                let Some(canonical_model) = canonical_model else {
                    return Some((model.clone(), None));
                };

                if !canonical_model
                    .modalities
                    .input
                    .contains(&crate::canonical::Modality::Text)
                {
                    return None;
                }

                if !canonical_model.tool_call && !toolshim {
                    return None;
                }

                let release_date = canonical_model.release_date.clone();

                Some((model.clone(), release_date))
            })
            .collect();

        // Sort by release date (most recent first), then alphabetically for models without dates
        models_with_dates.sort_by(|a, b| match (&a.1, &b.1) {
            (Some(date_a), Some(date_b)) => date_b.cmp(date_a),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.0.cmp(&b.0),
        });

        let inventory_models: Vec<String> = models_with_dates
            .into_iter()
            .map(|(name, _)| name)
            .collect();

        if inventory_models.is_empty() {
            Ok(all_models)
        } else {
            Ok(inventory_models)
        }
    }

    async fn fetch_recommended_model_info(
        &self,
        toolshim: bool,
    ) -> Result<Vec<ModelInfo>, ProviderError> {
        Ok(self
            .fetch_recommended_models(toolshim)
            .await?
            .iter()
            .map(|model_name| model_info_for_provider_model(self.get_name(), model_name))
            .collect())
    }

    async fn map_to_canonical_model(
        &self,
        provider_model: &str,
    ) -> Result<Option<String>, ProviderError> {
        let registry = CanonicalModelRegistry::bundled().map_err(|e| {
            ProviderError::ExecutionError(format!("Failed to load canonical registry: {}", e))
        })?;

        Ok(map_to_canonical_model(
            self.get_name(),
            provider_model,
            &registry,
        ))
    }

    /// Whether the provider manages its own conversation context (e.g. CLI
    /// wrappers like Claude Code or Gemini CLI). When true, goose-side
    /// context management such as tool-pair summarization is skipped because
    /// the provider's internal state is the source of truth.
    fn manages_own_context(&self) -> bool {
        false
    }

    fn uses_local_session_naming(&self) -> bool {
        self.manages_own_context()
    }

    fn supports_builtin_tools(&self) -> bool {
        !self.manages_own_context()
    }

    /// Configure OAuth authentication for this provider
    ///
    /// This method is called when a provider has configuration keys marked with oauth_flow = true.
    /// Providers that support OAuth should override this method to implement their specific OAuth flow.
    ///
    /// # Returns
    /// * `Ok(())` if OAuth configuration succeeds and credentials are saved
    /// * `Err(ProviderError)` if OAuth fails or is not supported by this provider
    ///
    /// # Default Implementation
    /// The default implementation returns an error indicating OAuth is not supported.
    async fn configure_oauth(&self) -> Result<(), ProviderError> {
        Err(ProviderError::ExecutionError(
            "OAuth configuration not supported by this provider".to_string(),
        ))
    }

    async fn refresh_credentials(&self) -> Result<(), ProviderError> {
        Err(ProviderError::NotImplemented(
            "credential refresh not supported by this provider".to_string(),
        ))
    }

    async fn update_mode(&self, _session_id: &str, _mode: GooseMode) -> Result<(), ProviderError> {
        Ok(())
    }

    /// How this provider participates in thinking-effort selection. Providers
    /// that manage reasoning through an external harness report the harness's
    /// advertised capability; the default keeps the model-name-based path.
    fn thinking_effort_support(&self) -> ThinkingEffortSupport {
        ThinkingEffortSupport::Unspecified
    }

    /// Subscribe to provider-managed thinking-effort capability changes.
    /// Providers without an asynchronous capability source return `None`.
    fn subscribe_thinking_effort_support(&self) -> Option<watch::Receiver<ThinkingEffortSupport>> {
        None
    }

    /// Forward a thinking-effort selection to the provider. Returns `Ok(true)`
    /// when the provider applied the value itself (no provider recreation
    /// needed); `Ok(false)` when the caller should use the legacy path.
    async fn set_thinking_effort(
        &self,
        _session_id: &str,
        _value: &str,
    ) -> Result<bool, ProviderError> {
        Ok(false)
    }

    /// Apply a session's model selection after the provider is installed.
    /// Providers that manage their own model (e.g. ACP harnesses) override
    /// this to sync the selection before the first prompt.
    async fn apply_model_selection(
        &self,
        _model_config: &ModelConfig,
    ) -> Result<(), ProviderError> {
        Ok(())
    }

    fn permission_routing(&self) -> PermissionRouting {
        PermissionRouting::Noop
    }

    async fn handle_permission_confirmation(
        &self,
        _request_id: &str,
        _confirmation: &PermissionConfirmation,
    ) -> bool {
        false
    }
}

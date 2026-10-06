use crate::canonical::CanonicalModel;
use crate::formats::{
    extract_reasoning_effort, is_openai_responses_model, is_xai_reasoning_model,
    supports_xai_reasoning_effort,
};
use crate::thinking::ThinkingEffort;
use crate::{Modality, maybe_get_canonical_model};
use serde::de::Deserializer;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
pub const DEFAULT_CONTEXT_LIMIT: usize = 128_000;

/// Request param keys that describe model-family-agnostic reasoning behavior and
/// are therefore safe to carry across a model switch or subagent delegation.
/// Provider-specific keys (e.g. `anthropic_beta`) are deliberately excluded so
/// they can't bleed into a request targeting a different model family.
const INHERITED_SESSION_PARAM_KEYS: &[&str] = &[
    "thinking_effort",
    "thinking_budget",
    "budget_tokens",
    "enable_thinking",
    "preserve_thinking_context",
    "preserve_unsigned_thinking",
];

/// Request params goose consumes itself: formats that forward unknown params into
/// the payload must skip these, or the provider gets an unrecognized wire parameter.
pub fn is_goose_internal_request_param(key: &str) -> bool {
    matches!(
        key,
        "thinking_effort"
            | "disable_prompt_cache"
            | "cache_ttl"
            | "emit_clear_thinking"
            | "preserve_thinking_context"
            | "preserve_unsigned_thinking"
    )
}

#[derive(Debug, Clone, Serialize)]
pub struct ModelConfig {
    pub model_name: String,
    #[serde(skip)]
    pub context_limit: Option<usize>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i32>,
    pub toolshim: bool,
    pub toolshim_model: Option<String>,
    /// Provider-specific request parameters (e.g., anthropic_beta headers)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_params: Option<HashMap<String, Value>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_vision: Option<bool>,
    /// Per-request HTTP headers attached to outgoing provider calls.
    /// Never serialized into request bodies.
    #[serde(skip)]
    pub request_headers: Option<HashMap<String, String>>,
}

impl<'de> Deserialize<'de> for ModelConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct RawModelConfig {
            model_name: String,
            #[serde(rename = "context_limit")]
            _context_limit: Option<usize>,
            temperature: Option<f32>,
            max_tokens: Option<i32>,
            toolshim: bool,
            toolshim_model: Option<String>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            request_params: Option<HashMap<String, Value>>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            reasoning: Option<bool>,
            #[serde(default, skip_serializing_if = "Option::is_none")]
            supports_vision: Option<bool>,
        }

        let raw = RawModelConfig::deserialize(deserializer)?;
        let mut config = Self {
            model_name: raw.model_name,
            context_limit: None,
            temperature: raw.temperature,
            max_tokens: raw.max_tokens,
            toolshim: raw.toolshim,
            toolshim_model: raw.toolshim_model,
            request_params: raw.request_params,
            reasoning: raw.reasoning,
            supports_vision: raw.supports_vision,
            request_headers: None,
        };
        config.normalize_effort_suffix();
        Ok(config)
    }
}

impl ModelConfig {
    pub fn new(model_name: impl AsRef<str>) -> Self {
        let mut config = Self {
            model_name: model_name.as_ref().to_string(),
            context_limit: None,
            temperature: None,
            max_tokens: None,
            toolshim: false,
            toolshim_model: None,
            request_params: None,
            reasoning: None,
            supports_vision: None,
            request_headers: None,
        };
        config.normalize_effort_suffix();
        config
    }

    fn canonical_model(&self, provider_name: &str) -> Option<CanonicalModel> {
        // Try canonical lookup with the full model name first, then fall back
        // to the name with reasoning-effort suffixes stripped (e.g.
        // "databricks-gpt-5.4-high" → "databricks-gpt-5.4").
        maybe_get_canonical_model(provider_name, &self.model_name).or_else(|| {
            let (base, _effort) = extract_reasoning_effort(&self.model_name);
            if base != self.model_name {
                maybe_get_canonical_model(provider_name, &base)
            } else {
                None
            }
        })
    }

    pub fn with_canonical_vision_support(mut self, provider_name: &str) -> Self {
        if self.supports_vision.is_none()
            && let Some(canonical) = self.canonical_model(provider_name)
        {
            self.supports_vision = Some(canonical.modalities.input.contains(&Modality::Image));
        }
        self
    }

    pub fn with_canonical_limits(mut self, provider_name: &str) -> Self {
        let canonical = self.canonical_model(provider_name);

        if let Some(canonical) = canonical {
            if self.max_tokens.is_none() {
                self.max_tokens = canonical
                    .limit
                    .output
                    .filter(|&output| output < canonical.limit.context)
                    .map(|output| output as i32);
            }
            if self.reasoning.is_none() {
                self.reasoning = canonical.reasoning;
            }
            if self.supports_vision.is_none() {
                self.supports_vision = Some(canonical.modalities.input.contains(&Modality::Image))
            }
        }

        self
    }

    pub fn with_context_limit(mut self, limit: Option<usize>) -> Self {
        if limit.is_some() {
            self.context_limit = limit;
        }
        self
    }

    pub fn with_temperature(mut self, temp: Option<f32>) -> Self {
        self.temperature = temp;
        self
    }

    pub fn with_max_tokens(mut self, tokens: Option<i32>) -> Self {
        self.max_tokens = tokens;
        self
    }

    pub fn with_default_context_limit(mut self, limit: Option<usize>) -> Self {
        if self.context_limit.is_none() {
            self.context_limit = limit;
        }
        self
    }

    pub fn with_default_max_tokens(mut self, tokens: Option<i32>) -> Self {
        if self.max_tokens.is_none() {
            self.max_tokens = tokens;
        }
        self
    }

    pub fn with_toolshim(mut self, toolshim: bool) -> Self {
        self.toolshim = toolshim;
        self
    }

    pub fn with_toolshim_model(mut self, model: Option<String>) -> Self {
        self.toolshim_model = model;
        self
    }

    pub fn with_request_headers(mut self, headers: Option<HashMap<String, String>>) -> Self {
        self.request_headers = headers;
        self
    }

    pub fn with_merged_request_params(mut self, params: HashMap<String, Value>) -> Self {
        match self.request_params.as_mut() {
            Some(existing) => {
                for (k, v) in params {
                    existing.insert(k, v);
                }
            }
            None => {
                self.request_params = Some(params);
            }
        }
        self
    }

    pub fn with_thinking_effort(mut self, effort: ThinkingEffort) -> Self {
        let params = self.request_params.get_or_insert_with(HashMap::new);
        params.insert(
            "thinking_effort".to_string(),
            serde_json::json!(effort.to_string()),
        );
        self
    }

    pub fn with_default_thinking_effort(mut self, effort: Option<ThinkingEffort>) -> Self {
        // Guard on raw-param presence rather than parseability: a persisted
        // harness value like "default" doesn't parse into ThinkingEffort but
        // is still an explicit user pick that must not be overwritten.
        if self.request_param::<String>("thinking_effort").is_none()
            && let Some(effort) = effort
        {
            self = self.with_thinking_effort(effort);
        }
        self
    }

    pub fn with_vision_support(mut self, supports_vision: bool) -> Self {
        self.supports_vision = Some(supports_vision);
        self
    }

    pub fn with_inherited_session_settings_from(
        mut self,
        previous: Option<&ModelConfig>,
        request_params: Option<HashMap<String, Value>>,
    ) -> Self {
        if let Some(previous_params) = previous.and_then(|p| p.request_params.as_ref()) {
            for key in INHERITED_SESSION_PARAM_KEYS {
                if let Some(value) = previous_params.get(*key) {
                    self.request_params
                        .get_or_insert_with(HashMap::new)
                        .entry(key.to_string())
                        .or_insert_with(|| value.clone());
                }
            }
        }

        if let Some(request_params) = request_params {
            self = self.with_merged_request_params(request_params);
        }

        self
    }

    pub fn context_limit(&self) -> usize {
        self.context_limit.unwrap_or(DEFAULT_CONTEXT_LIMIT)
    }

    pub fn is_openai_reasoning_model(&self) -> bool {
        is_openai_responses_model(&self.model_name)
    }

    pub fn is_reasoning_model(&self) -> bool {
        if let Some(reasoning) = self.reasoning {
            return reasoning;
        }

        self.is_openai_reasoning_model()
            || self.model_name.to_lowercase().contains("claude")
            || Self::is_gemini3_reasoning_model_name(&self.model_name)
            || self.is_glm_5_3_reasoning_model()
            || self.is_kimi_k3_reasoning_model()
            || is_xai_reasoning_model(&self.model_name)
    }

    pub fn is_glm_5_3_reasoning_model(&self) -> bool {
        let name = self
            .model_name
            .splitn(3, '.')
            .nth(2)
            .unwrap_or(&self.model_name);
        let lower = name.to_lowercase();
        let segments: Vec<_> = lower
            .split(|character: char| !character.is_ascii_alphanumeric())
            .filter(|segment| !segment.is_empty())
            .collect();
        segments
            .windows(3)
            .any(|segments| segments == ["glm", "5", "3"])
    }

    pub fn is_kimi_k3_reasoning_model(&self) -> bool {
        let name = self
            .model_name
            .splitn(3, '.')
            .nth(2)
            .unwrap_or(&self.model_name);
        let lower = name.to_lowercase();
        let segments: Vec<_> = lower
            .split(|character: char| !character.is_ascii_alphanumeric())
            .filter(|segment| !segment.is_empty())
            .collect();
        segments
            .windows(2)
            .any(|segments| segments == ["kimi", "k3"])
    }

    fn is_gemini3_reasoning_model_name(model_name: &str) -> bool {
        let lower = model_name.to_lowercase();
        lower.starts_with("gemini-3") || lower.contains("/gemini-3") || lower.contains("-gemini-3")
    }

    pub fn max_output_tokens(&self) -> i32 {
        if let Some(tokens) = self.max_tokens {
            return tokens;
        }

        4_096
    }

    pub fn normalize_effort_suffix(&mut self) {
        if !self.is_openai_reasoning_model() && !supports_xai_reasoning_effort(&self.model_name) {
            return;
        }
        let parts: Vec<&str> = self.model_name.split('-').collect();
        let last = match parts.last() {
            Some(l) => *l,
            None => return,
        };
        let effort = match last {
            "none" => ThinkingEffort::Off,
            "low" => ThinkingEffort::Low,
            "medium" => ThinkingEffort::Medium,
            "high" => ThinkingEffort::High,
            "xhigh" => ThinkingEffort::Max,
            _ => return,
        };
        self.model_name = parts[..parts.len() - 1].join("-");
        let has_explicit_effort = self
            .request_params
            .as_ref()
            .and_then(|p| p.get("thinking_effort"))
            .is_some();
        if !has_explicit_effort {
            let params = self.request_params.get_or_insert_with(HashMap::new);
            params.insert(
                "thinking_effort".to_string(),
                serde_json::json!(effort.to_string()),
            );
        }
    }

    pub fn thinking_effort(&self) -> Option<ThinkingEffort> {
        self.request_param::<String>("thinking_effort")
            .and_then(|s| s.parse::<ThinkingEffort>().ok())
    }

    pub fn with_prompt_cache_disabled(self) -> Self {
        self.with_merged_request_params(HashMap::from([(
            "disable_prompt_cache".to_string(),
            Value::Bool(true),
        )]))
    }

    pub fn prompt_cache_disabled(&self) -> bool {
        self.request_param::<bool>("disable_prompt_cache")
            .unwrap_or(false)
    }

    /// Set the prompt-cache TTL requested from providers that support one
    /// (currently the Anthropic message format). Valid values are "5m" and
    /// "1h"; absent means the provider default (5m).
    pub fn with_cache_ttl(self, ttl: &str) -> Self {
        self.with_merged_request_params(HashMap::from([(
            "cache_ttl".to_string(),
            Value::String(ttl.to_string()),
        )]))
    }

    /// Remove any prompt-cache TTL request parameter. The TTL is
    /// configuration state, not session state: callers that resume a
    /// persisted config drop the stored value and re-derive it from the
    /// current configuration so a clamped run never sticks to the session.
    pub fn without_cache_ttl(mut self) -> Self {
        if let Some(params) = self.request_params.as_mut() {
            params.remove("cache_ttl");
            if params.is_empty() {
                self.request_params = None;
            }
        }
        self
    }

    /// Clamp the prompt-cache TTL back to the provider default (5m).
    /// Burst-only surfaces (headless runs, subagents, scheduled recipes) call
    /// this so a user-level 1h opt-in never pays the 2x cache-write premium on
    /// workloads that finish in one burst and cannot idle.
    pub fn with_cache_ttl_clamped(self) -> Self {
        if self.cache_ttl().is_some_and(|ttl| ttl != "5m") {
            self.with_cache_ttl("5m")
        } else {
            self
        }
    }

    pub fn cache_ttl(&self) -> Option<String> {
        self.request_param::<String>("cache_ttl")
    }

    pub fn request_param<T: for<'de> serde::Deserialize<'de>>(
        &self,
        request_key: &str,
    ) -> Option<T> {
        self.request_params
            .as_ref()
            .and_then(|params| params.get(request_key))
            .and_then(|v| serde_json::from_value(v.clone()).ok())
    }
}

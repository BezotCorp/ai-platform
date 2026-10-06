use crate::providers::base::ProviderType;
use crate::providers::{
    huggingface::HuggingFaceProvider, huggingface_auth, inventory::declarative_inventory_identity,
    ollama_def::OllamaProviderDef, openai_def::OpenAiProviderDef, private_file::write_private_file,
};
use crate::{
    config::{Config, paths::Paths},
    providers::anthropic_def::AnthropicProviderDef,
};
use anyhow::Result;
use bcaip_provider_types::base::ModelInfo;
use goose_providers::declarative::AuthConfig;
use goose_providers::declarative::DeclarativeProviderConfig;
use goose_providers::declarative::EnvVarConfig;
use goose_providers::declarative::ProviderEngine;
use goose_providers::declarative::deserialize_provider_config;
use goose_providers::declarative::fixed_provider_configs;
use goose_providers::declarative::load_custom_providers;
use goose_providers::declarative::should_preserve_thinking_by_default;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, str::FromStr, sync::Mutex};

pub fn custom_providers_dir() -> std::path::PathBuf {
    Paths::config_dir().join("custom_providers")
}

/// Expand `${VAR_NAME}` placeholders in a template string using the given env var configs.
/// Resolves values via Config (secret if `secret`, param otherwise), falls back to `default`.
/// Returns an error if a `required` var is missing.
pub fn expand_env_vars(template: &str, env_vars: &[EnvVarConfig]) -> Result<String> {
    let config = Config::global();
    let mut result = template.to_string();
    for var in env_vars {
        let placeholder = format!("${{{}}}", var.name);
        if !result.contains(&placeholder) {
            continue;
        }
        let value = if var.secret {
            config.get_secret::<String>(&var.name).ok()
        } else {
            config.get_param::<String>(&var.name).ok()
        };
        let value = match value {
            Some(v) => v,
            None => match &var.default {
                Some(d) => d.clone(),
                None if var.required => {
                    return Err(anyhow::anyhow!(
                        "Required environment variable {} is not set",
                        var.name
                    ));
                }
                None => continue,
            },
        };
        result = result.replace(&placeholder, &value);
    }
    Ok(result)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadedProvider {
    pub config: DeclarativeProviderConfig,
    pub is_editable: bool,
}

static ID_GENERATION_LOCK: Lazy<Mutex<()>> = Lazy::new(|| Mutex::new(()));

pub fn generate_id(display_name: &str) -> String {
    let _guard = ID_GENERATION_LOCK.lock().unwrap();

    let normalized = display_name
        .to_lowercase()
        .chars()
        .map(|ch| {
            if ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string();
    let base_id = format!("custom_{}", normalized);

    let custom_dir = custom_providers_dir();
    let mut candidate_id = base_id.clone();
    let mut counter = 1;

    while custom_dir.join(format!("{}.json", candidate_id)).exists() {
        candidate_id = format!("{}_{}", base_id, counter);
        counter += 1;
    }

    candidate_id
}

pub fn validate_provider_id(id: &str) -> Result<()> {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return Err(anyhow::anyhow!(
            "Invalid provider id: provider id cannot be empty"
        ));
    };

    if !(first.is_ascii_lowercase() || first.is_ascii_digit() || first == '_') {
        return Err(anyhow::anyhow!("Invalid provider id: {}", id));
    }

    if chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '_' || ch == '-') {
        Ok(())
    } else {
        Err(anyhow::anyhow!("Invalid provider id: {}", id))
    }
}

pub(crate) fn custom_provider_file_path(id: &str) -> Result<PathBuf> {
    if id.is_empty()
        || id
            .chars()
            .any(|ch| ch == '/' || ch == '\\' || ch.is_control())
    {
        return Err(anyhow::anyhow!(
            "Invalid provider id: {}",
            if id.is_empty() { "<empty>" } else { id }
        ));
    }

    Ok(custom_providers_dir().join(format!("{}.json", id)))
}

fn persist_custom_provider(provider: &DeclarativeProviderConfig) -> Result<()> {
    let json_content = serde_json::to_string_pretty(provider)?;
    let file_path = custom_provider_file_path(&provider.name)?;
    write_private_file(&file_path, &json_content)?;
    Ok(())
}

pub fn generate_api_key_name(id: &str) -> String {
    format!("{}_API_KEY", id.to_uppercase())
}

#[derive(Debug, Clone)]
pub struct CreateCustomProviderParams {
    pub engine: String,
    pub display_name: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub models: Vec<ModelInfo>,
    pub supports_streaming: Option<bool>,
    pub headers: Option<HashMap<String, String>>,
    pub requires_auth: bool,
    pub catalog_provider_id: Option<String>,
    pub base_path: Option<String>,
    pub toolshim: bool,
    pub preserves_thinking: Option<bool>,
    /// Alternative to `api_key`; mutually exclusive with it.
    pub auth: Option<AuthConfig>,
}

#[derive(Debug, Clone)]
pub struct UpdateCustomProviderParams {
    pub id: String,
    pub engine: String,
    pub display_name: String,
    pub api_url: String,
    pub api_key: Option<String>,
    pub models: Vec<ModelInfo>,
    pub supports_streaming: Option<bool>,
    pub headers: Option<HashMap<String, String>>,
    pub requires_auth: bool,
    pub catalog_provider_id: Option<String>,
    pub base_path: Option<String>,
    pub toolshim: bool,
    pub preserves_thinking: Option<bool>,
    /// Alternative to `api_key`; mutually exclusive with it.
    pub auth: Option<AuthConfig>,
}

pub fn create_custom_provider(
    params: CreateCustomProviderParams,
) -> Result<DeclarativeProviderConfig> {
    let id = generate_id(&params.display_name);
    validate_provider_id(&id)?;

    if params.auth.is_some()
        && params
            .api_key
            .as_deref()
            .is_some_and(|key| !key.trim().is_empty())
    {
        anyhow::bail!("cannot set both apiKey and auth.command");
    }

    let api_key_env = if params.auth.is_some() {
        String::new()
    } else if params.requires_auth {
        let api_key = params
            .api_key
            .as_deref()
            .filter(|api_key| !api_key.trim().is_empty())
            .ok_or_else(|| anyhow::anyhow!("apiKey cannot be empty"))?;
        let api_key_name = generate_api_key_name(&id);
        let config = Config::global();
        config.set_secret(&api_key_name, &api_key)?;
        api_key_name
    } else {
        String::new()
    };

    let model_infos = params.models;

    let engine = ProviderEngine::from_str(&params.engine)?;
    let preserves_thinking = params
        .preserves_thinking
        .unwrap_or_else(|| should_preserve_thinking_by_default(&engine));

    let provider_config = DeclarativeProviderConfig {
        name: id.clone(),
        engine,
        display_name: params.display_name.clone(),
        description: Some(format!("Custom {} provider", params.display_name)),
        api_key_env,
        base_url: params.api_url,
        models: model_infos,
        headers: params.headers,
        session_id_header_override: None,
        timeout_seconds: None,
        supports_streaming: params.supports_streaming,
        requires_auth: params.requires_auth,
        catalog_provider_id: params.catalog_provider_id,
        base_path: params.base_path,
        env_vars: None,
        auth: params.auth,
        dynamic_models: None,
        skip_canonical_filtering: false,
        model_doc_link: None,
        setup_steps: vec![],
        toolshim: params.toolshim,
        preserves_thinking,
        emit_clear_thinking: false,
        setup: None,
    };

    persist_custom_provider(&provider_config)?;

    Ok(provider_config)
}

pub fn update_custom_provider(params: UpdateCustomProviderParams) -> Result<()> {
    let loaded_provider = load_provider(&params.id)?;
    let existing_config = loaded_provider.config;
    let editable = loaded_provider.is_editable;

    if params.auth.is_some()
        && params
            .api_key
            .as_deref()
            .is_some_and(|key| !key.trim().is_empty())
    {
        anyhow::bail!("cannot set both apiKey and auth.command");
    }

    let config = Config::global();
    let api_key_env = if params.auth.is_some() {
        if existing_config.api_key_env == generate_api_key_name(&params.id) {
            config.delete_secret(&existing_config.api_key_env)?;
        }
        String::new()
    } else if params.requires_auth {
        let api_key_name = if existing_config.api_key_env.is_empty() {
            generate_api_key_name(&params.id)
        } else {
            existing_config.api_key_env.clone()
        };
        if let Some(api_key) = params.api_key.as_deref() {
            config.set_secret(&api_key_name, &api_key)?;
        } else if config.get_secret::<String>(&api_key_name).is_err() {
            return Err(anyhow::anyhow!(
                "apiKey is required when auth is enabled and no secret is stored"
            ));
        }
        api_key_name
    } else {
        if existing_config.api_key_env == generate_api_key_name(&params.id) {
            config.delete_secret(&existing_config.api_key_env)?;
        }
        String::new()
    };

    if editable {
        let model_infos = params
            .models
            .into_iter()
            .map(|mut model| {
                if let Some(existing) = existing_config
                    .models
                    .iter()
                    .find(|existing| existing.name == model.name)
                {
                    model.resolved_model = model.resolved_model.or(existing.resolved_model.clone());
                    model.context_limit = model.context_limit.or(existing.context_limit);
                    model.input_token_cost = model.input_token_cost.or(existing.input_token_cost);
                    model.output_token_cost =
                        model.output_token_cost.or(existing.output_token_cost);
                    model.currency = model.currency.or(existing.currency.clone());
                    model.supports_cache_control = model
                        .supports_cache_control
                        .or(existing.supports_cache_control);
                    model.reasoning |= existing.reasoning;
                    model.thinking_preservation_format = model
                        .thinking_preservation_format
                        .or(existing.thinking_preservation_format);
                    model.request_params = model.request_params.or(existing.request_params.clone());
                }
                model
            })
            .collect();

        let engine = ProviderEngine::from_str(&params.engine)?;
        let preserves_thinking = match params.preserves_thinking {
            Some(value) => value,
            None if existing_config.engine != engine => {
                should_preserve_thinking_by_default(&engine)
            }
            None => existing_config.preserves_thinking,
        };

        let updated_config = DeclarativeProviderConfig {
            name: params.id.clone(),
            engine,
            display_name: params.display_name,
            description: existing_config.description,
            api_key_env,
            base_url: params.api_url,
            models: model_infos,
            headers: match params.headers {
                Some(h) if h.is_empty() => None,
                Some(h) => Some(h),
                None => existing_config.headers,
            },
            session_id_header_override: existing_config.session_id_header_override,
            timeout_seconds: existing_config.timeout_seconds,
            supports_streaming: params.supports_streaming,
            requires_auth: params.requires_auth,
            catalog_provider_id: params.catalog_provider_id,
            base_path: params.base_path,
            env_vars: existing_config.env_vars,
            auth: params.auth,
            dynamic_models: existing_config.dynamic_models,
            skip_canonical_filtering: existing_config.skip_canonical_filtering,
            model_doc_link: existing_config.model_doc_link,
            setup_steps: existing_config.setup_steps,
            toolshim: params.toolshim,
            preserves_thinking,
            emit_clear_thinking: existing_config.emit_clear_thinking,
            setup: existing_config.setup,
        };

        persist_custom_provider(&updated_config)?;
    }
    Ok(())
}

pub fn remove_custom_provider(id: &str) -> Result<()> {
    let config = Config::global();
    let loaded_provider = load_provider(id)?;
    let api_key_env = loaded_provider.config.api_key_env;
    if api_key_env == generate_api_key_name(id) {
        let _ = config.delete_secret(&api_key_env);
    }

    let file_path = custom_provider_file_path(id)?;

    if file_path.exists() {
        std::fs::remove_file(file_path)?;
    }

    Ok(())
}

pub fn load_provider(id: &str) -> Result<LoadedProvider> {
    let custom_file_path = custom_provider_file_path(id)?;

    if custom_file_path.exists() {
        let content = std::fs::read_to_string(&custom_file_path)?;
        let config = deserialize_provider_config(&content)?;
        return Ok(LoadedProvider {
            config,
            is_editable: true,
        });
    }

    if let Some(config) = fixed_provider_configs()?
        .into_iter()
        .find(|config| config.name == id)
    {
        return Ok(LoadedProvider {
            config,
            is_editable: false,
        });
    }

    Err(anyhow::anyhow!("Provider not found: {}", id))
}

pub fn register_declarative_providers(
    registry: &mut crate::providers::provider_registry::ProviderRegistry,
) -> Result<()> {
    let dir = custom_providers_dir();
    let custom_providers = load_custom_providers(&dir)?;
    let fixed_providers = fixed_provider_configs()?;
    for config in fixed_providers {
        register_declarative_provider(registry, config, ProviderType::Declarative);
    }

    for config in custom_providers {
        register_declarative_provider(registry, config, ProviderType::Custom);
    }

    Ok(())
}

/// Resolve `${VAR}` placeholders in the config's `base_url` and apply
/// runtime overrides from env_vars. Called lazily (at provider instantiation)
/// so values configured through the UI after startup are picked up.
fn resolve_config(config: &mut DeclarativeProviderConfig) -> Result<()> {
    if let Some(ref env_vars) = config.env_vars {
        config.base_url = expand_env_vars(&config.base_url, env_vars)?;

        // Check for streaming override via env_vars.
        // Config/env may store the value as a string ("true") or a native bool,
        // so try String first, then fall back to bool.
        let global_config = Config::global();
        for var in env_vars {
            if var.name.ends_with("_STREAMING") {
                let val: Option<bool> = global_config
                    .get_param::<String>(&var.name)
                    .ok()
                    .map(|s| s.to_lowercase() == "true")
                    .or_else(|| global_config.get_param::<bool>(&var.name).ok())
                    .or_else(|| var.default.as_deref().map(|d| d.to_lowercase() == "true"));
                if let Some(v) = val {
                    config.supports_streaming = Some(v);
                }
            }
        }
    }
    Ok(())
}

pub fn register_declarative_provider(
    registry: &mut crate::providers::provider_registry::ProviderRegistry,
    config: DeclarativeProviderConfig,
    provider_type: ProviderType,
) {
    // Each closure needs its own owned copy of config because closures are
    // moved into the registry and may be invoked much later than registration.
    // Env var expansion happens lazily inside resolve_base_url so that values
    // configured through the UI after startup are picked up.
    match config.engine {
        ProviderEngine::OpenAI => {
            let captured = config.clone();
            let identity_config = config.clone();
            if HuggingFaceProvider::matches_declarative_config(&config) {
                let inventory_configured_config = config.clone();
                registry
                    .register_with_name_and_inventory_configured::<HuggingFaceProvider, _, _, _>(
                        &config,
                        provider_type,
                        config.dynamic_models.unwrap_or(false),
                        move |tls_config| {
                            let mut cfg = captured.clone();
                            resolve_config(&mut cfg)?;
                            HuggingFaceProvider::from_custom_config(cfg, tls_config)
                        },
                        move || {
                            let mut cfg = identity_config.clone();
                            resolve_config(&mut cfg)?;
                            declarative_inventory_identity(&cfg)
                        },
                        move || {
                            let mut cfg = inventory_configured_config.clone();
                            if resolve_config(&mut cfg).is_err() {
                                return false;
                            }
                            huggingface_declarative_inventory_configured(&cfg)
                        },
                    );
            } else if crate::providers::ollama_cloud::OllamaCloudProvider::matches_declarative_config(&config) {
                registry.register_with_name::<crate::providers::ollama_cloud::OllamaCloudProvider, _, _>(
                    &config,
                    provider_type,
                    config.dynamic_models.unwrap_or(false),
                    move |tls_config| {
                        let mut cfg = captured.clone();
                        resolve_config(&mut cfg)?;
                        crate::providers::ollama_cloud::OllamaCloudProvider::from_custom_config(cfg, tls_config)
                    },
                    move || {
                        let mut cfg = identity_config.clone();
                        resolve_config(&mut cfg)?;
                        declarative_inventory_identity(&cfg)
                    },
                );
            } else {
                registry.register_with_name::<OpenAiProviderDef, _, _>(
                    &config,
                    provider_type,
                    config.dynamic_models.unwrap_or(false),
                    move |tls_config| {
                        let mut cfg = captured.clone();
                        resolve_config(&mut cfg)?;
                        crate::providers::openai_def::from_custom_config(cfg, tls_config)
                    },
                    move || {
                        let mut cfg = identity_config.clone();
                        resolve_config(&mut cfg)?;
                        declarative_inventory_identity(&cfg)
                    },
                );
            }
        }
        ProviderEngine::Ollama => {
            let captured = config.clone();
            let identity_config = config.clone();
            registry.register_with_name::<OllamaProviderDef, _, _>(
                &config,
                provider_type,
                config.dynamic_models.unwrap_or(false),
                move |tls_config| {
                    let mut cfg = captured.clone();
                    resolve_config(&mut cfg)?;
                    crate::providers::ollama_def::from_custom_config(cfg, tls_config)
                },
                move || {
                    let mut cfg = identity_config.clone();
                    resolve_config(&mut cfg)?;
                    declarative_inventory_identity(&cfg)
                },
            );
        }
        ProviderEngine::Anthropic => {
            let captured = config.clone();
            let identity_config = config.clone();
            registry.register_with_name::<AnthropicProviderDef, _, _>(
                &config,
                provider_type,
                config.dynamic_models.unwrap_or(false),
                move |tls_config| {
                    let mut cfg = captured.clone();
                    resolve_config(&mut cfg)?;
                    crate::providers::anthropic_def::from_custom_config(cfg, tls_config)
                },
                move || {
                    let mut cfg = identity_config.clone();
                    resolve_config(&mut cfg)?;
                    declarative_inventory_identity(&cfg)
                },
            );
        }
    }
}

fn huggingface_declarative_inventory_configured(config: &DeclarativeProviderConfig) -> bool {
    huggingface_declarative_inventory_configured_from_sources(
        config,
        |key| Config::global().get_secret::<String>(key).is_ok(),
        || huggingface_auth::has_configured_token().unwrap_or(false),
    )
}

fn huggingface_declarative_inventory_configured_from_sources(
    config: &DeclarativeProviderConfig,
    provider_secret_configured: impl FnOnce(&str) -> bool,
    global_huggingface_configured: impl FnOnce() -> bool,
) -> bool {
    if config.auth.is_some() {
        return true;
    }

    if !config.requires_auth {
        return true;
    }

    if !config.api_key_env.is_empty() {
        return provider_secret_configured(&config.api_key_env);
    }

    global_huggingface_configured()
}

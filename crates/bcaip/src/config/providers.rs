use super::base::{Config, ConfigError};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use std::env;
use tracing::warn;
use yaml_serde::Mapping;
const PROVIDERS_CONFIG_KEY: &str = "providers";
const ACTIVE_PROVIDER_KEY: &str = "active_provider";

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ProviderEntry {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub model: String,
    #[serde(default)]
    pub configured: bool,
}

fn parse_providers_map(raw: Mapping) -> IndexMap<String, ProviderEntry> {
    let mut map = IndexMap::with_capacity(raw.len());
    for (k, v) in raw {
        match (k, yaml_serde::from_value::<ProviderEntry>(v)) {
            (yaml_serde::Value::String(key), Ok(entry)) => {
                map.insert(key, entry);
            }
            (k, v) => {
                warn!(
                    key = ?k,
                    value = ?v,
                    "Skipping malformed provider config entry"
                );
            }
        }
    }
    map
}

fn get_providers_map(config: &Config) -> IndexMap<String, ProviderEntry> {
    let raw: Mapping = config
        .get_param(PROVIDERS_CONFIG_KEY)
        .unwrap_or_else(|_| Default::default());
    parse_providers_map(raw)
}

pub fn get_provider_entry(config: &Config, name: &str) -> Option<ProviderEntry> {
    get_providers_map(config).get(name).cloned()
}

pub fn set_provider_entry(
    config: &Config,
    name: &str,
    entry: &ProviderEntry,
) -> Result<(), ConfigError> {
    let name = name.to_string();
    let entry = entry.clone();
    config.update_param::<Mapping, _, _>(PROVIDERS_CONFIG_KEY, |raw| {
        let mut map = parse_providers_map(raw);
        map.insert(name, entry);
        map
    })
}

pub fn get_active_provider(config: &Config) -> Option<String> {
    if let Ok(val) = env::var("BCAIP_PROVIDER") {
        return Some(val);
    }
    if let Ok(val) = config.get_param::<String>(ACTIVE_PROVIDER_KEY) {
        return Some(val);
    }
    config.get_param::<String>("BCAIP_PROVIDER").ok()
}

pub fn get_active_model(config: &Config) -> Option<String> {
    if let Ok(val) = env::var("BCAIP_MODEL") {
        return Some(val);
    }
    if let Some(provider_name) = get_active_provider(config)
        && let Some(entry) = get_provider_entry(config, &provider_name)
        && !entry.model.is_empty()
    {
        return Some(entry.model);
    }
    config.get_param::<String>("BCAIP_MODEL").ok()
}

pub fn set_active_provider(config: &Config, name: &str, model: &str) -> Result<(), ConfigError> {
    config.set_param(ACTIVE_PROVIDER_KEY, name)?;
    let entry = ProviderEntry {
        enabled: true,
        model: model.to_string(),
        configured: true,
    };
    set_provider_entry(config, name, &entry)
}

pub fn clear_active_provider(config: &Config) -> Result<(), ConfigError> {
    for key in [ACTIVE_PROVIDER_KEY, "BCAIP_PROVIDER", "BCAIP_MODEL"] {
        match config.delete(key) {
            Ok(()) | Err(ConfigError::NotFound(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

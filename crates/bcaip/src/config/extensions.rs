use super::base::Config;
use crate::agents::{ExtensionConfig, extension::PLATFORM_EXTENSIONS};
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use yaml_serde::Mapping;
pub const DEFAULT_EXTENSION: &str = "developer";
pub const DEFAULT_EXTENSION_TIMEOUT: u64 = 300;
pub const DEFAULT_EXTENSION_DESCRIPTION: &str = "";
pub const DEFAULT_DISPLAY_NAME: &str = "Developer";
const EXTENSIONS_CONFIG_KEY: &str = "extensions";

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct ExtensionEntry {
    pub enabled: bool,
    #[serde(flatten)]
    pub config: ExtensionConfig,
}

pub fn name_to_key(name: &str) -> String {
    let mut result = String::with_capacity(name.len());
    for c in name.chars() {
        result.push(match c {
            c if c.is_ascii_alphanumeric() || c == '_' || c == '-' => c,
            c if c.is_whitespace() => continue,
            _ => '_',
        });
    }
    result.to_lowercase()
}

pub(crate) fn is_extension_available(config: &ExtensionConfig) -> bool {
    match config {
        ExtensionConfig::Platform { name, .. } => {
            crate::agents::extension::PLATFORM_EXTENSIONS.contains_key(name_to_key(name).as_str())
        }
        _ => true,
    }
}

fn inject_name_if_missing(key: &str, value: yaml_serde::Value) -> yaml_serde::Value {
    let name_key = yaml_serde::Value::String("name".to_string());
    if let yaml_serde::Value::Mapping(mut map) = value {
        if !map.contains_key(&name_key) {
            map.insert(name_key, yaml_serde::Value::String(key.to_string()));
        }
        yaml_serde::Value::Mapping(map)
    } else {
        value
    }
}

fn parse_extensions_map(raw: &Mapping) -> IndexMap<String, ExtensionEntry> {
    let mut extensions_map = IndexMap::with_capacity(raw.len());
    for (k, v) in raw {
        let Some(key) = k.as_str() else {
            warn!(key = ?k, "Skipping malformed extension config entry");
            continue;
        };

        let v = inject_name_if_missing(key, v.clone());
        match yaml_serde::from_value::<ExtensionEntry>(v) {
            Ok(entry) => {
                if !is_extension_available(&entry.config) {
                    continue;
                }
                extensions_map.insert(key.to_string(), entry);
            }
            Err(err) => {
                info!(
                    key = %key,
                    error = %err,
                    "Skipping malformed extension config entry"
                );
            }
        }
    }

    extensions_map
}

fn get_extensions_map_with_config(config: &Config) -> IndexMap<String, ExtensionEntry> {
    let raw: Mapping = config
        .get_param(EXTENSIONS_CONFIG_KEY)
        .unwrap_or_else(|err| {
            warn!(
                "Failed to load {}: {err}. Falling back to empty object.",
                EXTENSIONS_CONFIG_KEY
            );
            Default::default()
        });

    parse_extensions_map(&raw)
}

fn get_extensions_map() -> IndexMap<String, ExtensionEntry> {
    get_extensions_map_with_config(Config::global())
}

enum ExtensionMutation {
    Upsert(String, Box<ExtensionEntry>),
    Remove(String),
    Noop,
}

fn with_raw_extensions_mapping<F>(config: &Config, mutate: F)
where
    F: FnOnce(&mut IndexMap<String, ExtensionEntry>) -> ExtensionMutation,
{
    let mut serialize_error = None;
    let result = config.update_param::<Mapping, Mapping, _>(EXTENSIONS_CONFIG_KEY, |mut raw| {
        let mut extensions = parse_extensions_map(&raw);

        match mutate(&mut extensions) {
            ExtensionMutation::Upsert(key, entry) => match yaml_serde::to_value(entry) {
                Ok(value) => {
                    raw.insert(yaml_serde::Value::String(key), value);
                }
                Err(err) => {
                    serialize_error = Some(err);
                }
            },
            ExtensionMutation::Remove(key) => {
                raw.shift_remove(key.as_str());
            }
            ExtensionMutation::Noop => {}
        }

        raw
    });

    if let Some(e) = serialize_error {
        warn!("Failed to serialize extensions config entry: {}", e);
    } else if let Err(e) = result {
        warn!("Failed to save extensions config: {}", e);
    }
}

pub fn get_extension_by_name(name: &str) -> Option<ExtensionConfig> {
    get_extension_by_name_with_config(Config::global(), name)
}

fn get_extension_by_name_with_config(config: &Config, name: &str) -> Option<ExtensionConfig> {
    let extensions = get_extensions_map_with_config(config);
    let key = name_to_key(name);

    if let Some(entry) = extensions
        .values()
        .find(|entry| entry.config.name() == name)
        .or_else(|| extensions.get(&key))
    {
        return Some(entry.config.clone());
    }

    get_available_extensions()
        .into_iter()
        .find(|config| config.name() == name || config.key() == key)
}

pub fn set_extension(entry: ExtensionEntry) {
    set_extension_with_config(Config::global(), entry);
}

fn set_extension_with_config(config: &Config, entry: ExtensionEntry) {
    let key = entry.config.key();
    with_raw_extensions_mapping(config, |_| ExtensionMutation::Upsert(key, Box::new(entry)));
}

pub fn remove_extension(key: &str) {
    remove_extension_with_config(Config::global(), key);
}

fn remove_extension_with_config(config: &Config, key: &str) {
    with_raw_extensions_mapping(config, |_| ExtensionMutation::Remove(key.to_string()));
}

/// Returns true when an existing extension was updated, false when the key was missing.
pub fn set_extension_enabled(key: &str, enabled: bool) -> bool {
    set_extension_enabled_with_config(Config::global(), key, enabled)
}

fn set_extension_enabled_with_config(config: &Config, key: &str, enabled: bool) -> bool {
    let mut updated = false;
    with_raw_extensions_mapping(config, |extensions| {
        let Some(entry) = extensions.get_mut(key) else {
            return ExtensionMutation::Noop;
        };

        entry.enabled = enabled;
        updated = true;
        ExtensionMutation::Upsert(key.to_string(), Box::new(entry.clone()))
    });

    updated
}

pub fn get_all_extensions() -> Vec<ExtensionEntry> {
    let extensions = get_extensions_map();
    extensions.into_values().collect()
}

pub fn get_all_extension_names() -> Vec<String> {
    let extensions = get_extensions_map();
    extensions.keys().cloned().collect()
}

pub fn is_extension_enabled(key: &str) -> bool {
    let extensions = get_extensions_map();
    extensions.get(key).map(|e| e.enabled).unwrap_or(false)
}

/// Returns the configured enabled state for an extension, or `None` when it has no entry.
pub fn configured_enabled_state(config: &Config, name: &str) -> Option<bool> {
    let extensions = get_extensions_map_with_config(config);
    let key = name_to_key(name);
    extensions
        .values()
        .find(|entry| entry.config.name() == name)
        .or_else(|| extensions.get(&key))
        .map(|entry| entry.enabled)
}

pub fn get_enabled_extensions() -> Vec<ExtensionConfig> {
    get_all_extensions()
        .into_iter()
        .filter(|ext| ext.enabled)
        .map(|ext| ext.config)
        .collect()
}

pub fn get_enabled_extensions_with_config(config: &Config) -> Vec<ExtensionConfig> {
    get_extensions_map_with_config(config)
        .into_values()
        .filter(|ext| ext.enabled)
        .map(|ext| ext.config)
        .collect()
}

pub fn get_available_extensions() -> Vec<ExtensionConfig> {
    let mut builtin_names = crate::builtin_extension::get_builtin_extension_names();
    builtin_names.sort_unstable();

    let mut platform_definitions = PLATFORM_EXTENSIONS
        .values()
        .filter(|definition| !definition.hidden)
        .collect::<Vec<_>>();
    platform_definitions.sort_unstable_by_key(|definition| definition.name);

    builtin_names
        .into_iter()
        .map(|name| ExtensionConfig::Builtin {
            name: name.to_string(),
            description: String::new(),
            display_name: Some(name.to_string()),
            timeout: None,
            bundled: Some(true),
            available_tools: Vec::new(),
        })
        .chain(
            platform_definitions
                .into_iter()
                .map(|definition| ExtensionConfig::Platform {
                    name: definition.name.to_string(),
                    description: definition.description.to_string(),
                    display_name: Some(definition.display_name.to_string()),
                    bundled: Some(true),
                    available_tools: Vec::new(),
                }),
        )
        .collect()
}

pub fn get_warnings() -> Vec<String> {
    let raw: Mapping = Config::global()
        .get_param(EXTENSIONS_CONFIG_KEY)
        .unwrap_or_default();

    let mut warnings = Vec::new();
    for (k, v) in raw {
        let Some(key) = k.as_str() else {
            continue;
        };
        let Some(extension) = v.as_mapping() else {
            continue;
        };
        let extension_type = extension
            .get(yaml_serde::Value::String("type".to_string()))
            .and_then(yaml_serde::Value::as_str);
        if extension_type == Some("sse") {
            warnings.push(format!(
                "'{}': SSE is unsupported, migrate to streamable_http",
                key
            ));
        }
    }
    warnings
}

pub fn resolve_extensions_for_new_session(
    recipe_extensions: Option<&[ExtensionConfig]>,
    override_extensions: Option<Vec<ExtensionConfig>>,
) -> Vec<ExtensionConfig> {
    let extensions = if let Some(exts) = recipe_extensions {
        exts.to_vec()
    } else if let Some(exts) = override_extensions {
        exts
    } else {
        get_enabled_extensions()
    };

    extensions
        .into_iter()
        .filter(is_extension_available)
        .collect()
}

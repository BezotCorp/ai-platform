use crate::{
    agents::{ExtensionConfig, extension::PLATFORM_EXTENSIONS},
    config::{extensions::ExtensionEntry, providers::ProviderEntry},
};
use yaml_serde::Mapping;
const EXTENSIONS_CONFIG_KEY: &str = "extensions";
const PROVIDERS_CONFIG_KEY: &str = "providers";
const ACTIVE_PROVIDER_KEY: &str = "active_provider";

pub fn run_migrations(config: &mut Mapping) -> bool {
    let mut changed = false;
    changed |= migrate_platform_extensions(config);
    changed |= migrate_provider_config(config);
    changed
}

/// Run only non-destructive migrations suitable for in-memory read paths.
/// Provider migration is excluded because it removes flat keys that
/// `get_param()` callers may still look up directly.
pub fn run_read_migrations(config: &mut Mapping) {
    migrate_platform_extensions(config);
}

fn read_enabled_field(value: &yaml_serde::Value) -> Option<bool> {
    value
        .as_mapping()?
        .get(yaml_serde::Value::String("enabled".to_string()))?
        .as_bool()
}

fn migrate_platform_extensions(config: &mut Mapping) -> bool {
    let extensions_key = yaml_serde::Value::String(EXTENSIONS_CONFIG_KEY.to_string());

    let extensions_value = config
        .get(&extensions_key)
        .cloned()
        .unwrap_or(yaml_serde::Value::Mapping(Mapping::new()));

    let mut extensions_map: Mapping = match extensions_value {
        yaml_serde::Value::Mapping(m) => m,
        _ => Mapping::new(),
    };

    let mut needs_save = false;

    for (name, def) in PLATFORM_EXTENSIONS.iter() {
        let ext_key = yaml_serde::Value::String(name.to_string());
        let existing = extensions_map.get(&ext_key);

        let needs_migration = match existing {
            None => true,
            Some(value) => match yaml_serde::from_value::<ExtensionEntry>(value.clone()) {
                Ok(entry) => match &entry.config {
                    ExtensionConfig::Platform {
                        description,
                        display_name,
                        ..
                    }
                    | ExtensionConfig::Builtin {
                        description,
                        display_name,
                        ..
                    } => {
                        description != def.description
                            || display_name.as_deref() != Some(def.display_name)
                    }
                    _ => true,
                },
                Err(_) => true,
            },
        };

        if needs_migration {
            let existing_entry =
                existing.and_then(|v| yaml_serde::from_value::<ExtensionEntry>(v.clone()).ok());

            let enabled = existing
                .and_then(read_enabled_field)
                .unwrap_or(def.default_enabled);

            let available_tools = existing_entry
                .as_ref()
                .and_then(|entry| match &entry.config {
                    ExtensionConfig::Builtin {
                        available_tools, ..
                    }
                    | ExtensionConfig::Platform {
                        available_tools, ..
                    } => Some(available_tools.clone()),
                    _ => None,
                })
                .unwrap_or_default();

            // If the extension already exists as type 'builtin', preserve that type
            let is_existing_builtin = existing_entry
                .as_ref()
                .is_some_and(|e| matches!(e.config, ExtensionConfig::Builtin { .. }));

            let config = if is_existing_builtin {
                ExtensionConfig::Builtin {
                    name: def.name.to_string(),
                    description: def.description.to_string(),
                    display_name: Some(def.display_name.to_string()),
                    timeout: None,
                    bundled: Some(true),
                    available_tools,
                }
            } else {
                ExtensionConfig::Platform {
                    name: def.name.to_string(),
                    description: def.description.to_string(),
                    display_name: Some(def.display_name.to_string()),
                    bundled: Some(true),
                    available_tools,
                }
            };

            let new_entry = ExtensionEntry { config, enabled };

            if let Ok(value) = yaml_serde::to_value(&new_entry) {
                extensions_map.insert(ext_key, value);
                needs_save = true;
            }
        }
    }

    if needs_save {
        config.insert(extensions_key, yaml_serde::Value::Mapping(extensions_map));
    }

    needs_save
}

/// Remove leftover legacy flat keys when `providers:` block already exists.
fn cleanup_legacy_provider_keys(config: &mut Mapping) -> bool {
    let configured_suffix = "_configured";
    let mut changed = false;

    let stale_keys: Vec<yaml_serde::Value> = config
        .keys()
        .filter(|k| {
            k.as_str()
                .map(|s| {
                    s == "GOOSE_PROVIDER" || s == "GOOSE_MODEL" || s.ends_with(configured_suffix)
                })
                .unwrap_or(false)
        })
        .cloned()
        .collect();

    for key in stale_keys {
        config.shift_remove(&key);
        changed = true;
    }

    changed
}

/// Migrate flat provider keys to the structured `providers:` block.
///
/// Old layout (flat keys):
/// ```yaml
/// GOOSE_PROVIDER: claude-acp
/// GOOSE_MODEL: current
/// claude-acp_configured: true
/// lmstudio_configured: true
/// ```
///
/// New layout:
/// ```yaml
/// active_provider: claude-acp
/// providers:
///   claude-acp:
///     enabled: true
///     model: current
///     configured: true
///   lmstudio:
///     enabled: true
///     model: ""
///     configured: true
/// ```
///
fn migrate_provider_config(config: &mut Mapping) -> bool {
    let providers_key = yaml_serde::Value::String(PROVIDERS_CONFIG_KEY.to_string());

    // If providers block already exists, backfill active_provider from the
    // legacy flat key when missing, then clean up leftover flat keys.
    if config.contains_key(&providers_key) {
        let ap_key = yaml_serde::Value::String(ACTIVE_PROVIDER_KEY.to_string());
        if !config.contains_key(&ap_key)
            && let Some(legacy) = config
                .get(yaml_serde::Value::String("GOOSE_PROVIDER".to_string()))
                .and_then(|v| v.as_str())
        {
            config.insert(ap_key, yaml_serde::Value::String(legacy.to_string()));
        }
        return cleanup_legacy_provider_keys(config);
    }

    // Read the old flat keys, if present.
    let active_provider = config
        .get(yaml_serde::Value::String("GOOSE_PROVIDER".to_string()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    let active_model = config
        .get(yaml_serde::Value::String("GOOSE_MODEL".to_string()))
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
        .unwrap_or_default();

    // Scan for `*_configured` keys to discover all previously-used providers.
    let configured_suffix = "_configured";
    let mut discovered_providers: Vec<String> = config
        .keys()
        .filter_map(|k| {
            k.as_str().and_then(|s| {
                if s.ends_with(configured_suffix) {
                    Some(s.trim_end_matches(configured_suffix).to_string())
                } else {
                    None
                }
            })
        })
        .collect();

    // Ensure the active provider is in the list even if no `*_configured`
    // marker exists for it yet.
    if let Some(ref ap) = active_provider
        && !discovered_providers.contains(ap)
    {
        discovered_providers.push(ap.clone());
    }

    // If there is nothing to migrate, bail out.
    if discovered_providers.is_empty() && active_provider.is_none() {
        return false;
    }

    // Build the providers mapping.
    let mut providers_map = Mapping::new();
    for name in &discovered_providers {
        let is_active = active_provider.as_deref() == Some(name.as_str());
        let model = if is_active {
            active_model.clone()
        } else {
            String::new()
        };
        let entry = ProviderEntry {
            enabled: true,
            model,
            configured: true,
        };
        if let Ok(value) = yaml_serde::to_value(&entry) {
            providers_map.insert(yaml_serde::Value::String(name.clone()), value);
        }
    }

    config.insert(providers_key, yaml_serde::Value::Mapping(providers_map));

    // Write `active_provider` top-level key.
    if let Some(ref ap) = active_provider {
        config.insert(
            yaml_serde::Value::String(ACTIVE_PROVIDER_KEY.to_string()),
            yaml_serde::Value::String(ap.clone()),
        );
    }

    // Remove old flat keys.
    config.shift_remove(yaml_serde::Value::String("GOOSE_PROVIDER".to_string()));
    config.shift_remove(yaml_serde::Value::String("GOOSE_MODEL".to_string()));
    for name in &discovered_providers {
        let marker_key = yaml_serde::Value::String(format!("{}{}", name, configured_suffix));
        config.shift_remove(&marker_key);
    }

    true
}

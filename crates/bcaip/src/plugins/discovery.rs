use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::{Config, paths::Paths};
use crate::plugins::plugin_install_dir;
pub(in crate::plugins) const PLUGINS_CONFIG_KEY: &str = "plugins";

/// Per-plugin entry stored under the `plugins` map in `config.yaml`, keyed by
/// the plugin's filesystem path.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(in crate::plugins) struct PluginConfigEntry {
    pub enabled: bool,
}

/// A plugin found on disk and not disabled by any settings file.
#[derive(Debug, Clone)]
pub struct DiscoveredPlugin {
    pub name: String,
    pub root: PathBuf,
    pub scope: PluginScope,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginScope {
    User,
    Project,
}

/// Settings file format from <https://open-plugins.com/plugin-builders/installation>.
#[derive(Debug, Default, Deserialize)]
struct PluginSettings {
    #[serde(default, rename = "enabledPlugins")]
    enabled: Vec<String>,
    #[serde(default, rename = "disabledPlugins")]
    disabled: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum SettingsScope {
    Local,
    Project,
    User,
}

/// Discover all plugins that should be considered active.
///
/// `project_root`, when supplied, enables project + local scope settings and
/// project-scope `.agents/plugins/` lookups.
pub fn discover_enabled_plugins(project_root: Option<&Path>) -> Vec<DiscoveredPlugin> {
    discover_enabled_plugins_with_config(project_root, Config::global())
}

pub(crate) fn discover_enabled_plugins_with_config(
    project_root: Option<&Path>,
    config: &Config,
) -> Vec<DiscoveredPlugin> {
    let scoped_settings = load_all_settings(project_root);
    let user_plugins_dir = plugin_install_dir();
    let mut found = Vec::new();

    if let Some(root) = project_root {
        let project_plugins_dir = project_plugin_dir(root);
        if !equivalent_paths(&project_plugins_dir, &user_plugins_dir) {
            found.extend(list_dir_children(&project_plugins_dir).into_iter().map(
                |(name, root)| DiscoveredPlugin {
                    name,
                    root,
                    scope: PluginScope::Project,
                },
            ));
        }
    }
    found.extend(
        list_dir_children(&user_plugins_dir)
            .into_iter()
            .map(|(name, root)| DiscoveredPlugin {
                name,
                root,
                scope: PluginScope::User,
            }),
    );

    let mut enabled_plugins: Vec<DiscoveredPlugin> = filter_by_config(found, config)
        .into_iter()
        .filter(|plugin| is_enabled(&plugin.name, &scoped_settings))
        .collect();
    enabled_plugins.sort_by(|left, right| {
        plugin_scope_rank(left.scope)
            .cmp(&plugin_scope_rank(right.scope))
            .then_with(|| left.name.cmp(&right.name))
            .then_with(|| left.root.cmp(&right.root))
    });

    let mut seen_names = HashSet::new();
    enabled_plugins
        .into_iter()
        .filter(|plugin| seen_names.insert(plugin.name.clone()))
        .collect()
}

fn equivalent_paths(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .zip(right.canonicalize().ok())
            .is_some_and(|(left, right)| left == right)
}

fn plugin_scope_rank(scope: PluginScope) -> u8 {
    match scope {
        PluginScope::Project => 0,
        PluginScope::User => 1,
    }
}

/// Apply the `plugins` map in `config.yaml`. Newly discovered plugins are added
/// to the map with `enabled: true`; plugins explicitly set to `enabled: false`
/// are dropped.
fn filter_by_config(plugins: Vec<DiscoveredPlugin>, config: &Config) -> Vec<DiscoveredPlugin> {
    let mut entries: HashMap<String, PluginConfigEntry> =
        config.get_param(PLUGINS_CONFIG_KEY).unwrap_or_default();

    let mut dirty = false;
    let mut enabled = Vec::new();
    for plugin in plugins {
        let key = plugin.root.to_string_lossy().to_string();
        match entries.get(&key) {
            Some(entry) => {
                if entry.enabled {
                    enabled.push(plugin);
                }
            }
            None => {
                entries.insert(key, PluginConfigEntry { enabled: true });
                dirty = true;
                enabled.push(plugin);
            }
        }
    }

    if dirty && let Err(e) = config.set_param(PLUGINS_CONFIG_KEY, entries) {
        tracing::warn!(error = %e, "Failed to persist plugin config entries");
    }

    enabled
}

fn is_enabled(plugin_name: &str, scoped_settings: &[(SettingsScope, PluginSettings)]) -> bool {
    for scope in [
        SettingsScope::Local,
        SettingsScope::Project,
        SettingsScope::User,
    ] {
        let Some(settings) = scoped_settings
            .iter()
            .find_map(|(s, settings)| (*s == scope).then_some(settings))
        else {
            continue;
        };

        let listed_disabled = settings.disabled.iter().any(|n| n == plugin_name);
        let listed_enabled = settings.enabled.iter().any(|n| n == plugin_name);

        if listed_disabled {
            return false;
        }
        if listed_enabled {
            return true;
        }
    }

    true
}

fn project_plugin_dir(project_root: &Path) -> PathBuf {
    project_root.join(".agents").join("plugins")
}

fn list_dir_children(dir: &Path) -> Vec<(String, PathBuf)> {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let name = path.file_name()?.to_str()?.to_string();
            Some((name, path))
        })
        .collect()
}

fn load_all_settings(project_root: Option<&Path>) -> Vec<(SettingsScope, PluginSettings)> {
    let mut paths: Vec<(SettingsScope, PathBuf)> = Vec::new();
    if let Some(path) = user_settings_path() {
        paths.push((SettingsScope::User, path));
    }
    if let Some(root) = project_root {
        paths.push((SettingsScope::Project, project_settings_path(root, false)));
        paths.push((SettingsScope::Local, project_settings_path(root, true)));
    }

    paths
        .into_iter()
        .filter_map(|(scope, path)| match read_settings(&path) {
            Ok(Some(s)) => Some((scope, s)),
            Ok(None) => None,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "Failed to read plugin settings");
                None
            }
        })
        .collect()
}

fn user_settings_path() -> Option<PathBuf> {
    if let Some(path_root) = Paths::path_root() {
        return Some(
            path_root
                .join(".config")
                .join("bcaip")
                .join("settings.json"),
        );
    }
    Some(
        dirs::home_dir()?
            .join(".config")
            .join("bcaip")
            .join("settings.json"),
    )
}

fn project_settings_path(project_root: &Path, local: bool) -> PathBuf {
    let file = if local {
        "settings.local.json"
    } else {
        "settings.json"
    };
    project_root.join(".config").join("bcaip").join(file)
}

fn read_settings(path: &Path) -> anyhow::Result<Option<PluginSettings>> {
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(path)?;
    let parsed: PluginSettings = serde_json::from_str(&text)?;
    Ok(Some(parsed))
}

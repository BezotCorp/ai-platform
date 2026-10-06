use crate::config::paths::Paths;
use fs2::FileExt;
use rmcp::model::Tool;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, RwLock};
use tempfile::NamedTempFile;
use tracing;
const PERMISSION_FILE: &str = "permission.yaml";

static PERMISSION_MANAGER: LazyLock<Arc<PermissionManager>> =
    LazyLock::new(|| Arc::new(PermissionManager::new(Paths::config_dir())));

/// Enum representing the possible permission levels for a tool.
#[derive(Debug, Deserialize, Serialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PermissionLevel {
    AlwaysAllow, // Tool can always be used without prompt
    AskBefore,   // Tool requires permission to be granted before use
    NeverAllow,  // Tool is never allowed to be used
}

/// Struct representing the configuration of permissions, categorized by level.
#[derive(Debug, Deserialize, Serialize, Default, Clone)]
pub struct PermissionConfig {
    pub always_allow: Vec<String>, // List of tools that are always allowed
    pub ask_before: Vec<String>,   // List of tools that require user consent
    pub never_allow: Vec<String>,  // List of tools that are never allowed
}

/// PermissionManager manages permission configurations for various tools.
#[derive(Debug)]
pub struct PermissionManager {
    config_path: PathBuf,
    permission_map: RwLock<HashMap<String, PermissionConfig>>,
}

// Constants representing specific permission categories
const USER_PERMISSION: &str = "user";
const SMART_APPROVE_PERMISSION: &str = "smart_approve";

impl PermissionManager {
    pub fn new(config_dir: PathBuf) -> Self {
        let permission_path = config_dir.join(PERMISSION_FILE);
        let permission_map = if permission_path.exists() {
            Self::load_permission_map(&permission_path)
        } else {
            // Consolidate directory creation for re-use in global singleton or ACP.
            fs::create_dir_all(&config_dir).expect("Failed to create config directory");
            HashMap::new()
        };
        PermissionManager {
            config_path: permission_path,
            permission_map: RwLock::new(permission_map),
        }
    }

    pub fn instance() -> Arc<PermissionManager> {
        Arc::clone(&PERMISSION_MANAGER)
    }

    /// Returns a list of all the names (keys) in the permission map.
    pub fn get_permission_names(&self) -> Vec<String> {
        self.permission_map
            .read()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    /// Retrieves the user permission level for a specific tool.
    pub fn get_user_permission(&self, principal_name: &str) -> Option<PermissionLevel> {
        self.get_permission(USER_PERMISSION, principal_name)
    }

    /// Retrieves the smart approve permission level for a specific tool.
    pub fn get_smart_approve_permission(&self, principal_name: &str) -> Option<PermissionLevel> {
        self.get_permission(SMART_APPROVE_PERMISSION, principal_name)
    }

    /// Retrieves the config file path.
    pub fn get_config_path(&self) -> &Path {
        self.config_path.as_path()
    }

    pub fn apply_tool_annotations(&self, tools: &[Tool]) {
        let mut write_annotated = Vec::new();
        for tool in tools {
            let Some(anns) = &tool.annotations else {
                continue;
            };
            if anns.read_only_hint == Some(false) {
                write_annotated.push(tool.name.to_string());
            }
        }
        if !write_annotated.is_empty() {
            self.bulk_update_smart_approve_permissions(
                &write_annotated,
                PermissionLevel::AskBefore,
            );
        }
    }

    fn bulk_update_smart_approve_permissions(&self, tool_names: &[String], level: PermissionLevel) {
        self.mutate_permission_map(|map| {
            let permission_config = map.entry(SMART_APPROVE_PERMISSION.to_string()).or_default();

            for tool_name in tool_names {
                permission_config.always_allow.retain(|p| p != tool_name);
                permission_config.ask_before.retain(|p| p != tool_name);
                permission_config.never_allow.retain(|p| p != tool_name);

                match &level {
                    PermissionLevel::AlwaysAllow => {
                        permission_config.always_allow.push(tool_name.clone())
                    }
                    PermissionLevel::AskBefore => {
                        permission_config.ask_before.push(tool_name.clone())
                    }
                    PermissionLevel::NeverAllow => {
                        permission_config.never_allow.push(tool_name.clone())
                    }
                }
            }
        });
    }

    /// Helper function to retrieve the permission level for a specific permission category and tool.
    fn get_permission(&self, name: &str, principal_name: &str) -> Option<PermissionLevel> {
        let map = self.permission_map.read().unwrap();
        // Check if the permission category exists in the map
        if let Some(permission_config) = map.get(name) {
            // Check the permission levels for the given tool
            if permission_config
                .never_allow
                .contains(&principal_name.to_string())
            {
                return Some(PermissionLevel::NeverAllow);
            } else if permission_config
                .always_allow
                .contains(&principal_name.to_string())
            {
                return Some(PermissionLevel::AlwaysAllow);
            } else if permission_config
                .ask_before
                .contains(&principal_name.to_string())
            {
                return Some(PermissionLevel::AskBefore);
            }
        }
        None // Return None if no matching permission level is found
    }

    /// Updates the user permission level for a specific tool.
    pub fn update_user_permission(&self, principal_name: &str, level: PermissionLevel) {
        self.update_permission(USER_PERMISSION, principal_name, level)
    }

    /// Updates the smart approve permission level for a specific tool.
    pub fn update_smart_approve_permission(&self, principal_name: &str, level: PermissionLevel) {
        self.update_permission(SMART_APPROVE_PERMISSION, principal_name, level)
    }

    /// Helper function to update a permission level for a specific tool in a given permission category.
    fn update_permission(&self, name: &str, principal_name: &str, level: PermissionLevel) {
        self.mutate_permission_map(|map| {
            let permission_config = map.entry(name.to_string()).or_default();

            permission_config
                .always_allow
                .retain(|p| p != principal_name);
            permission_config.ask_before.retain(|p| p != principal_name);
            permission_config
                .never_allow
                .retain(|p| p != principal_name);

            match level {
                PermissionLevel::AlwaysAllow => permission_config
                    .always_allow
                    .push(principal_name.to_string()),
                PermissionLevel::AskBefore => permission_config
                    .ask_before
                    .push(principal_name.to_string()),
                PermissionLevel::NeverAllow => permission_config
                    .never_allow
                    .push(principal_name.to_string()),
            }
        });
    }

    fn mutate_permission_map<F>(&self, mutation: F)
    where
        F: FnOnce(&mut HashMap<String, PermissionConfig>),
    {
        let mut in_memory_map = self.permission_map.write().unwrap();
        let storage_path = Self::permission_storage_path(&self.config_path);
        let lock_path = storage_path.with_extension("yaml.lock");
        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)
            .expect("Failed to open permission.yaml lock file");
        lock_file
            .lock_exclusive()
            .expect("Failed to lock permission.yaml");

        let mut latest_map = if storage_path.exists() {
            Self::load_permission_map(&storage_path)
        } else {
            HashMap::new()
        };
        mutation(&mut latest_map);
        Self::write_permission_map(&storage_path, &latest_map);
        *in_memory_map = latest_map;
    }

    fn load_permission_map(config_path: &Path) -> HashMap<String, PermissionConfig> {
        let file_contents =
            fs::read_to_string(config_path).expect("Failed to read permission.yaml");
        yaml_serde::from_str(&file_contents).unwrap_or_else(|error| {
            tracing::error!(
                "Failed to parse {}: {}. Refusing to start with corrupted permission config.",
                config_path.display(),
                error,
            );
            panic!(
                "Corrupted permission config at {}. Fix or remove the file to continue.",
                config_path.display(),
            );
        })
    }

    fn permission_storage_path(config_path: &Path) -> PathBuf {
        match fs::symlink_metadata(config_path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                let target =
                    fs::read_link(config_path).expect("Failed to resolve permission.yaml symlink");
                if target.is_absolute() {
                    target
                } else {
                    config_path
                        .parent()
                        .expect("permission.yaml must have a parent directory")
                        .join(target)
                }
            }
            Ok(_) => config_path.to_path_buf(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => config_path.to_path_buf(),
            Err(error) => panic!("Failed to inspect permission.yaml: {error}"),
        }
    }

    fn write_permission_map(storage_path: &Path, map: &HashMap<String, PermissionConfig>) {
        let yaml_content =
            yaml_serde::to_string(map).expect("Failed to serialize permission config");
        let config_dir = storage_path
            .parent()
            .expect("permission.yaml must have a parent directory");
        let mut temporary_file =
            NamedTempFile::new_in(config_dir).expect("Failed to write to permission.yaml");
        temporary_file
            .write_all(yaml_content.as_bytes())
            .expect("Failed to write to permission.yaml");
        temporary_file
            .as_file()
            .sync_all()
            .expect("Failed to write to permission.yaml");
        temporary_file
            .persist(storage_path)
            .expect("Failed to write to permission.yaml");
    }

    pub fn remove_extension(&self, extension_name: &str) {
        self.mutate_permission_map(|map| {
            for permission_config in map.values_mut() {
                permission_config
                    .always_allow
                    .retain(|p| !Self::belongs_to_extension(p, extension_name));
                permission_config
                    .ask_before
                    .retain(|p| !Self::belongs_to_extension(p, extension_name));
                permission_config
                    .never_allow
                    .retain(|p| !Self::belongs_to_extension(p, extension_name));
            }
        });
    }

    pub fn clear_permissions(&self) {
        self.mutate_permission_map(HashMap::clear);
    }

    fn belongs_to_extension(principal_name: &str, extension_name: &str) -> bool {
        !extension_name.is_empty()
            && principal_name
                .strip_prefix(extension_name)
                .is_some_and(|suffix| suffix.starts_with("__"))
    }
}

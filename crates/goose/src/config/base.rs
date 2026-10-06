use crate::config;
use crate::config::paths::Paths;
use crate::providers::private_file::{private_file_target_path, write_private_file};
use fs2::FileExt;
use bcaip_provider_types::goose_mode::GooseMode;
use bcaip_provider_types::thinking::ThinkingEffort;
#[cfg(feature = "system-keyring")]
use keyring::Entry;
use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::{collections::HashMap, env, fs::OpenOptions, io::Write};
use thiserror::Error;
use yaml_serde::Mapping;
fn write_secrets_file(path: &Path, content: &str) -> std::io::Result<()> {
    write_private_file(path, content)
}

fn secrets_lock_path(path: &Path) -> PathBuf {
    let mut lock_path = path.as_os_str().to_os_string();
    lock_path.push(".lock");
    PathBuf::from(lock_path)
}

#[cfg(feature = "system-keyring")]
const KEYRING_SERVICE: &str = "goose";
#[cfg(feature = "system-keyring")]
const KEYRING_USERNAME: &str = "secrets";
pub const CONFIG_YAML_NAME: &str = "config.yaml";

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Configuration value not found: {0}")]
    NotFound(String),
    #[error("Failed to deserialize value: {0}")]
    DeserializeError(String),
    #[error("Failed to read config file: {0}")]
    FileError(#[from] std::io::Error),
    #[error("Failed to create config directory: {0}")]
    DirectoryError(String),
    #[error("Failed to access keyring: {0}")]
    KeyringError(String),
    #[error("Failed to lock config file: {0}")]
    LockError(String),
    #[error("Secret stored using file-based fallback")]
    FallbackToFileStorage,
}

impl From<serde_json::Error> for ConfigError {
    fn from(err: serde_json::Error) -> Self {
        ConfigError::DeserializeError(err.to_string())
    }
}

impl From<yaml_serde::Error> for ConfigError {
    fn from(err: yaml_serde::Error) -> Self {
        ConfigError::DeserializeError(err.to_string())
    }
}

#[cfg(feature = "system-keyring")]
impl From<keyring::Error> for ConfigError {
    fn from(err: keyring::Error) -> Self {
        ConfigError::KeyringError(err.to_string())
    }
}

/// Configuration management for goose.
///
/// This module provides a flexible configuration system that supports:
/// - Dynamic configuration keys
/// - Multiple value types through serde deserialization
/// - Environment variable overrides
/// - YAML-based configuration file storage
/// - Hot reloading of configuration changes
/// - Secure secret storage in system keyring
///
/// Configuration values are loaded with the following precedence:
/// 1. Environment variables (exact key match)
/// 2. Configuration file (~/.config/goose/config.yaml by default)
///
/// Secrets are loaded with the following precedence:
/// 1. Environment variables (exact key match)
/// 2. System keyring (which can be disabled with GOOSE_DISABLE_KEYRING)
/// 3. If the keyring is disabled, secrets are stored in a secrets file
///    (~/.config/goose/secrets.yaml by default)
///
/// # Examples
///
/// ```no_run
/// use goose::config::Config;
/// use serde::Deserialize;
///
/// // Get a string value
/// let config = Config::global();
/// let api_key: String = config.get_param("OPENAI_API_KEY").unwrap();
///
/// // Get a complex type
/// #[derive(Deserialize)]
/// struct ServerConfig {
///     host: String,
///     port: u16,
/// }
///
/// let server_config: ServerConfig = config.get_param("server").unwrap();
/// ```
///
/// # Naming Convention
/// we recommend snake_case for keys, and will convert to UPPERCASE when
/// checking for environment overrides. e.g. openai_api_key will check for an
/// environment variable OPENAI_API_KEY
///
/// For goose-specific configuration, consider prefixing with "goose_" to avoid conflicts.
pub struct Config {
    /// Ordered list of config files to load and merge.
    /// Later entries take precedence over earlier ones.
    /// The last entry is where changes will be written.
    config_paths: Vec<PathBuf>,
    secrets: SecretStorage,
    guard: Mutex<()>,
    secrets_cache: Arc<Mutex<Option<HashMap<String, Value>>>>,
}

enum SecretStorage {
    #[cfg(feature = "system-keyring")]
    Keyring {
        service: String,
    },
    File {
        path: PathBuf,
    },
}

enum SecretMutation<T> {
    Write(T),
    Unchanged(T),
}

pub(crate) enum SecretUpdate<V, R> {
    Write(V, R),
    Unchanged(R),
}

// Global instance
static GLOBAL_CONFIG: OnceCell<Config> = OnceCell::new();

fn system_config_path() -> PathBuf {
    #[cfg(unix)]
    {
        PathBuf::from("/etc/goose/config.yaml")
    }
    #[cfg(windows)]
    {
        env::var("PROGRAMDATA")
            .map(|d| PathBuf::from(d).join("goose").join("config.yaml"))
            .unwrap_or_else(|_| PathBuf::from(r"C:\ProgramData\goose\config.yaml"))
    }
}

fn additional_config_paths_from_env() -> Vec<PathBuf> {
    env::var_os("GOOSE_ADDITIONAL_CONFIG_FILES")
        .map(|value| env::split_paths(&value).collect())
        .unwrap_or_default()
}

fn metadata_is_symlink_or_reparse_point(metadata: &std::fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }

    #[cfg(not(windows))]
    {
        false
    }
}

impl Default for Config {
    fn default() -> Self {
        let config_dir = Paths::config_dir();
        let user_config_path = config_dir.join(CONFIG_YAML_NAME);

        let mut config_paths = vec![system_config_path()];
        config_paths.extend(additional_config_paths_from_env());
        config_paths.push(user_config_path.clone());

        let no_secrets_config = Self {
            config_paths: config_paths.clone(),
            secrets: SecretStorage::File {
                path: Default::default(),
            },
            guard: Mutex::new(()),
            secrets_cache: Arc::new(Mutex::new(None)),
        };

        let keyring_disabled = keyring_disabled_by_environment()
            || no_secrets_config
                .get_param::<yaml_serde::Value>("GOOSE_DISABLE_KEYRING")
                .is_ok_and(|v| keyring_disabled_value(&v));
        let secrets = secret_storage(&config_dir, keyring_disabled, default_keyring_service());
        Self {
            config_paths,
            secrets,
            guard: Mutex::new(()),
            secrets_cache: Arc::new(Mutex::new(None)),
        }
    }
}

pub trait ConfigValue {
    const KEY: &'static str;
    const DEFAULT: &'static str;
}

macro_rules! config_value {
    ($key:ident, $type:ty) => {
        impl Config {
            pastey::paste! {
                pub fn [<get_ $key:lower>](&self) -> Result<$type, ConfigError> {
                    self.get_param(stringify!($key))
                }
            }
            pastey::paste! {
                pub fn [<set_ $key:lower>](&self, v: impl Into<$type>) -> Result<(), ConfigError> {
                    self.set_param(stringify!($key), &v.into())
                }
            }
        }
    };

    ($key:ident, $inner:ty, $default:expr) => {
        pastey::paste! {
            #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
            #[serde(transparent)]
            pub struct [<$key:camel>]($inner);

            impl ConfigValue for [<$key:camel>] {
                const KEY: &'static str = stringify!($key);
                const DEFAULT: &'static str = $default;
            }

            impl Default for [<$key:camel>] {
                fn default() -> Self {
                    [<$key:camel>]($default.into())
                }
            }

            impl std::ops::Deref for [<$key:camel>] {
                type Target = $inner;

                fn deref(&self) -> &Self::Target {
                    &self.0
                }
            }

            impl std::ops::DerefMut for [<$key:camel>] {
                fn deref_mut(&mut self) -> &mut Self::Target {
                    &mut self.0
                }
            }

            impl std::fmt::Display for [<$key:camel>] {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    write!(f, "{:?}", self.0)
                }
            }

            impl From<$inner> for [<$key:camel>] {
                fn from(value: $inner) -> Self {
                    [<$key:camel>](value)
                }
            }

            impl From<[<$key:camel>]> for $inner {
                fn from(value: [<$key:camel>]) -> $inner {
                    value.0
                }
            }

            config_value!($key, [<$key:camel>]);
        }
    };
}

fn parse_yaml_content(content: &str) -> Result<Mapping, ConfigError> {
    yaml_serde::from_str(content).map_err(|e| e.into())
}

fn keyring_disabled_value(value: &yaml_serde::Value) -> bool {
    value.as_bool().unwrap_or(false) || value.as_str().is_some_and(|s| s == "true" || s == "1")
}

const EXTENSIONS_KEY: &str = "extensions";
const PROVIDERS_KEY: &str = "providers";

pub fn merge_config_values(base: &mut Mapping, overlay: Mapping) {
    let extensions_key = yaml_serde::Value::String(EXTENSIONS_KEY.to_string());
    let providers_key = yaml_serde::Value::String(PROVIDERS_KEY.to_string());

    for (key, overlay_value) in overlay {
        if key == extensions_key {
            let base_ext = base
                .entry(key.clone())
                .or_insert_with(|| yaml_serde::Value::Mapping(Mapping::new()));
            if let (Some(base_map), Some(overlay_map)) =
                (base_ext.as_mapping_mut(), overlay_value.as_mapping())
            {
                merge_nested_entries(base_map, overlay_map);
            } else {
                base.insert(key, overlay_value);
            }
        } else if key == providers_key {
            let base_prov = base
                .entry(key.clone())
                .or_insert_with(|| yaml_serde::Value::Mapping(Mapping::new()));
            if let (Some(base_map), Some(overlay_map)) =
                (base_prov.as_mapping_mut(), overlay_value.as_mapping())
            {
                merge_nested_entries(base_map, overlay_map);
            } else {
                base.insert(key, overlay_value);
            }
        } else {
            base.insert(key, overlay_value);
        }
    }
}

fn merge_nested_entries(base: &mut Mapping, overlay: &Mapping) {
    for (ext_key, overlay_ext) in overlay {
        match base.get_mut(ext_key) {
            Some(base_ext) => {
                if let (Some(base_map), Some(overlay_map)) =
                    (base_ext.as_mapping_mut(), overlay_ext.as_mapping())
                {
                    for (field_key, field_value) in overlay_map {
                        base_map.insert(field_key.clone(), field_value.clone());
                    }
                } else {
                    *base_ext = overlay_ext.clone();
                }
            }
            None => {
                base.insert(ext_key.clone(), overlay_ext.clone());
            }
        }
    }
}

/// Set once the system keyring has proven unusable, so later `Config` instances in this process
/// go straight to file storage.
static KEYRING_UNAVAILABLE: AtomicBool = AtomicBool::new(false);

fn keyring_disabled_by_environment() -> bool {
    env::var("GOOSE_DISABLE_KEYRING").is_ok() || KEYRING_UNAVAILABLE.load(Ordering::Relaxed)
}

/// Read the GOOSE_DISABLE_KEYRING flag from the config file.
///
/// Called before Config is fully initialised, so we do a minimal raw read
/// rather than going through `get_param`.  All errors are treated as `false`
/// (keyring stays enabled) so a missing/malformed file is never fatal here.
fn keyring_disabled_in_config(config_path: &Path) -> bool {
    std::fs::read_to_string(config_path)
        .ok()
        .and_then(|s| parse_yaml_content(&s).ok())
        .and_then(|m| m.get("GOOSE_DISABLE_KEYRING").map(keyring_disabled_value))
        .unwrap_or(false)
}

#[cfg(feature = "system-keyring")]
fn default_keyring_service() -> &'static str {
    KEYRING_SERVICE
}

#[cfg(not(feature = "system-keyring"))]
fn default_keyring_service() -> &'static str {
    ""
}

fn secrets_file_path_in(config_dir: &Path) -> PathBuf {
    config_dir.join("secrets.yaml")
}

#[cfg(feature = "system-keyring")]
fn secret_storage(config_dir: &Path, keyring_disabled: bool, service: &str) -> SecretStorage {
    if keyring_disabled {
        SecretStorage::File {
            path: secrets_file_path_in(config_dir),
        }
    } else {
        SecretStorage::Keyring {
            service: service.to_string(),
        }
    }
}

#[cfg(not(feature = "system-keyring"))]
fn secret_storage(config_dir: &Path, _keyring_disabled: bool, _service: &str) -> SecretStorage {
    SecretStorage::File {
        path: secrets_file_path_in(config_dir),
    }
}

impl Config {
    /// Get the global configuration instance.
    ///
    /// This will initialize the configuration with the default path (~/.config/goose/config.yaml)
    /// if it hasn't been initialized yet.
    pub fn global() -> &'static Config {
        GLOBAL_CONFIG.get_or_init(Config::default)
    }

    /// Create a new configuration instance with custom paths
    ///
    /// This is primarily useful for testing or for applications that need
    /// to manage multiple configuration files.
    pub fn new<P: AsRef<Path>>(config_path: P, service: &str) -> Result<Self, ConfigError> {
        let config_path = config_path.as_ref().to_path_buf();
        let keyring_disabled =
            keyring_disabled_by_environment() || keyring_disabled_in_config(&config_path);
        let config_dir = config_path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(Paths::config_dir);
        let secrets = secret_storage(&config_dir, keyring_disabled, service);
        Ok(Config {
            config_paths: vec![config_path],
            secrets,
            guard: Mutex::new(()),
            secrets_cache: Arc::new(Mutex::new(None)),
        })
    }

    /// Create a new configuration instance with custom paths
    ///
    /// This is primarily useful for testing or for applications that need
    /// to manage multiple configuration files.
    pub fn new_with_file_secrets<P1: AsRef<Path>, P2: AsRef<Path>>(
        config_path: P1,
        secrets_path: P2,
    ) -> Result<Self, ConfigError> {
        Ok(Config {
            config_paths: vec![config_path.as_ref().to_path_buf()],
            secrets: SecretStorage::File {
                path: secrets_path.as_ref().to_path_buf(),
            },
            guard: Mutex::new(()),
            secrets_cache: Arc::new(Mutex::new(None)),
        })
    }

    pub fn new_with_config_paths<P1: AsRef<Path>>(
        config_paths: Vec<PathBuf>,
        secrets_path: P1,
    ) -> Result<Self, ConfigError> {
        Ok(Config {
            config_paths,
            secrets: SecretStorage::File {
                path: secrets_path.as_ref().to_path_buf(),
            },
            guard: Mutex::new(()),
            secrets_cache: Arc::new(Mutex::new(None)),
        })
    }

    fn write_path(&self) -> &PathBuf {
        self.config_paths
            .last()
            .expect("config_paths must not be empty")
    }

    pub fn exists(&self) -> bool {
        self.config_paths.iter().any(|p| p.exists())
    }

    pub fn clear(&self) -> Result<(), ConfigError> {
        Ok(std::fs::remove_file(self.write_path())?)
    }

    pub fn path(&self) -> String {
        self.write_path().to_string_lossy().to_string()
    }

    /// Load only the writable config file for read-modify-write operations.
    /// Returns an empty mapping if the file doesn't exist or can't be parsed.
    fn load_write_config(&self) -> Result<Mapping, ConfigError> {
        if !self.write_path().exists() {
            return Ok(Mapping::new());
        }
        let content = std::fs::read_to_string(self.write_path())?;
        let mut values = parse_yaml_content(&content).unwrap_or_else(|e| {
            tracing::warn!(
                "Config file {:?} is corrupt: {}. Starting fresh.",
                self.write_path(),
                e
            );
            Mapping::new()
        });

        if config::migrations::run_migrations(&mut values)
            && let Err(e) = self.save_values(&values)
        {
            tracing::warn!("Failed to save migrated config: {}", e);
        }

        Ok(values)
    }

    fn load(&self) -> Result<Mapping, ConfigError> {
        let mut merged = Mapping::new();

        for path in &self.config_paths {
            if !path.exists() {
                continue;
            }
            match std::fs::read_to_string(path)
                .map_err(ConfigError::from)
                .and_then(|content| parse_yaml_content(&content))
            {
                Ok(layer) => {
                    tracing::debug!("Loading config from: {:?}", path);
                    merge_config_values(&mut merged, layer);
                }
                Err(e) => {
                    tracing::warn!("Failed to load config {:?}: {}. Skipping.", path, e);
                }
            }
        }

        config::migrations::run_read_migrations(&mut merged);

        Ok(merged)
    }

    fn load_strict(&self) -> Result<Mapping, ConfigError> {
        let mut merged = Mapping::new();

        for path in &self.config_paths {
            match std::fs::symlink_metadata(path) {
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    for ancestor in path.ancestors().skip(1) {
                        match std::fs::symlink_metadata(ancestor) {
                            Ok(metadata) if metadata_is_symlink_or_reparse_point(&metadata) => {
                                std::fs::metadata(ancestor)?;
                            }
                            Ok(_) => {}
                            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                            Err(error) => return Err(error.into()),
                        }
                    }
                    continue;
                }
                Err(error) => return Err(error.into()),
            }
            let content = std::fs::read_to_string(path)?;
            let layer = parse_yaml_content(&content)?;
            merge_config_values(&mut merged, layer);
        }

        config::migrations::run_read_migrations(&mut merged);

        Ok(merged)
    }

    pub fn all_values(&self) -> Result<HashMap<String, Value>, ConfigError> {
        let config_values = self.load()?;
        let mut map = HashMap::from_iter(config_values.into_iter().filter_map(|(k, v)| {
            k.as_str()
                .map(|k| k.to_string())
                .zip(serde_json::to_value(v).ok())
        }));

        if let Ok(provider) = self.get_goose_provider() {
            map.insert("GOOSE_PROVIDER".to_string(), Value::String(provider));
        }
        if let Ok(model) = self.get_goose_model() {
            map.insert("GOOSE_MODEL".to_string(), Value::String(model));
        }

        Ok(map)
    }

    fn config_write_target_path(&self) -> Result<PathBuf, ConfigError> {
        let mut path = self.write_path().clone();

        // Follow symlinks so we update the target file without replacing the link itself.
        const MAX_SYMLINK_HOPS: usize = 1;
        let mut hops = 0usize;
        loop {
            match std::fs::symlink_metadata(&path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    if hops >= MAX_SYMLINK_HOPS {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            format!(
                                "Too many symlink levels (or a cycle) while resolving config path: {:?}",
                                self.write_path()
                            ),
                        )
                        .into());
                    }
                    hops += 1;

                    let link = std::fs::read_link(&path)?;
                    path = if link.is_absolute() {
                        link
                    } else {
                        path.parent().unwrap_or_else(|| Path::new(".")).join(link)
                    };
                }
                Ok(_) => return Ok(path),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(path),
                Err(e) => return Err(e.into()),
            }
        }
    }

    fn save_values(&self, values: &Mapping) -> Result<(), ConfigError> {
        let target_path = self.config_write_target_path()?;

        // Convert to YAML for storage
        let yaml_value = yaml_serde::to_string(values)?;

        if let Some(parent) = target_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ConfigError::DirectoryError(e.to_string()))?;
        }

        // Write to a temporary file first for atomic operation
        let temp_path = target_path.with_extension("tmp");

        {
            let mut file = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&temp_path)?;

            // Acquire an exclusive lock
            file.lock_exclusive()
                .map_err(|e| ConfigError::LockError(e.to_string()))?;

            // Write the contents using the same file handle
            file.write_all(yaml_value.as_bytes())?;
            file.sync_all()?;

            // Unlock is handled automatically when file is dropped
        }

        // Atomically replace the original file
        std::fs::rename(&temp_path, &target_path)?;

        Ok(())
    }

    pub fn initialize_if_empty(&self, values: Mapping) -> Result<(), ConfigError> {
        let _guard = self.guard.lock().unwrap();
        if !self.exists() {
            self.save_values(&values)
        } else {
            Ok(())
        }
    }

    pub fn all_secrets(&self) -> Result<HashMap<String, Value>, ConfigError> {
        let mut cache = self.secrets_cache.lock().unwrap();

        let values = if let Some(ref cached_secrets) = *cache {
            cached_secrets.clone()
        } else {
            tracing::debug!("secrets cache miss, fetching from storage");
            let loaded = self.load_secrets_from_storage()?;

            *cache = Some(loaded.clone());
            loaded
        };

        Ok(values)
    }

    /// Parse an environment variable value into a JSON Value.
    ///
    /// This function tries to intelligently parse environment variable values:
    /// 1. First attempts JSON parsing (for structured data)
    /// 2. If that fails, tries primitive type parsing for common cases
    /// 3. Falls back to string if nothing else works
    fn parse_env_value(val: &str) -> Result<Value, ConfigError> {
        // First try JSON parsing - this handles quoted strings, objects, arrays, etc.
        if let Ok(json_value) = serde_json::from_str(val) {
            return Ok(json_value);
        }

        let trimmed = val.trim();

        match trimmed.to_lowercase().as_str() {
            "true" => return Ok(Value::Bool(true)),
            "false" => return Ok(Value::Bool(false)),
            _ => {}
        }

        if let Ok(int_val) = trimmed.parse::<i64>() {
            return Ok(Value::Number(int_val.into()));
        }

        if let Ok(float_val) = trimmed.parse::<f64>()
            && let Some(num) = serde_json::Number::from_f64(float_val)
        {
            return Ok(Value::Number(num));
        }

        Ok(Value::String(val.to_string()))
    }

    // check all possible places for a parameter
    pub fn get(&self, key: &str, is_secret: bool) -> Result<Value, ConfigError> {
        if is_secret {
            self.get_secret(key)
        } else {
            self.get_param(key)
        }
    }

    // save a parameter in the appropriate location based on if it's secret or not
    pub fn set<V>(&self, key: &str, value: &V, is_secret: bool) -> Result<(), ConfigError>
    where
        V: Serialize,
    {
        if is_secret {
            self.set_secret(key, value)
        } else {
            self.set_param(key, value)
        }
    }

    /// Get a configuration value (non-secret).
    ///
    /// This will attempt to get the value from (in order):
    /// 1. Environment variable with the uppercase key name
    /// 2. Merged config from all config paths (system → user → local)
    ///
    /// The value will be deserialized into the requested type. This works with
    /// both simple types (String, i32, etc.) and complex types that implement
    /// serde::Deserialize.
    ///
    /// # Errors
    ///
    /// Returns a ConfigError if:
    /// - The key doesn't exist in any of the above sources
    /// - The value cannot be deserialized into the requested type
    /// - There is an error reading the config file
    pub fn get_param<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<T, ConfigError> {
        let env_key = key.to_uppercase();
        if let Ok(val) = env::var(&env_key) {
            let value = Self::parse_env_value(&val)?;
            return Ok(serde_json::from_value(value)?);
        }

        let values = self.load()?;
        let value = values
            .get(key)
            .ok_or_else(|| ConfigError::NotFound(key.to_string()))?;

        match yaml_serde::from_value(value.clone()) {
            Ok(value) => Ok(value),
            Err(yaml_err) => {
                let Some(string_value) = value.as_str() else {
                    return Err(yaml_err.into());
                };
                let parsed = Self::parse_env_value(string_value)?;
                serde_json::from_value(parsed).map_err(|_| yaml_err.into())
            }
        }
    }

    pub(crate) fn get_param_source_values<T: for<'de> Deserialize<'de>>(
        &self,
        key: &str,
    ) -> Result<Vec<T>, ConfigError> {
        let mut source_values = Vec::new();
        let env_key = key.to_uppercase();
        if let Some(value) = env::var_os(&env_key) {
            let value = value.into_string().map_err(|_| {
                ConfigError::DeserializeError(format!(
                    "environment variable {env_key} is not valid UTF-8"
                ))
            })?;
            source_values.push(serde_json::from_value(Self::parse_env_value(&value)?)?);
        }

        for path in &self.config_paths {
            let content = match std::fs::read_to_string(path) {
                Ok(content) => content,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) => return Err(error.into()),
            };
            let mut values = parse_yaml_content(&content)?;
            crate::config::migrations::run_read_migrations(&mut values);
            let Some(value) = values.get(key) else {
                continue;
            };
            source_values.push(yaml_serde::from_value(value.clone())?);
        }

        Ok(source_values)
    }

    /// Read-modify-write a configuration value atomically through the write path.
    pub fn update_param<T, V, F>(&self, key: &str, f: F) -> Result<(), ConfigError>
    where
        T: for<'de> Deserialize<'de> + Default,
        V: Serialize,
        F: FnOnce(T) -> V,
    {
        let _guard = self.guard.lock().unwrap();
        let mut values = self.load_write_config()?;
        let current: T = values
            .get(key)
            .and_then(|v| yaml_serde::from_value(v.clone()).ok())
            .unwrap_or_default();
        let updated = f(current);
        values.insert(yaml_serde::to_value(key)?, yaml_serde::to_value(updated)?);
        self.save_values(&values)
    }

    /// Set a configuration value in the config file (non-secret).
    ///
    /// This will immediately write the value to the config file. The value
    /// can be any type that can be serialized to JSON/YAML.
    ///
    /// Note that this does not affect environment variables - those can only
    /// be set through the system environment.
    ///
    /// # Errors
    ///
    /// Returns a ConfigError if:
    /// - There is an error reading or writing the config file
    /// - There is an error serializing the value
    pub fn set_param<V: Serialize>(&self, key: &str, value: V) -> Result<(), ConfigError> {
        let _guard = self.guard.lock().unwrap();
        let mut values = self.load_write_config()?;
        values.insert(yaml_serde::to_value(key)?, yaml_serde::to_value(value)?);
        self.save_values(&values)
    }

    /// Set multiple configuration values in the config file with one read and one write.
    pub fn set_param_values(&self, updates: &[(String, Value)]) -> Result<(), ConfigError> {
        if updates.is_empty() {
            return Ok(());
        }

        let _guard = self.guard.lock().unwrap();
        let mut values = self.load_write_config()?;
        for (key, value) in updates {
            values.insert(yaml_serde::to_value(key)?, yaml_serde::to_value(value)?);
        }
        self.save_values(&values)
    }

    /// Delete a configuration value in the config file.
    ///
    /// This will immediately write the value to the config file. The value
    /// can be any type that can be serialized to JSON/YAML.
    ///
    /// Note that this does not affect environment variables - those can only
    /// be set through the system environment.
    ///
    /// # Errors
    ///
    /// Returns a ConfigError if:
    /// - There is an error reading or writing the config file
    /// - There is an error serializing the value
    pub fn delete(&self, key: &str) -> Result<(), ConfigError> {
        // Lock before reading to prevent race condition.
        let _guard = self.guard.lock().unwrap();

        let mut values = self.load_write_config()?;
        values.shift_remove(key);

        self.save_values(&values)
    }

    /// Get a secret value.
    ///
    /// This will attempt to get the value from:
    /// 1. Environment variable with the exact key name
    /// 2. System keyring
    ///
    /// The value will be deserialized into the requested type. This works with
    /// both simple types (String, i32, etc.) and complex types that implement
    /// serde::Deserialize.
    ///
    /// # Errors
    ///
    /// Returns a ConfigError if:
    /// - The key doesn't exist in either environment or keyring
    /// - The value cannot be deserialized into the requested type
    /// - There is an error accessing the keyring
    pub fn get_secret<T: for<'de> Deserialize<'de>>(&self, key: &str) -> Result<T, ConfigError> {
        // First check environment variables (convert to uppercase)
        let env_key = key.to_uppercase();
        if let Ok(val) = env::var(&env_key) {
            let value = Self::parse_env_value(&val)?;
            return Ok(serde_json::from_value(value)?);
        }

        // Then check keyring
        let values = self.all_secrets()?;
        values
            .get(key)
            .ok_or_else(|| ConfigError::NotFound(key.to_string()))
            .and_then(|v| Ok(serde_json::from_value(v.clone())?))
    }

    /// Get secrets. If primary is in env, use env for all keys. Otherwise, use secret storage.
    pub fn get_secrets(
        &self,
        primary: &str,
        maybe_secret: &[&str],
    ) -> Result<HashMap<String, String>, ConfigError> {
        let use_env = env::var(primary.to_uppercase()).is_ok();
        let get_value = |key: &str| -> Result<String, ConfigError> {
            if use_env {
                env::var(key.to_uppercase()).map_err(|_| ConfigError::NotFound(key.to_string()))
            } else {
                self.get_secret(key)
            }
        };

        let mut result = HashMap::new();
        result.insert(primary.to_string(), get_value(primary)?);
        for &key in maybe_secret {
            if let Ok(v) = get_value(key) {
                result.insert(key.to_string(), v);
            }
        }
        Ok(result)
    }

    fn load_secrets_from_storage(&self) -> Result<HashMap<String, Value>, ConfigError> {
        match &self.secrets {
            #[cfg(feature = "system-keyring")]
            SecretStorage::Keyring { service } => {
                let result =
                    self.handle_keyring_operation(|entry| entry.get_password(), service, None);

                match result {
                    Ok(content) => Ok(serde_json::from_str(&content)?),
                    Err(ConfigError::FallbackToFileStorage) => self.fallback_to_file_storage(),
                    Err(ConfigError::KeyringError(msg))
                        if msg.contains("No entry found")
                            || msg.contains("No matching entry found") =>
                    {
                        self.fallback_to_file_storage()
                    }
                    Err(e) => Err(e),
                }
            }
            SecretStorage::File { path } => self.read_secrets_from_file(path),
        }
    }

    fn secrets_mutation_lock_path(&self) -> Result<PathBuf, ConfigError> {
        let storage_path = match &self.secrets {
            #[cfg(feature = "system-keyring")]
            SecretStorage::Keyring { .. } => Self::secrets_file_path(),
            SecretStorage::File { path } => path.clone(),
        };
        Ok(secrets_lock_path(&private_file_target_path(&storage_path)?))
    }

    fn lock_secrets_for_mutation(&self) -> Result<std::fs::File, ConfigError> {
        let lock_path = self.secrets_mutation_lock_path()?;
        if let Some(parent) = lock_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| ConfigError::DirectoryError(e.to_string()))?;
        }

        let lock_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(lock_path)?;
        lock_file
            .lock_exclusive()
            .map_err(|e| ConfigError::LockError(e.to_string()))?;
        Ok(lock_file)
    }

    fn write_all_secrets(&self, values: &HashMap<String, Value>) -> Result<(), ConfigError> {
        match &self.secrets {
            #[cfg(feature = "system-keyring")]
            SecretStorage::Keyring { service } => {
                let json_value = serde_json::to_string(values)?;
                match self.handle_keyring_operation(
                    |entry| entry.set_password(&json_value),
                    service,
                    Some(values),
                ) {
                    Ok(_) => {}
                    Err(ConfigError::FallbackToFileStorage) => {}
                    Err(e) => return Err(e),
                }
            }
            SecretStorage::File { path } => {
                let yaml_value = yaml_serde::to_string(values)?;
                write_secrets_file(path, &yaml_value)?;
            }
        }

        self.invalidate_secrets_cache();
        Ok(())
    }

    fn mutate_secrets<T>(
        &self,
        mutate: impl FnOnce(&mut HashMap<String, Value>) -> Result<SecretMutation<T>, ConfigError>,
    ) -> Result<T, ConfigError> {
        let _guard = self.guard.lock().unwrap();
        let _storage_lock = self.lock_secrets_for_mutation()?;
        let mut values = self.load_secrets_from_storage()?;
        match mutate(&mut values)? {
            SecretMutation::Write(result) => {
                self.write_all_secrets(&values)?;
                Ok(result)
            }
            SecretMutation::Unchanged(result) => Ok(result),
        }
    }

    /// Set a secret value in the system keyring.
    ///
    /// This will store the value in a single JSON object in the system keyring,
    /// alongside any other secrets. The value can be any type that can be
    /// serialized to JSON.
    ///
    /// Note that this does not affect environment variables - those can only
    /// be set through the system environment.
    ///
    /// # Errors
    ///
    /// Returns a ConfigError if:
    /// - There is an error accessing the keyring
    /// - There is an error serializing the value
    pub fn set_secret<V>(&self, key: &str, value: &V) -> Result<(), ConfigError>
    where
        V: Serialize,
    {
        let value = serde_json::to_value(value)?;
        self.mutate_secrets(|values| {
            values.insert(key.to_string(), value);
            Ok(SecretMutation::Write(()))
        })
    }

    pub(crate) fn update_secret<T, V, R>(
        &self,
        key: &str,
        update: impl FnOnce(T) -> SecretUpdate<V, R>,
    ) -> Result<R, ConfigError>
    where
        T: for<'de> Deserialize<'de> + Default,
        V: Serialize,
    {
        self.mutate_secrets(|values| {
            let current = values
                .get(key)
                .cloned()
                .map(serde_json::from_value)
                .transpose()?
                .unwrap_or_default();
            match update(current) {
                SecretUpdate::Write(updated, result) => {
                    values.insert(key.to_string(), serde_json::to_value(updated)?);
                    Ok(SecretMutation::Write(result))
                }
                SecretUpdate::Unchanged(result) => Ok(SecretMutation::Unchanged(result)),
            }
        })
    }

    /// Set multiple secret values with one storage read and one storage write.
    ///
    /// This is intended for provider setup flows that save several fields at once.
    /// It keeps keychain access batched while preserving the same storage format as
    /// `set_secret`.
    pub fn set_secret_values(&self, updates: &[(String, Value)]) -> Result<(), ConfigError> {
        if updates.is_empty() {
            return Ok(());
        }

        self.mutate_secrets(|values| {
            for (key, value) in updates {
                values.insert(key.clone(), value.clone());
            }
            Ok(SecretMutation::Write(()))
        })
    }

    /// Delete a secret from the system keyring.
    ///
    /// This will remove the specified key from the JSON object in the system keyring.
    /// Other secrets will remain unchanged.
    ///
    /// # Errors
    ///
    /// Returns a ConfigError if:
    /// - There is an error accessing the keyring
    /// - There is an error serializing the remaining values
    pub fn delete_secret(&self, key: &str) -> Result<(), ConfigError> {
        self.mutate_secrets(|values| {
            values.remove(key);
            Ok(SecretMutation::Write(()))
        })
    }

    /// Delete multiple secret values with one storage read and one storage write.
    pub fn delete_secret_values(&self, keys: &[String]) -> Result<(), ConfigError> {
        if keys.is_empty() {
            return Ok(());
        }

        self.mutate_secrets(|values| {
            for key in keys {
                values.remove(key);
            }
            Ok(SecretMutation::Write(()))
        })
    }

    /// Read secrets from a YAML file
    fn read_secrets_from_file(&self, path: &Path) -> Result<HashMap<String, Value>, ConfigError> {
        if path.exists() {
            let file_content = std::fs::read_to_string(path)?;
            let yaml_value: yaml_serde::Value = yaml_serde::from_str(&file_content)?;
            let json_value: Value = serde_json::to_value(yaml_value)?;
            match json_value {
                Value::Object(map) => Ok(map.into_iter().collect()),
                _ => Ok(HashMap::new()),
            }
        } else {
            Ok(HashMap::new())
        }
    }

    /// Get the path to the secrets storage file
    #[cfg(feature = "system-keyring")]
    fn secrets_file_path() -> PathBuf {
        secrets_file_path_in(&Paths::config_dir())
    }

    /// Fall back to file storage when keyring is unavailable
    #[cfg(feature = "system-keyring")]
    fn fallback_to_file_storage(&self) -> Result<HashMap<String, Value>, ConfigError> {
        let path = Self::secrets_file_path();
        self.read_secrets_from_file(&path)
    }

    /// Write secrets to file storage (used for fallback)
    #[cfg(feature = "system-keyring")]
    fn write_secrets_to_file(&self, values: &HashMap<String, Value>) -> Result<(), ConfigError> {
        std::fs::create_dir_all(Paths::config_dir())?;
        let path = Self::secrets_file_path();
        let yaml_value = yaml_serde::to_string(values)?;
        write_secrets_file(&path, &yaml_value)?;
        Ok(())
    }

    pub fn invalidate_secrets_cache(&self) {
        let mut cache = self.secrets_cache.lock().unwrap();
        *cache = None;
    }

    /// Check if an error string indicates a keyring availability issue that should trigger fallback
    #[cfg(feature = "system-keyring")]
    fn is_keyring_availability_error(&self, error_str: &str) -> bool {
        let lower = error_str.to_lowercase();
        lower.contains("keyring")
            || lower.contains("dbus")
            || lower.contains("org.freedesktop.secrets")
            || lower.contains("platform secure storage")
            || lower.contains("no secret service")
    }

    /// Get a keyring entry for the specified service
    #[cfg(feature = "system-keyring")]
    fn get_keyring_entry(service: &str) -> Result<keyring::Entry, keyring::Error> {
        Entry::new(service, KEYRING_USERNAME)
    }

    /// Handle keyring errors with automatic fallback to file storage
    #[cfg(feature = "system-keyring")]
    fn handle_keyring_fallback_error<T>(
        &self,
        keyring_err: &keyring::Error,
        fallback_values: Option<&HashMap<String, Value>>,
    ) -> Result<T, ConfigError> {
        if self.is_keyring_availability_error(&keyring_err.to_string()) {
            KEYRING_UNAVAILABLE.store(true, Ordering::Relaxed);
            tracing::warn!("Keyring unavailable. Using file storage for secrets.");

            if let Some(values) = fallback_values {
                self.write_secrets_to_file(values)?;
                Err(ConfigError::FallbackToFileStorage)
            } else {
                Err(ConfigError::FallbackToFileStorage)
            }
        } else {
            Err(ConfigError::KeyringError(keyring_err.to_string()))
        }
    }

    /// Handle keyring operation with automatic fallback to file storage
    #[cfg(feature = "system-keyring")]
    fn handle_keyring_operation<T>(
        &self,
        operation: impl FnOnce(keyring::Entry) -> Result<T, keyring::Error>,
        service: &str,
        fallback_values: Option<&HashMap<String, Value>>,
    ) -> Result<T, ConfigError> {
        // Try to get the keyring entry and perform the operation
        let entry = match Self::get_keyring_entry(service) {
            Ok(entry) => entry,
            Err(keyring_err) => {
                return self.handle_keyring_fallback_error(&keyring_err, fallback_values);
            }
        };

        // Perform the operation
        match operation(entry) {
            Ok(result) => Ok(result),
            Err(keyring_err) => self.handle_keyring_fallback_error(&keyring_err, fallback_values),
        }
    }
}

config_value!(CLAUDE_CODE_COMMAND, String, "claude");
config_value!(GEMINI_CLI_COMMAND, String, "gemini");
config_value!(CURSOR_AGENT_COMMAND, String, "cursor-agent");
config_value!(CODEX_COMMAND, String, "codex");
config_value!(CODEX_ENABLE_SKILLS, String, "true");
config_value!(CODEX_SKIP_GIT_CHECK, String, "false");
config_value!(CHATGPT_CODEX_REASONING_EFFORT, String, "medium");

config_value!(GOOSE_SEARCH_PATHS, Vec<String>);
config_value!(GOOSE_MODE, GooseMode);
impl Config {
    pub(crate) fn get_goose_mode_strict(&self) -> Result<GooseMode, ConfigError> {
        match env::var("GOOSE_MODE") {
            Ok(value) => {
                let value = Self::parse_env_value(&value)?;
                Ok(serde_json::from_value(value)?)
            }
            Err(env::VarError::NotPresent) => {
                let values = self.load_strict()?;
                let value = values
                    .get("GOOSE_MODE")
                    .ok_or_else(|| ConfigError::NotFound("GOOSE_MODE".to_string()))?;
                Ok(yaml_serde::from_value(value.clone())?)
            }
            Err(env::VarError::NotUnicode(_)) => Err(ConfigError::DeserializeError(
                "GOOSE_MODE contains non-Unicode data".to_string(),
            )),
        }
    }
}
// GOOSE_PROVIDER and GOOSE_MODEL are handled by crate::config::providers
// which checks the structured `providers:` block first and falls back to
// the legacy flat keys. The accessors below delegate to that module.
impl Config {
    pub fn get_goose_provider(&self) -> Result<String, ConfigError> {
        crate::config::providers::get_active_provider(self)
            .ok_or_else(|| ConfigError::NotFound("GOOSE_PROVIDER".to_string()))
    }
    pub fn set_goose_provider(&self, v: impl Into<String>) -> Result<(), ConfigError> {
        let name = v.into();
        let model = crate::config::providers::get_provider_entry(self, &name)
            .map(|e| e.model)
            .unwrap_or_default();
        crate::config::providers::set_active_provider(self, &name, &model)
    }
    pub fn get_goose_model(&self) -> Result<String, ConfigError> {
        crate::config::providers::get_active_model(self)
            .ok_or_else(|| ConfigError::NotFound("GOOSE_MODEL".to_string()))
    }
    pub fn set_goose_model(&self, v: impl Into<String>) -> Result<(), ConfigError> {
        let model = v.into();
        if let Some(provider) = crate::config::providers::get_active_provider(self) {
            crate::config::providers::set_active_provider(self, &provider, &model)?;
        }
        Ok(())
    }
}
config_value!(GOOSE_PROMPT_EDITOR, Option<String>);
config_value!(GOOSE_PROMPT_EDITOR_ALWAYS, Option<bool>);
config_value!(GOOSE_MAX_ACTIVE_AGENTS, usize);
config_value!(GOOSE_DISABLE_SESSION_NAMING, bool);

impl Config {
    pub fn get_goose_context_limit(&self) -> Result<Option<usize>, ConfigError> {
        match self.get_param::<usize>("GOOSE_CONTEXT_LIMIT") {
            Ok(0) => Err(ConfigError::DeserializeError(
                "GOOSE_CONTEXT_LIMIT must be greater than 0".to_string(),
            )),
            Ok(limit) => Ok(Some(limit)),
            Err(ConfigError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn get_goose_max_tokens(&self) -> Result<Option<i32>, ConfigError> {
        match self.get_param::<i32>("GOOSE_MAX_TOKENS") {
            Ok(tokens) if tokens <= 0 => Err(ConfigError::DeserializeError(
                "GOOSE_MAX_TOKENS must be greater than 0".to_string(),
            )),
            Ok(tokens) => Ok(Some(tokens)),
            Err(ConfigError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn get_goose_docs_root(&self) -> Result<Option<String>, ConfigError> {
        match self.get_param::<String>("GOOSE_DOCS_ROOT") {
            Ok(root) => Ok(Some(root.trim().to_string()).filter(|root| !root.is_empty())),
            Err(ConfigError::NotFound(_)) => Ok(None),
            Err(e) => Err(e),
        }
    }

    pub fn get_goose_thinking_effort(&self) -> Option<ThinkingEffort> {
        self.get_param::<String>("GOOSE_THINKING_EFFORT")
            .ok()
            .and_then(|e| e.parse().ok())
            .or_else(|| self.legacy_thinking_effort())
    }

    pub fn set_goose_thinking_effort(&self, v: ThinkingEffort) -> Result<(), ConfigError> {
        self.set_param("GOOSE_THINKING_EFFORT", v)
    }

    pub fn get_openai_store(&self) -> Option<bool> {
        self.get_param::<bool>("OPENAI_STORE").ok()
    }

    fn legacy_thinking_effort(&self) -> Option<ThinkingEffort> {
        if let Ok(value) = self.get_param::<String>("CLAUDE_THINKING_TYPE")
            && let Some(effort) = match value.to_lowercase().as_str() {
                "adaptive" | "enabled" => Some(ThinkingEffort::High),
                "disabled" => Some(ThinkingEffort::Off),
                _ => None,
            }
        {
            return Some(effort);
        }

        if let Ok(enabled) = self.get_param::<bool>("CLAUDE_THINKING_ENABLED") {
            return Some(if enabled {
                ThinkingEffort::High
            } else {
                ThinkingEffort::Off
            });
        }

        if let Ok(value) = self.get_param::<String>("GEMINI3_THINKING_LEVEL")
            && let Some(effort) = Self::legacy_gemini3_thinking_effort(&value)
        {
            return Some(effort);
        }

        None
    }

    fn legacy_gemini3_thinking_effort(value: &str) -> Option<ThinkingEffort> {
        match value.to_lowercase().as_str() {
            "low" => Some(ThinkingEffort::Low),
            "high" => Some(ThinkingEffort::High),
            _ => None,
        }
    }
}

config_value!(GOOSE_DEFAULT_EXTENSION_TIMEOUT, u64);

fn find_workspace_or_exe_root() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe_dir = exe.parent()?.to_path_buf();

    let mut path = exe;
    while let Some(parent) = path.parent() {
        let cargo_toml = parent.join("Cargo.toml");
        if cargo_toml.exists()
            && let Ok(content) = std::fs::read_to_string(&cargo_toml)
            && content.contains("[workspace]")
        {
            return Some(parent.to_path_buf());
        }
        path = parent.to_path_buf();
    }

    Some(exe_dir)
}

pub fn load_init_config_from_workspace() -> Result<Mapping, ConfigError> {
    let root = find_workspace_or_exe_root().ok_or_else(|| {
        ConfigError::FileError(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "Could not determine executable path",
        ))
    })?;

    let init_config_path = root.join("init-config.yaml");
    if !init_config_path.exists() {
        return Err(ConfigError::NotFound(
            "init-config.yaml not found".to_string(),
        ));
    }

    let init_content = std::fs::read_to_string(&init_config_path)?;
    parse_yaml_content(&init_content)
}

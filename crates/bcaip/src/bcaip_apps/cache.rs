use super::app::BcaipApp;
use crate::{config::paths::Paths, utils::bytes_to_hex};
use sha2::{Digest, Sha256};
use std::{fs, path::PathBuf};
use tracing::warn;
static CLOCK_HTML: &str = include_str!("../bcaip_apps/clock.html");
const APPS_EXTENSION_NAME: &str = "apps";

pub const BUNDLED_DEFAULT_APP_URIS: &[&str] = &["ui://apps/clock"];

/// Bundled default apps: (cache URI, HTML source).
const DEFAULT_APPS: &[(&str, &str)] = &[("ui://apps/clock", CLOCK_HTML)];

pub fn mark_deletable_apps(apps: &mut [BcaipApp]) {
    for app in apps.iter_mut() {
        let is_apps_extension = app
            .mcp_servers
            .iter()
            .any(|server| server == APPS_EXTENSION_NAME);
        app.deletable =
            is_apps_extension && !McpAppCache::is_bundled_default_uri(&app.resource.uri);
    }
}

pub struct McpAppCache {
    cache_dir: PathBuf,
}

impl McpAppCache {
    pub fn new() -> Result<Self, std::io::Error> {
        let config_dir = Paths::config_dir();
        let cache_dir = config_dir.join("mcp-apps-cache");
        let cache = Self { cache_dir };
        cache.ensure_default_apps();
        Ok(cache)
    }

    fn ensure_default_apps(&self) {
        if fs::create_dir_all(&self.cache_dir).is_err() {
            return;
        }

        for (uri, _) in DEFAULT_APPS {
            let Some(app) = Self::bundled_default_app(uri) else {
                continue;
            };
            let Ok(json) = serde_json::to_string_pretty(&app) else {
                continue;
            };
            let _ = fs::write(self.app_path(APPS_EXTENSION_NAME, uri), json);
        }
    }

    pub fn is_bundled_default_uri(uri: &str) -> bool {
        BUNDLED_DEFAULT_APP_URIS.contains(&uri)
    }

    fn cache_key(extension_name: &str, resource_uri: &str) -> String {
        let input = format!("{}::{}", extension_name, resource_uri);
        let hash = bytes_to_hex(Sha256::digest(input.as_bytes()));
        format!("{}_{}", extension_name, hash)
    }

    fn app_path(&self, extension_name: &str, resource_uri: &str) -> PathBuf {
        self.cache_dir.join(format!(
            "{}.json",
            Self::cache_key(extension_name, resource_uri)
        ))
    }

    fn is_bundled_default_identity(extension_name: &str, resource_uri: &str) -> bool {
        extension_name == APPS_EXTENSION_NAME && Self::is_bundled_default_uri(resource_uri)
    }

    fn bundled_default_app(resource_uri: &str) -> Option<BcaipApp> {
        let (_, html) = DEFAULT_APPS.iter().find(|(uri, _)| *uri == resource_uri)?;
        let mut app = BcaipApp::from_html(html).ok()?;
        app.mcp_servers = vec![APPS_EXTENSION_NAME.to_string()];
        Some(app)
    }

    pub fn restore_bundled_default_apps(apps: &mut [BcaipApp]) {
        for app in apps {
            let is_bundled_identity = app.mcp_servers.iter().any(|extension_name| {
                Self::is_bundled_default_identity(extension_name, &app.resource.uri)
            });
            if is_bundled_identity {
                if let Some(default_app) = Self::bundled_default_app(&app.resource.uri) {
                    *app = default_app;
                }
            }
        }
    }

    pub fn list_apps(&self) -> Result<Vec<BcaipApp>, std::io::Error> {
        let mut apps = Vec::new();

        if self.cache_dir.exists() {
            for entry in fs::read_dir(&self.cache_dir)? {
                let entry = entry?;
                let path = entry.path();

                if let Some(app) = DEFAULT_APPS.iter().find_map(|(uri, _)| {
                    (path == self.app_path(APPS_EXTENSION_NAME, uri))
                        .then(|| Self::bundled_default_app(uri))
                        .flatten()
                }) {
                    apps.push(app);
                    continue;
                }

                if path.extension().and_then(|s| s.to_str()) == Some("json") {
                    match fs::read_to_string(&path) {
                        Ok(content) => match serde_json::from_str::<BcaipApp>(&content) {
                            Ok(app) => apps.push(app),
                            Err(e) => warn!("Failed to parse cached app from {:?}: {}", path, e),
                        },
                        Err(e) => warn!("Failed to read cached app from {:?}: {}", path, e),
                    }
                }
            }
        }

        Self::restore_bundled_default_apps(&mut apps);
        for (uri, _) in DEFAULT_APPS {
            let contains_default = apps.iter().any(|app| {
                app.mcp_servers.iter().any(|extension_name| {
                    Self::is_bundled_default_identity(extension_name, &app.resource.uri)
                        && app.resource.uri == *uri
                })
            });
            if !contains_default {
                if let Some(app) = Self::bundled_default_app(uri) {
                    apps.push(app);
                }
            }
        }
        Ok(apps)
    }

    pub fn store_app(&self, app: &BcaipApp) -> Result<(), std::io::Error> {
        fs::create_dir_all(&self.cache_dir)?;

        // Store the app once for each MCP server it's associated with
        for extension_name in &app.mcp_servers {
            if Self::is_bundled_default_identity(extension_name, &app.resource.uri) {
                continue;
            }
            let cache_key = Self::cache_key(extension_name, &app.resource.uri);
            let app_path = self.cache_dir.join(format!("{}.json", cache_key));
            let json = serde_json::to_string_pretty(app).map_err(std::io::Error::other)?;
            fs::write(app_path, json)?;
        }

        Ok(())
    }

    pub fn get_app(&self, extension_name: &str, resource_uri: &str) -> Option<BcaipApp> {
        if Self::is_bundled_default_identity(extension_name, resource_uri) {
            return Self::bundled_default_app(resource_uri);
        }

        let cache_key = Self::cache_key(extension_name, resource_uri);
        let app_path = self.cache_dir.join(format!("{}.json", cache_key));

        if !app_path.exists() {
            return None;
        }

        fs::read_to_string(&app_path)
            .ok()
            .and_then(|content| serde_json::from_str::<BcaipApp>(&content).ok())
    }

    pub fn delete_app(
        &self,
        extension_name: &str,
        resource_uri: &str,
    ) -> Result<(), std::io::Error> {
        if Self::is_bundled_default_identity(extension_name, resource_uri) {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "Cannot delete bundled default app",
            ));
        }

        let cache_key = Self::cache_key(extension_name, resource_uri);
        let app_path = self.cache_dir.join(format!("{}.json", cache_key));

        if !app_path.exists() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!(
                    "App not found in cache: {}::{}",
                    extension_name, resource_uri
                ),
            ));
        }

        fs::remove_file(app_path)
    }

    pub fn delete_extension_apps(&self, extension_name: &str) -> Result<usize, std::io::Error> {
        let mut deleted_count = 0;

        if !self.cache_dir.exists() {
            return Ok(0);
        }

        for entry in fs::read_dir(&self.cache_dir)? {
            let entry = entry?;
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("json") {
                if let Ok(content) = fs::read_to_string(&path) {
                    if let Ok(app) = serde_json::from_str::<BcaipApp>(&content) {
                        if app.mcp_servers.contains(&extension_name.to_string())
                            && !Self::is_bundled_default_identity(extension_name, &app.resource.uri)
                            && fs::remove_file(&path).is_ok()
                        {
                            deleted_count += 1;
                        }
                    }
                }
            }
        }

        Ok(deleted_count)
    }
}

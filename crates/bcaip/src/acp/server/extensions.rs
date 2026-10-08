use crate::acp::server::server_informations::{BcaipAcpAgent, ResultExt};
use crate::{
    agents::{ExtensionConfig, extension::Envs},
    config::{Config, extensions::ExtensionEntry},
    session::EnabledExtensionsState,
};
use agent_client_protocol::schema::v1::{HttpHeader, McpServer, McpServerHttp, McpServerStdio};
use bcaip_sdk_types::custom_requests::{
    AddConfigExtensionRequest, AddSessionExtensionRequest, BcaipExtension, BcaipExtensionEntry,
    EmptyResponse, GetConfigExtensionsResponse, GetSessionExtensionsRequest,
    GetSessionExtensionsResponse, RemoveConfigExtensionRequest, RemoveSessionExtensionRequest,
    SessionExtensionEntry, SetConfigExtensionEnabledRequest,
};
use std::collections::HashSet;
impl BcaipAcpAgent {
    pub(crate) async fn on_add_session_extension(
        &self,
        req: AddSessionExtensionRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        let session_id = &req.session_id;
        let config = bcaip_extension_to_config_without_secrets(req.extension)?;
        let agent = self.get_session_agent(&req.session_id).await?;
        agent
            .add_extension(config, session_id)
            .await
            .internal_err()?;
        Ok(EmptyResponse {})
    }

    pub(crate) async fn on_remove_session_extension(
        &self,
        req: RemoveSessionExtensionRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        let session_id = &req.session_id;
        let agent = self.get_session_agent(&req.session_id).await?;
        let removed = agent
            .remove_extension_by_key(&req.extension_key, session_id)
            .await
            .internal_err()?;
        if !removed {
            return Err(agent_client_protocol::Error::invalid_params()
                .data(format!("Extension '{}' not found", req.extension_key)));
        }
        Ok(EmptyResponse {})
    }

    pub(crate) async fn on_get_config_extensions(
        &self,
    ) -> Result<GetConfigExtensionsResponse, agent_client_protocol::Error> {
        let extensions = crate::config::extensions::get_all_extensions()
            .into_iter()
            .filter(|ext| {
                !crate::agents::extension_manager::is_hidden_extension(&ext.config.name())
            })
            .collect::<Vec<_>>();
        let warnings = crate::config::extensions::get_warnings();
        let extensions = extensions
            .into_iter()
            .map(config_entry_to_bcaip_entry)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        Ok(GetConfigExtensionsResponse {
            extensions,
            warnings,
        })
    }

    pub(crate) async fn on_add_config_extension(
        &self,
        req: AddConfigExtensionRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        let conversion = bcaip_extension_to_config(req.extension)?;

        Config::global()
            .set_secret_values(&conversion.secret_updates)
            .internal_err_ctx("Failed to save extension env secrets")?;

        crate::config::extensions::set_extension(ExtensionEntry {
            enabled: req.enabled,
            config: conversion.config,
        });
        Ok(EmptyResponse {})
    }

    pub(crate) async fn on_remove_config_extension(
        &self,
        req: RemoveConfigExtensionRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        crate::config::extensions::remove_extension(&req.config_key);
        Ok(EmptyResponse {})
    }

    pub(crate) async fn on_set_config_extension_enabled(
        &self,
        req: SetConfigExtensionEnabledRequest,
    ) -> Result<EmptyResponse, agent_client_protocol::Error> {
        let updated =
            crate::config::extensions::set_extension_enabled(&req.config_key, req.enabled);
        if !updated {
            return Err(agent_client_protocol::Error::invalid_params()
                .data(format!("Extension '{}' not found", req.config_key)));
        }

        Ok(EmptyResponse {})
    }

    pub(crate) async fn on_get_session_extensions(
        &self,
        req: GetSessionExtensionsRequest,
    ) -> Result<GetSessionExtensionsResponse, agent_client_protocol::Error> {
        let session_id = &req.session_id;
        let session = self
            .session_manager()
            .get_session(session_id, false)
            .await
            .internal_err()?;

        let extensions = EnabledExtensionsState::extensions_or_default(
            Some(&session.extension_data),
            crate::config::Config::global(),
        );

        Ok(GetSessionExtensionsResponse {
            extensions: session_configs_to_entries(extensions)?,
        })
    }
}

fn session_configs_to_entries(
    configs: Vec<ExtensionConfig>,
) -> Result<Vec<SessionExtensionEntry>, agent_client_protocol::Error> {
    let mut extension_keys = HashSet::with_capacity(configs.len());
    let mut entries = Vec::with_capacity(configs.len());
    for config in configs {
        let extension_key = config.key();
        if !extension_keys.insert(extension_key.clone()) {
            return Err(agent_client_protocol::Error::internal_error()
                .data(format!("Duplicate session extension key '{extension_key}'")));
        }
        if let Some(extension) = config_to_bcaip_extension(&config)? {
            entries.push(SessionExtensionEntry {
                extension,
                extension_key,
            });
        }
    }
    Ok(entries)
}

fn config_to_bcaip_extension(
    config: &ExtensionConfig,
) -> Result<Option<BcaipExtension>, agent_client_protocol::Error> {
    let extension = match config {
        ExtensionConfig::Builtin {
            name,
            description,
            display_name,
            timeout,
            bundled,
            available_tools,
        } => BcaipExtension::Builtin {
            name: name.clone(),
            description: empty_string_to_none(description),
            display_name: display_name.clone(),
            timeout: *timeout,
            bundled: *bundled,
            available_tools: available_tools_to_wire(available_tools),
        },
        ExtensionConfig::Platform {
            name,
            description,
            display_name,
            bundled,
            available_tools,
        } => BcaipExtension::Platform {
            name: name.clone(),
            description: empty_string_to_none(description),
            display_name: display_name.clone(),
            bundled: *bundled,
            available_tools: available_tools_to_wire(available_tools),
        },
        ExtensionConfig::Stdio {
            name,
            description,
            cmd,
            args,
            env_keys,
            timeout,
            bundled,
            available_tools,
            ..
        } => BcaipExtension::Mcp {
            server: Box::new(McpServer::Stdio(
                McpServerStdio::new(name, cmd).args(args.clone()),
            )),
            env_keys: env_keys.clone(),
            description: empty_string_to_none(description),
            timeout: *timeout,
            socket: None,
            client_id: None,
            client_secret_key: None,
            scopes: vec![],
            bundled: *bundled,
            available_tools: available_tools_to_wire(available_tools),
        },
        ExtensionConfig::StreamableHttp {
            name,
            description,
            uri,
            env_keys,
            headers,
            timeout,
            socket,
            client_id,
            client_secret_key,
            scopes,
            bundled,
            available_tools,
            ..
        } => {
            let headers = headers
                .iter()
                .map(|(key, value)| HttpHeader::new(key, value))
                .collect();
            BcaipExtension::Mcp {
                server: Box::new(McpServer::Http(
                    McpServerHttp::new(name, uri).headers(headers),
                )),
                env_keys: env_keys.clone(),
                description: empty_string_to_none(description),
                timeout: *timeout,
                socket: socket.clone(),
                client_id: client_id.clone(),
                client_secret_key: client_secret_key.clone(),
                scopes: scopes.clone(),
                bundled: *bundled,
                available_tools: available_tools_to_wire(available_tools),
            }
        }
    };
    Ok(Some(extension))
}

struct ConfigExtensionConversion {
    config: ExtensionConfig,
    secret_updates: Vec<(String, serde_json::Value)>,
}

fn bcaip_extension_to_config(
    extension: BcaipExtension,
) -> Result<ConfigExtensionConversion, agent_client_protocol::Error> {
    let mut secret_updates = Vec::new();
    let config = match extension {
        BcaipExtension::Builtin {
            name,
            description,
            display_name,
            timeout,
            bundled,
            available_tools,
        } => ExtensionConfig::Builtin {
            name,
            description: description.unwrap_or_default(),
            display_name,
            timeout,
            bundled,
            available_tools: available_tools.unwrap_or_default(),
        },
        BcaipExtension::Platform {
            name,
            description,
            display_name,
            bundled,
            available_tools,
        } => ExtensionConfig::Platform {
            name,
            description: description.unwrap_or_default(),
            display_name,
            bundled,
            available_tools: available_tools.unwrap_or_default(),
        },
        BcaipExtension::Mcp {
            server,
            env_keys,
            description,
            timeout,
            socket,
            client_id,
            client_secret_key,
            scopes,
            bundled,
            available_tools,
        } => match *server {
            McpServer::Stdio(stdio) => {
                if socket.is_some() {
                    return Err(agent_client_protocol::Error::invalid_params()
                        .data("socket is only supported for streamable_http MCP extensions"));
                }
                if client_id.is_some() || client_secret_key.is_some() || !scopes.is_empty() {
                    return Err(agent_client_protocol::Error::invalid_params().data(
                        "OAuth client fields are only supported for streamable_http MCP extensions",
                    ));
                }
                let mut env_keys = env_keys;
                for env in stdio.env {
                    if !env_keys.contains(&env.name) {
                        env_keys.push(env.name.clone());
                    }
                    secret_updates.push((env.name, serde_json::Value::String(env.value)));
                }
                ExtensionConfig::Stdio {
                    name: stdio.name,
                    description: description.unwrap_or_default(),
                    cmd: stdio.command.to_string_lossy().to_string(),
                    args: stdio.args,
                    envs: Envs::default(),
                    env_keys,
                    timeout,
                    cwd: None,
                    bundled,
                    available_tools: available_tools.unwrap_or_default(),
                }
            }
            McpServer::Http(http) => ExtensionConfig::StreamableHttp {
                name: http.name,
                description: description.unwrap_or_default(),
                uri: http.url,
                envs: Envs::default(),
                env_keys,
                headers: http
                    .headers
                    .into_iter()
                    .map(|header| (header.name, header.value))
                    .collect(),
                timeout,
                socket,
                client_id,
                client_secret_key,
                scopes,
                bundled,
                available_tools: available_tools.unwrap_or_default(),
            },
            McpServer::Sse(_) => {
                return Err(agent_client_protocol::Error::invalid_params()
                    .data("SSE is unsupported, migrate to streamable_http"));
            }
            _ => {
                return Err(
                    agent_client_protocol::Error::invalid_params().data("unsupported MCP server")
                );
            }
        },
    };
    Ok(ConfigExtensionConversion {
        config,
        secret_updates,
    })
}

fn bcaip_extension_to_config_without_secrets(
    extension: BcaipExtension,
) -> Result<ExtensionConfig, agent_client_protocol::Error> {
    let conversion = bcaip_extension_to_config(extension)?;
    if !conversion.secret_updates.is_empty() {
        return Err(agent_client_protocol::Error::invalid_params().data(
            "extension env values must be passed via envKeys referencing stored secrets, not inline env",
        ));
    }
    Ok(conversion.config)
}

pub(super) fn bcaip_extensions_to_configs(
    extensions: Vec<BcaipExtension>,
) -> Result<Vec<ExtensionConfig>, agent_client_protocol::Error> {
    extensions
        .into_iter()
        .map(bcaip_extension_to_config_without_secrets)
        .collect()
}

fn config_entry_to_bcaip_entry(
    entry: ExtensionEntry,
) -> Result<Option<BcaipExtensionEntry>, agent_client_protocol::Error> {
    let config_key = entry.config.key();
    let Some(extension) = config_to_bcaip_extension(&entry.config)? else {
        return Ok(None);
    };
    Ok(Some(BcaipExtensionEntry {
        extension,
        enabled: entry.enabled,
        config_key: Some(config_key),
    }))
}

fn empty_string_to_none(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn available_tools_to_wire(available_tools: &[String]) -> Option<Vec<String>> {
    if available_tools.is_empty() {
        None
    } else {
        Some(available_tools.to_vec())
    }
}

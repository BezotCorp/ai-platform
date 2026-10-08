//mod.rs need to have only module declarations and public exports. So review and extract
mod common;
pub(crate) mod fs;
mod handoff;
#[cfg(feature = "acp-http")]
mod mcp_app_proxy;
mod provider;
mod response_builder;
pub mod server;
pub mod server_factory;
pub(crate) mod tool_call_notifier;
pub(crate) mod tools;
#[cfg(feature = "acp-http")]
pub mod transport;

pub use common::{PermissionDecision, map_permission_response};
pub use provider::{
    ACP_CURRENT_MODEL, AcpProvider, AcpProviderConfig, extension_configs_to_mcp_servers,
};
pub use server::*;

/// `data.reason` on a prompt error raised because the agent's account is out of credits.
/// Set by the ACP server, read by the provider to tell a spent account apart from a
/// prompt the agent could not accept.
pub(crate) const CREDITS_EXHAUSTED_REASON: &str = "credits_exhausted";

pub(crate) fn configured_model_for_provider(
    config: &crate::config::Config,
    provider_name: &str,
) -> String {
    if config.get_bcaip_provider().ok().as_deref() == Some(provider_name) {
        config
            .get_bcaip_model()
            .unwrap_or_else(|_| ACP_CURRENT_MODEL.to_string())
    } else {
        ACP_CURRENT_MODEL.to_string()
    }
}

pub(crate) fn is_auth_required(error: &anyhow::Error) -> bool {
    error.chain().any(|source| {
        source
            .downcast_ref::<agent_client_protocol::Error>()
            .is_some_and(|error| {
                error.code == agent_client_protocol::schema::v1::ErrorCode::AuthRequired
            })
    })
}

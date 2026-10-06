mod acp_tooling;
pub mod amp_acp;
pub mod anthropic_def;
pub mod avian;
pub mod azure;
pub mod azure_foundry_def;
pub mod azureauth;
pub mod base;
#[cfg(feature = "aws-providers")]
pub mod bedrock;
pub mod canonical_cost;
mod catalog_util;
pub mod catalog {
    pub use super::catalog_util::*;
}
pub mod chatgpt_codex;
pub mod claude_acp;
pub mod claude_code;
pub(crate) mod cli_common;
pub mod codex;
pub mod codex_acp;
pub mod command_auth;
pub mod copilot_acp;
pub mod cursor_agent;
pub mod custom_provider_config;
pub mod databricks_def;
pub mod databricks_v2_def;
pub mod formats;
mod gcpauth;
pub mod gcpvertexai;
pub mod gemini_cli;
pub mod gemini_oauth;
pub mod githubcopilot;
pub mod gondola;
pub mod google_def;
pub mod huggingface;
pub mod huggingface_auth;
mod init;
pub mod inventory;
pub mod kimicode;
pub mod litellm;
#[cfg(feature = "local-inference")]
pub mod local_inference;
pub mod muse_code_def;
pub mod nanogpt;
pub mod oauth;
pub mod oauth_device_flow;
pub mod ollama_cloud;
pub mod ollama_def;
pub mod openrouter_def;
pub mod pi_acp;
pub(crate) mod private_file;
pub mod provider_registry;
pub mod provider_secrets;
pub mod provider_test;
mod retry {}
pub mod openai_def;
#[cfg(feature = "aws-providers")]
pub mod sagemaker_tgi;
pub mod snowflake_def;
pub mod testprovider;
pub mod tetrate;
pub mod toolshim;
pub mod usage_estimator;
pub mod utils;

pub mod xai;
pub mod xai_oauth;

pub use init::{
    cleanup_provider, create, create_with_default_model, create_with_named_model,
    create_with_working_dir, get_from_registry, inventory_identity, providers,
    refresh_custom_providers,
};

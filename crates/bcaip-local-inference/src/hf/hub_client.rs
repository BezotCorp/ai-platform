use anyhow::Result;
use hf_hub::{HFClient, HFRepository, RepoTypeModel};

use crate::huggingface_auth;

async fn optional_hf_token(
    token: impl std::future::Future<Output = Result<Option<String>>>,
) -> Option<String> {
    token.await.ok().flatten()
}

pub(crate) async fn hf_client() -> Result<HFClient> {
    let mut builder = HFClient::builder().user_agent("bcaip-ai-agent");
    if let Some(token) = optional_hf_token(huggingface_auth::resolve_token_async()).await {
        builder = builder.token(token);
    }
    builder.build().map_err(Into::into)
}

pub(crate) fn model_repo(client: &HFClient, repo_id: &str) -> Result<HFRepository<RepoTypeModel>> {
    let (owner, name) = split_repo_id(repo_id)?;
    Ok(client.model(owner, name))
}

pub(crate) fn split_repo_id(repo_id: &str) -> Result<(&str, &str)> {
    repo_id
        .split_once('/')
        .ok_or_else(|| anyhow::anyhow!("Invalid repo id '{}': expected owner/name", repo_id))
}

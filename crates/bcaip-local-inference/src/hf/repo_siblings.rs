use anyhow::Result;
use futures::StreamExt;
use hf_hub::repository::RepoTreeEntry;

use crate::hf::api_sibling::HfApiSibling;
use crate::hf::hub_client::{hf_client, model_repo};

pub(crate) async fn get_repo_siblings(repo_id: &str) -> Result<Vec<HfApiSibling>> {
    let client = hf_client().await?;
    let repo = model_repo(&client, repo_id)?;
    let stream = repo.list_tree().recursive(true).expand(true).send()?;
    futures::pin_mut!(stream);
    let mut siblings = Vec::new();
    while let Some(entry) = stream.next().await {
        if let RepoTreeEntry::File { path, size, .. } = entry? {
            siblings.push(HfApiSibling {
                rfilename: path,
                size: Some(size),
            });
        }
    }
    Ok(siblings)
}

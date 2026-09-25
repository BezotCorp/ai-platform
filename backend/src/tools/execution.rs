use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use tokio::fs;

use super::permissions;

const MAX_VISITED: usize = 4000;
const MAX_FILE_BYTES: u64 = 128 * 1024;
const MAX_RESULT_BYTES: usize = 16 * 1024;

fn parse_arguments<'a>(
    value: &'a Value,
    allowed: &[&str],
) -> Result<&'a Map<String, Value>> {
    let object = value
        .as_object()
        .context("Les arguments doivent être un objet JSON")?;

    if object.keys().any(|key| {
        !allowed.contains(&key.as_str())
    }) {
        bail!("Argument inconnu");
    }

    Ok(object)
}

fn required_string<'a>(
    object: &'a Map<String, Value>,
    key: &str,
) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .context(format!("Argument {key} manquant ou invalide"))
}

async fn confined_path(
    root: &Path,
    relative: &str,
) -> Result<PathBuf> {
    permissions::authorize_path(relative)?;

    let resolved = fs::canonicalize(root.join(relative))
        .await
        .context("Chemin introuvable")?;

    if !resolved.starts_with(root) {
        bail!("Accès hors du projet interdit");
    }

    Ok(resolved)
}

async fn list_files(
    root: &Path,
    arguments: &Value,
) -> Result<Value> {
    let object = parse_arguments(arguments, &["path"])?;
    let relative = required_string(object, "path")?;
    let directory = confined_path(root, relative).await?;

    if !fs::metadata(&directory).await?.is_dir() {
        bail!("Le chemin n'est pas un répertoire");
    }

    let mut pending = VecDeque::from([directory]);
    let mut files = Vec::new();
    let mut visited = 0usize;

    while let Some(directory) = pending.pop_front() {
        let mut entries = fs::read_dir(directory).await?;

        while let Some(entry) = entries.next_entry().await? {
            visited += 1;

            if visited > MAX_VISITED {
                bail!("Limite d'exploration du projet atteinte");
            }

            let path = entry.path();
            let relative = path.strip_prefix(root)?;

            if permissions::authorize_path(
                &relative.to_string_lossy()
            ).is_err() {
                continue;
            }

            let kind = entry.file_type().await?;

            if kind.is_symlink() {
                continue;
            }

            if kind.is_dir() {
                pending.push_back(path);
            } else if kind.is_file() {
                files.push(
                    relative.to_string_lossy().into_owned()
                );
            }
        }

        if files.len() > 200 {
            break;
        }
    }

    files.sort();
    let truncated = files.len() > 200
        || !pending.is_empty();

    files.truncate(200);

    Ok(json!({
        "files": files,
        "truncated": truncated,
    }))
}

async fn read_file(
    root: &Path,
    arguments: &Value,
) -> Result<Value> {
    let object = parse_arguments(
        arguments,
        &["path", "start_line", "max_lines"],
    )?;

    let relative = required_string(object, "path")?;
    let path = confined_path(root, relative).await?;

    let metadata = fs::metadata(&path).await?;

    if !metadata.is_file()
        || metadata.len() > MAX_FILE_BYTES
    {
        bail!("Fichier absent ou trop volumineux");
    }

    let start = object
        .get("start_line")
        .map(|value| {
            value.as_u64().context("start_line invalide")
        })
        .transpose()?
        .unwrap_or(1);

    let limit = object
        .get("max_lines")
        .map(|value| {
            value.as_u64().context("max_lines invalide")
        })
        .transpose()?
        .unwrap_or(80);

    if start == 0 || !(1..=120).contains(&limit) {
        bail!("Intervalle de lecture invalide");
    }

    let content = fs::read_to_string(&path)
        .await
        .context("Le fichier n'est pas du texte UTF-8")?;

    let mut lines = Vec::new();
    let mut bytes = 0usize;

    for (index, line) in content.lines().enumerate() {
        let number = index as u64 + 1;

        if number < start {
            continue;
        }

        if lines.len() >= limit as usize {
            break;
        }

        let remaining =
            MAX_RESULT_BYTES.saturating_sub(bytes);

        if remaining == 0 {
            break;
        }

        let mut text = line.to_owned();

        while text.len() > remaining {
            text.pop();
        }

        bytes += text.len();

        lines.push(json!({
            "line": number,
            "text": text,
        }));
    }

    Ok(json!({
        "path": relative,
        "start_line": start,
        "lines": lines,
        "truncated": bytes >= MAX_RESULT_BYTES
            || content.lines().count() as u64
                >= start.saturating_add(limit),
    }))
}

async fn search_text(
    root: &Path,
    arguments: &Value,
) -> Result<Value> {
    let object = parse_arguments(arguments, &["query"])?;
    let query = required_string(object, "query")?;

    if query.len() < 2 || query.len() > 128 {
        bail!("Longueur de recherche invalide");
    }

    let mut pending =
        VecDeque::from([root.to_path_buf()]);

    let mut matches = Vec::new();
    let mut visited = 0usize;
    let mut truncated = false;

    'exploration: while let Some(directory) =
        pending.pop_front()
    {
        let mut entries = fs::read_dir(directory).await?;

        while let Some(entry) = entries.next_entry().await? {
            visited += 1;

            if visited > MAX_VISITED {
                truncated = true;
                break 'exploration;
            }

            let path = entry.path();
            let relative = path.strip_prefix(root)?;

            if permissions::authorize_path(
                &relative.to_string_lossy()
            ).is_err() {
                continue;
            }

            let kind = entry.file_type().await?;

            if kind.is_symlink() {
                continue;
            }

            if kind.is_dir() {
                pending.push_back(path);
                continue;
            }

            if !kind.is_file() {
                continue;
            }

            let metadata = entry.metadata().await?;

            if metadata.len() > MAX_FILE_BYTES {
                continue;
            }

            let Ok(content) =
                fs::read_to_string(entry.path()).await
            else {
                continue;
            };

            for (index, line) in
                content.lines().enumerate()
            {
                if !line.contains(query) {
                    continue;
                }

                let preview: String =
                    line.chars().take(300).collect();

                matches.push(json!({
                    "path": relative.to_string_lossy(),
                    "line": index + 1,
                    "text": preview,
                }));

                if matches.len() >= 40 {
                    truncated = true;
                    break 'exploration;
                }
            }
        }
    }

    Ok(json!({
        "matches": matches,
        "truncated": truncated,
        "visited_entries": visited,
    }))
}

pub(crate) async fn execute(
    root: &Path,
    name: &str,
    args: &Value,
) -> Result<Value> {
    permissions::authorize_tool(name)?;

    match name {
        "project.list_files" => {
            list_files(root, args).await
        }

        "project.read_file" => {
            read_file(root, args).await
        }

        "project.search_text" => {
            search_text(root, args).await
        }

        _ => bail!("Outil non autorisé"),
    }
}

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};
use cap_std::fs::Dir;
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use tokio::task;

use crate::{
    file_manager::FileManager,
    tools::{anchored_path::AnchoredPath, permissions},
};

const MAX_VISITED: usize = 4000;
const MAX_FILE_BYTES: u64 = 128 * 1024;
const MAX_RESULT_BYTES: usize = 16 * 1024;

fn parse_arguments<'a>(value: &'a Value, allowed: &[&str]) -> Result<&'a Map<String, Value>> {
    let object = value
        .as_object()
        .context("Les arguments doivent être un objet JSON")?;
    if object.keys().any(|key| !allowed.contains(&key.as_str())) {
        bail!("Argument inconnu");
    }
    Ok(object)
}

fn required_string<'a>(object: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    object
        .get(key)
        .and_then(Value::as_str)
        .context(format!("Argument {key} manquant ou invalide"))
}

fn visit_directory(
    pending: &mut VecDeque<(PathBuf, Dir)>,
    directory: &Dir,
    relative: PathBuf,
) -> Result<()> {
    let opened = FileManager::open_child_directory(
        directory,
        relative
            .file_name()
            .map(Path::new)
            .context("Nom de répertoire manquant")?,
    )?;
    pending.push_back((relative, opened));
    Ok(())
}

fn list_files(root: &Path, arguments: &Value) -> Result<Value> {
    let object = parse_arguments(arguments, &["path"])?;
    let relative = required_string(object, "path")?;
    let initial = FileManager::open_directory(root, Path::new(relative))?;
    let prefix = if relative == "." {
        PathBuf::new()
    } else {
        PathBuf::from(relative)
    };
    let mut pending = VecDeque::from([(prefix, initial)]);
    let mut files = Vec::new();
    let mut visited = 0usize;
    let mut truncated = false;
    'exploration: while let Some((directory_path, directory)) = pending.pop_front() {
        for item in directory.entries()? {
            let entry = item?;
            visited += 1;
            if visited > MAX_VISITED {
                truncated = true;
                break 'exploration;
            }
            let name = entry.file_name();
            let relative = directory_path.join(&name);
            let Some(relative_text) = relative.to_str() else {
                continue;
            };
            if permissions::authorize_path(relative_text).is_err() {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                visit_directory(&mut pending, &directory, relative)?;
            } else if kind.is_file() {
                files.push(relative_text.to_owned());
                if files.len() > 200 {
                    truncated = true;
                    break 'exploration;
                }
            }
        }
    }
    files.sort();
    files.truncate(200);
    Ok(json!({
        "files": files,
        "truncated": truncated,
    }))
}

fn read_file(root: &Path, arguments: &Value) -> Result<Value> {
    let object = parse_arguments(arguments, &["path", "start_line", "max_lines"])?;
    let relative = required_string(object, "path")?;
    let snapshot = AnchoredPath::open(root, relative)?.read_existing()?;
    if snapshot.bytes.len() > MAX_FILE_BYTES as usize {
        bail!("Fichier trop volumineux");
    }
    let start = object
        .get("start_line")
        .map(|value| value.as_u64().context("start_line invalide"))
        .transpose()?
        .unwrap_or(1);
    let limit = object
        .get("max_lines")
        .map(|value| value.as_u64().context("max_lines invalide"))
        .transpose()?
        .unwrap_or(80);
    if start == 0 || !(1..=120).contains(&limit) {
        bail!("Intervalle de lecture invalide");
    }
    let sha256 = Sha256::digest(&snapshot.bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let content =
        String::from_utf8(snapshot.bytes).context("Le fichier n'est pas du texte UTF-8")?;
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
        let remaining = MAX_RESULT_BYTES.saturating_sub(bytes);
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
        "sha256": sha256,
        "start_line": start,
        "lines": lines,
        "truncated": bytes >= MAX_RESULT_BYTES
            || content.lines().count() as u64 >= start.saturating_add(limit),
    }))
}

fn search_text(root: &Path, arguments: &Value) -> Result<Value> {
    let object = parse_arguments(arguments, &["query"])?;
    let query = required_string(object, "query")?;
    if query.len() < 2 || query.len() > 128 {
        bail!("Longueur de recherche invalide");
    }
    let initial = FileManager::open_directory(root, Path::new("."))?;
    let mut pending = VecDeque::from([(PathBuf::new(), initial)]);
    let mut matches = Vec::new();
    let mut visited = 0usize;
    let mut truncated = false;
    'exploration: while let Some((directory_path, directory)) = pending.pop_front() {
        for item in directory.entries()? {
            let entry = item?;
            visited += 1;
            if visited > MAX_VISITED {
                truncated = true;
                break 'exploration;
            }
            let name = entry.file_name();
            let relative = directory_path.join(&name);
            let Some(relative_text) = relative.to_str() else {
                continue;
            };
            if permissions::authorize_path(relative_text).is_err() {
                continue;
            }
            let kind = entry.file_type()?;
            if kind.is_symlink() {
                continue;
            }
            if kind.is_dir() {
                visit_directory(&mut pending, &directory, relative)?;
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let snapshot = match AnchoredPath::open(root, relative_text)
                .and_then(|path| path.read_existing())
            {
                Ok(snapshot) => snapshot,
                Err(_) => continue,
            };
            let Ok(content) = String::from_utf8(snapshot.bytes) else {
                continue;
            };
            for (index, line) in content.lines().enumerate() {
                if !line.contains(query) {
                    continue;
                }
                let preview: String = line.chars().take(300).collect();
                matches.push(json!({
                    "path": relative_text,
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

pub(crate) async fn execute(root: &Path, name: &str, args: &Value) -> Result<Value> {
    permissions::authorize_tool(name)?;
    let root = root.to_path_buf();
    let name = name.to_owned();
    let args = args.clone();
    task::spawn_blocking(move || match name.as_str() {
        "project.list_files" => list_files(&root, &args),
        "project.read_file" => read_file(&root, &args),
        "project.search_text" => search_text(&root, &args),
        _ => bail!("Outil non autorisé"),
    })
    .await?
}

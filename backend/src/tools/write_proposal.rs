use std::path::Path;

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use similar::TextDiff;
use tokio::{sync::OwnedMutexGuard, task};

use crate::tools::{anchored_path::AnchoredPath, file_snapshot::FileSnapshot};

const MAX_SOURCE: usize = 128 * 1024;
const MAX_PREVIEW: usize = 48 * 1024;

pub(crate) struct WriteProposal {
    relative: String,
    anchored: AnchoredPath,
    previous: Option<FileSnapshot>,
    replacement: Vec<u8>,
    diff: String,
}

fn argument<'a>(object: &'a Map<String, Value>, name: &str) -> Result<&'a str> {
    object
        .get(name)
        .and_then(Value::as_str)
        .context(format!("Argument {name} manquant ou invalide"))
}

fn digest(data: &[u8]) -> String {
    Sha256::digest(data)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

impl WriteProposal {
    pub(crate) fn is_write(name: &str) -> bool {
        matches!(name, "project.replace_text" | "project.create_file")
    }

    pub(crate) async fn prepare(root: &Path, name: &str, arguments: &Value) -> Result<Self> {
        let root = root.to_path_buf();
        let name = name.to_owned();
        let arguments = arguments.clone();
        tokio::task::spawn_blocking(move || Self::prepare_sync(&root, &name, &arguments)).await?
    }

    fn prepare_sync(root: &Path, name: &str, arguments: &Value) -> Result<Self> {
        if !Self::is_write(name) {
            bail!("Outil d'écriture inconnu");
        }
        let object = arguments.as_object().context("Arguments JSON invalides")?;
        let relative = argument(object, "path")?.to_owned();
        let create = name == "project.create_file";
        let allowed: &[&str] = if create {
            &["path", "content"]
        } else {
            &["path", "expected_sha256", "old", "new"]
        };
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            bail!("Argument d'écriture inconnu");
        }
        let anchored = AnchoredPath::open(root, &relative)?;
        let (previous, replacement) = if create {
            anchored.ensure_absent()?;
            let content = argument(object, "content")?;
            if content.len() > MAX_SOURCE {
                bail!("Nouveau fichier trop volumineux");
            }
            (None, content.as_bytes().to_vec())
        } else {
            let original = anchored.read_existing()?;
            let text = str::from_utf8(&original.bytes).context("Fichier non UTF-8")?;
            let expected = argument(object, "expected_sha256")?;
            if expected.len() != 64
                || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
                || digest(&original.bytes) != expected.to_ascii_lowercase()
            {
                bail!("SHA-256 du fichier différent");
            }
            let old = argument(object, "old")?;
            let new = argument(object, "new")?;
            if old.is_empty() || old == new || text.matches(old).count() != 1 {
                bail!("Le texte à remplacer doit être unique et différent");
            }
            let replacement = text.replacen(old, new, 1);
            if replacement.len() > MAX_SOURCE {
                bail!("Résultat trop volumineux");
            }
            (Some(original), replacement.into_bytes())
        };
        let before = match &previous {
            Some(snapshot) => std::str::from_utf8(&snapshot.bytes)?,
            None => "",
        };
        let after = std::str::from_utf8(&replacement)?;
        let left = format!("a/{relative}");
        let right = format!("b/{relative}");
        let diff = TextDiff::from_lines(before, after)
            .unified_diff()
            .context_radius(3)
            .header(&left, &right)
            .to_string();
        if diff.len() > MAX_PREVIEW {
            bail!("Diff trop volumineux : fractionner la modification");
        }
        Ok(Self {
            relative,
            anchored,
            previous,
            replacement,
            diff,
        })
    }

    pub(crate) fn preview(&self) -> Value {
        json!({
            "path": self.relative,
            "diff": self.diff,
            "previous_sha256": self.previous
                .as_ref()
                .map(|snapshot| digest(&snapshot.bytes)),
            "new_sha256": digest(&self.replacement),
            "operation": if self.previous.is_some() {
                "replace_text"
            } else {
                "create_file"
            },
        })
    }

    pub(crate) async fn commit(self, guard: OwnedMutexGuard<()>) -> Result<Value> {
        task::spawn_blocking(move || {
            let _guard = guard;
            self.anchored
                .publish(self.previous.as_ref(), &self.replacement)?;
            Ok(json!({
                "path": self.relative,
                "sha256": digest(&self.replacement),
                "operation": if self.previous.is_some() {
                    "replaced"
                } else {
                    "created"
                },
            }))
        })
        .await?
    }
}

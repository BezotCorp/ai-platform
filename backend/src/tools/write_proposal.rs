use std::{
    io,
    path::{Component, Path, PathBuf},
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use serde_json::{Map, Value, json};
use sha2::{Digest, Sha256};
use similar::TextDiff;
use tokio::{fs, io::AsyncWriteExt};

use crate::tools::permissions;

const MAX_SOURCE: usize = 128 * 1024;
const MAX_PREVIEW: usize = 48 * 1024;

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct WriteProposal {
    relative: String,
    target: PathBuf,
    previous: Option<Vec<u8>>,
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
    let hash = Sha256::digest(data);
    hash.iter().map(|byte| format!("{byte:02x}")).collect()
}

// Vérifie les composants du chemin sans accepter
// les liens symboliques. Cette vérification est
// répétée immédiatement avant une écriture.
//
// Elle ne remplace pas une protection contre les
// courses provoquées par d'autres processus.
async fn path_in_project(root: &Path, relative: &str, create: bool) -> Result<PathBuf> {
    permissions::authorize_path(relative)?;
    let components: Vec<_> = Path::new(relative).components().collect();
    if components.is_empty()
        || components
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        bail!("Chemin d'écriture invalide");
    }
    let mut cursor = root.to_path_buf();
    for (index, component) in components.iter().enumerate() {
        let Component::Normal(name) = component else {
            unreachable!()
        };
        cursor.push(name);
        let final_component = index + 1 == components.len();
        match fs::symlink_metadata(&cursor).await {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    bail!("Lien symbolique interdit");
                }
                if !final_component && !metadata.is_dir() {
                    bail!("Un composant du chemin n'est pas un répertoire");
                }
                if final_component && (create || !metadata.is_file()) {
                    bail!("Destination existante ou non régulière");
                }
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound && final_component && create => {}

            Err(error) => return Err(error.into()),
        }
    }
    if !cursor.starts_with(root) {
        bail!("Destination hors du projet");
    }
    Ok(cursor)
}

impl WriteProposal {
    pub(crate) fn is_write(name: &str) -> bool {
        matches!(name, "project.replace_text" | "project.create_file")
    }

    pub(crate) async fn prepare(root: &Path, name: &str, arguments: &Value) -> Result<Self> {
        let object = arguments.as_object().context("Arguments JSON invalides")?;
        let relative = argument(object, "path")?.to_owned();
        let create = name == "project.create_file";
        if !Self::is_write(name) {
            bail!("Outil d'écriture inconnu");
        }
        let allowed: &[&str] = if create {
            &["path", "content"]
        } else {
            &["path", "expected_sha256", "old", "new"]
        };
        if object.keys().any(|key| !allowed.contains(&key.as_str())) {
            bail!("Argument d'écriture inconnu");
        }
        let target = path_in_project(root, &relative, create).await?;
        let (previous, replacement) = if create {
            let content = argument(object, "content")?;
            if content.len() > MAX_SOURCE {
                bail!("Nouveau fichier trop volumineux");
            }
            (None, content.as_bytes().to_vec())
        } else {
            let previous = fs::read(&target).await?;
            if previous.len() > MAX_SOURCE {
                bail!("Fichier trop volumineux");
            }
            let old_text = str::from_utf8(&previous).context("Fichier non UTF-8")?;
            let expected = argument(object, "expected_sha256")?;
            if expected.len() != 64
                || !expected.bytes().all(|byte| byte.is_ascii_hexdigit())
                || digest(&previous) != expected.to_ascii_lowercase()
            {
                bail!("Le fichier a changé : SHA-256 différent");
            }
            let old = argument(object, "old")?;
            let new = argument(object, "new")?;
            if old.is_empty() || old == new || old_text.matches(old).count() != 1 {
                bail!("Le texte à remplacer doit être unique et différent");
            }
            let replacement = old_text.replacen(old, new, 1);
            if replacement.len() > MAX_SOURCE {
                bail!("Résultat trop volumineux");
            }
            (Some(previous), replacement.into_bytes())
        };
        let old = match &previous {
            Some(data) => str::from_utf8(data)?,
            None => "",
        };
        let new = str::from_utf8(&replacement)?;
        let before_label = format!("a/{relative}");
        let after_label = format!("b/{relative}");
        let diff = TextDiff::from_lines(old, new)
            .unified_diff()
            .context_radius(3)
            .header(&before_label, &after_label)
            .to_string();
        if diff.len() > MAX_PREVIEW {
            bail!("Diff trop volumineux : fractionner la modification");
        }
        Ok(Self {
            relative,
            target,
            previous,
            replacement,
            diff,
        })
    }

    pub(crate) fn preview(&self) -> Value {
        json!({
            "path": self.relative,
            "diff": self.diff,
            "previous_sha256": self
                .previous
                .as_ref()
                .map(|data| digest(data)),
            "new_sha256": digest(&self.replacement),
            "operation": if self.previous.is_some() {
                "replace_text"
            } else {
                "create_file"
            },
        })
    }

    pub(crate) async fn commit(self, root: &Path) -> Result<Value> {
        let create = self.previous.is_none();
        let actual = path_in_project(root, &self.relative, create).await?;
        if actual != self.target {
            bail!("Chemin de destination modifié");
        }
        if let Some(previous) = &self.previous
            && fs::read(&actual).await? != *previous
        {
            bail!("Conflit : le fichier a changé depuis l'aperçu");
        }
        let parent = actual.parent().context("Répertoire parent absent")?;
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let serial = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".ai-platform-{}-{nonce}-{serial}.tmp",
            process::id(),
        ));
        let result = async {
            let mut output = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .await?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let permissions = if create {
                    std::fs::Permissions::from_mode(0o600)
                } else {
                    fs::metadata(&actual).await?.permissions()
                };
                fs::set_permissions(&temporary, permissions).await?;
            }
            output.write_all(&self.replacement).await?;
            output.sync_all().await?;
            drop(output);
            // Une création ne remplace jamais
            // un fichier apparu entre-temps.
            if create {
                fs::hard_link(&temporary, &actual).await?;
            } else {
                let previous = self
                    .previous
                    .as_ref()
                    .context("Version précédente absente")?;
                if fs::read(&actual).await? != *previous {
                    bail!("Conflit avant remplacement atomique");
                }
                fs::rename(&temporary, &actual).await?;
            }
            Ok::<_, anyhow::Error>(())
        }
        .await;
        let _ = fs::remove_file(&temporary).await;
        result?;
        Ok(json!({
            "path": self.relative,
            "sha256": digest(&self.replacement),
            "operation": if create {
                "created"
            } else {
                "replaced"
            },
        }))
    }
}

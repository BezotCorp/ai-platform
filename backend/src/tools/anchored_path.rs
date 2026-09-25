use anyhow::{Context, Result, bail};
use cap_std::fs::{MetadataExt, OpenOptions, Permissions};
use std::{
    fs,
    io::{self, Read, Write},
    os::unix::fs::PermissionsExt,
    path::Path,
    process,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::{
    file_manager::FileManager,
    tools::{file_snapshot::FileSnapshot, permissions},
};

const MAX_FILE_BYTES: usize = 128 * 1024;
static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

pub(crate) struct AnchoredPath {
    file_manager: FileManager,
}

impl AnchoredPath {
    pub(crate) fn open(root: &Path, relative: &str) -> Result<Self> {
        permissions::authorize_path(relative)?;
        Ok(Self {
            file_manager: FileManager::new(root, relative)?,
        })
    }

    pub(crate) fn ensure_absent(&self) -> Result<()> {
        self.file_manager.ensure_absent()
    }

    pub(crate) fn read_existing(&self) -> Result<FileSnapshot> {
        let name = self.file_manager.filename();
        let before = self.file_manager.parent().symlink_metadata(name)?;
        if before.is_symlink() || !before.is_file() {
            bail!("La cible n'est pas un fichier régulier");
        }
        if before.nlink() != 1 {
            bail!("Fichier à plusieurs liens matériels");
        }
        if before.len() > MAX_FILE_BYTES as u64 {
            bail!("Fichier trop volumineux");
        }
        // L'accès demeure relatif au répertoire
        // détenu par cap-std.
        let mut file = self.file_manager.parent().open(name)?;
        let opened = file.metadata()?;
        if !opened.is_file()
            || opened.nlink() != 1
            || opened.dev() != before.dev()
            || opened.ino() != before.ino()
        {
            bail!("La cible a changé pendant son ouverture");
        }
        let mut bytes = Vec::new();
        Read::by_ref(&mut file)
            .take((MAX_FILE_BYTES + 1) as u64)
            .read_to_end(&mut bytes)?;
        if bytes.len() > MAX_FILE_BYTES {
            bail!("Fichier trop volumineux");
        }
        let after = file.metadata()?;
        let final_path = self.file_manager.parent().symlink_metadata(name)?;
        if final_path.is_symlink()
            || !final_path.is_file()
            || opened.dev() != after.dev()
            || opened.ino() != after.ino()
            || opened.len() != after.len()
            || opened.modified()? != after.modified()?
            || after.dev() != final_path.dev()
            || after.ino() != final_path.ino()
        {
            bail!("Fichier modifié pendant la lecture");
        }
        Ok(FileSnapshot {
            bytes,
            device: opened.dev(),
            inode: opened.ino(),
            mode: opened.mode() & 0o777,
        })
    }

    pub(crate) fn publish(
        &self,
        previous: Option<&FileSnapshot>,
        replacement: &[u8],
    ) -> Result<()> {
        if replacement.len() > MAX_FILE_BYTES {
            bail!("Contenu trop volumineux");
        }
        self.file_manager.verify_parent()?;
        if let Some(previous) = previous {
            let current = self.read_existing()?;
            if !previous.matches(&current) {
                bail!("Conflit : le fichier a changé");
            }
        } else {
            self.file_manager.ensure_absent()?;
        }
        let serial = TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        let temporary = format!(".ai-platform-{}-{nonce}-{serial}.tmp", process::id(),);
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        let mut output = self.file_manager.parent().open_with(&temporary, &options)?;
        let result = (|| -> Result<()> {
            if let Some(previous) = previous {
                let permissions = Permissions::from_std(fs::Permissions::from_mode(previous.mode));
                output.set_permissions(permissions)?;
            }
            output.write_all(replacement)?;
            output.sync_all()?;
            drop(output);
            self.file_manager.verify_parent()?;
            if let Some(previous) = previous {
                let current = self.read_existing()?;
                if !previous.matches(&current) {
                    bail!("Conflit avant publication");
                }
                self.file_manager.parent().rename(
                    &temporary,
                    self.file_manager.parent(),
                    self.file_manager.filename(),
                )?;
            } else {
                self.file_manager.ensure_absent()?;
                // Un lien matériel crée le fichier
                // final sans remplacer une cible
                // apparue entre-temps.
                self.file_manager.parent().hard_link(
                    &temporary,
                    self.file_manager.parent(),
                    self.file_manager.filename(),
                )?;
            }
            Ok(())
        })();
        let cleanup = self.file_manager.parent().remove_file(&temporary);
        if result.is_ok() {
            match cleanup {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(anyhow::anyhow!(
                        "Publication effectuée, nettoyage échoué : {error}"
                    ));
                }
            }
        }
        result?;
        // Synchroniser également le répertoire
        // après la publication.
        self.file_manager
            .parent()
            .open(".")?
            .sync_all()
            .context("Publication effectuée, synchronisation du répertoire échouée")?;
        Ok(())
    }
}

use std::{
    ffi::OsString,
    io,
    path::{Component, Path, PathBuf},
};

use crate::tools::authorize_path;
use anyhow::{Result, bail};
use cap_std::{
    ambient_authority,
    fs::{Dir, MetadataExt},
};

pub(crate) struct FileManager {
    filename: OsString,
    root: PathBuf,
    relative: String,
    parent: Dir,
}

impl FileManager {
    pub(crate) fn new(root: &Path, relative: &str) -> Result<Self> {
        authorize_path(relative)?;
        let components = Path::new(relative).components().collect::<Vec<_>>();
        if components.is_empty()
            || components
                .iter()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("Chemin de fichier invalide");
        }
        let Component::Normal(filename) = components[components.len() - 1] else {
            bail!("Nom de fichier invalide");
        };
        let parent_path = Path::new(relative).parent().unwrap_or(Path::new("."));
        let parent_path = if parent_path.as_os_str().is_empty() {
            Path::new(".")
        } else {
            parent_path
        };

        Ok(Self {
            filename: filename.to_os_string(),
            root: root.to_path_buf(),
            relative: relative.to_owned(),
            parent: Self::open_directory(root, parent_path)?,
        })
    }

    pub(crate) fn filename(&self) -> &Path {
        Path::new(&self.filename)
    }

    pub(crate) fn relative(&self) -> &str {
        &self.relative
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn parent(&self) -> &Dir {
        &self.parent
    }

    pub(crate) fn open_directory(root: &Path, relative: &Path) -> Result<Dir> {
        let relative = relative
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Chemin non UTF-8 interdit"))?;
        authorize_path(relative)?;
        let mut directory = Dir::open_ambient_dir(root, ambient_authority())?;
        if relative == "." {
            return Ok(directory);
        }
        let components = Path::new(relative).components().collect::<Vec<_>>();
        if components.is_empty()
            || components
                .iter()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            bail!("Chemin de répertoire invalide");
        }
        for component in components {
            let Component::Normal(name) = component else {
                bail!("Composant de répertoire invalide");
            };
            directory = Self::open_child_directory(&directory, Path::new(name))?;
        }
        Ok(directory)
    }

    pub(crate) fn open_child_directory(parent: &Dir, name: &Path) -> Result<Dir> {
        let before = parent.symlink_metadata(name)?;
        if before.is_symlink() || !before.is_dir() {
            bail!("Répertoire interdit ou invalide");
        }
        let child = parent.open_dir(name)?;
        let opened = child.dir_metadata()?;
        let after = parent.symlink_metadata(name)?;
        if after.is_symlink()
            || !after.is_dir()
            || before.dev() != opened.dev()
            || before.ino() != opened.ino()
            || after.dev() != opened.dev()
            || after.ino() != opened.ino()
        {
            bail!("Répertoire modifié pendant son ouverture");
        }
        Ok(child)
    }

    pub(crate) fn verify_parent(&self) -> Result<()> {
        let current = Self::new(self.root(), self.relative())?;
        let original = self.parent.dir_metadata()?;
        let actual = current.parent.dir_metadata()?;
        if original.dev() != actual.dev() || original.ino() != actual.ino() {
            bail!("Répertoire déplacé ou remplacé");
        }
        Ok(())
    }

    pub(crate) fn ensure_absent(&self) -> Result<()> {
        match self.parent.symlink_metadata(self.filename()) {
            Ok(_) => bail!("Le fichier existe déjà"),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }
}

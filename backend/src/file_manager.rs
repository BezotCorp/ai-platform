use std::{
    ffi::OsString,
    io,
    path::{Component, Path, PathBuf},
};

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
        let components = Path::new(relative).components().collect::<Vec<_>>();
        if components.is_empty()
            || components
                .iter()
                .any(|part| !matches!(part, Component::Normal(_)))
        {
            bail!("Chemin de fichier invalide");
        }
        // L'autorité ambiante est utilisée
        // uniquement pour ouvrir la racine.
        let mut parent = Dir::open_ambient_dir(root, ambient_authority())?;
        // Parcourir les répertoires relativement
        // au descripteur déjà ouvert.
        for component in &components[..components.len() - 1] {
            let Component::Normal(name) = component else {
                bail!("Composant de chemin invalide");
            };
            let part = Path::new(name);
            let before = parent.symlink_metadata(part)?;
            if before.is_symlink() || !before.is_dir() {
                bail!("Répertoire interdit ou invalide");
            }
            let child = parent.open_dir(part)?;
            let opened = child.symlink_metadata(".")?;
            let after = parent.symlink_metadata(part)?;
            if after.is_symlink()
                || !after.is_dir()
                || before.dev() != opened.dev()
                || before.ino() != opened.ino()
                || after.dev() != opened.dev()
                || after.ino() != opened.ino()
            {
                bail!("Répertoire modifié pendant son ouverture");
            }
            parent = child;
        }
        let Component::Normal(filename) = components[components.len() - 1] else {
            bail!("Nom de fichier invalide");
        };
        Ok(Self {
            filename: filename.to_os_string(),
            root: root.to_path_buf(),
            relative: relative.to_owned(),
            parent,
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

    pub(crate) fn verify_parent(&self) -> Result<()> {
        // Repartir de la racine pour vérifier
        // que le chemin désigne encore le
        // répertoire précédemment ouvert.
        let current = Self::new(self.root(), self.relative())?;
        let original = self.parent.symlink_metadata(".")?;
        let actual = current.parent.symlink_metadata(".")?;
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

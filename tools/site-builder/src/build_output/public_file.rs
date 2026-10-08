use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug)]
pub(crate) struct PublicFile {
    source: PathBuf,
}

impl PublicFile {
    pub(crate) fn new(source: PathBuf) -> Self {
        Self { source }
    }

    pub(crate) fn source(&self) -> &Path {
        &self.source
    }

    pub(crate) fn relative_path<'a>(&'a self, public_root: &Path) -> Result<&'a Path> {
        self.source.strip_prefix(public_root).with_context(|| {
            format!(
                "{} is outside public root {}",
                self.source.display(),
                public_root.display(),
            )
        })
    }
}

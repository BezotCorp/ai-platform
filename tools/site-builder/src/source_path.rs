use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct SourcePath(PathBuf);

impl SourcePath {
    pub(crate) fn as_path(&self) -> &Path {
        &self.0
    }
}

impl From<PathBuf> for SourcePath {
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

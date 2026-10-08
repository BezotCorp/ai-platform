use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct TargetPath(PathBuf);

impl TargetPath {
    pub(crate) fn as_path(&self) -> &Path {
        &self.0
    }
}

impl From<PathBuf> for TargetPath {
    fn from(path: PathBuf) -> Self {
        Self(path)
    }
}

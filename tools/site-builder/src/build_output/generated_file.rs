use std::path::PathBuf;

#[derive(Debug)]
pub(crate) struct GeneratedFile {
    pub(crate) path: PathBuf,
    pub(crate) content: String,
}

impl GeneratedFile {
    pub(crate) fn new(path: impl Into<PathBuf>, content: String) -> Self {
        Self {
            path: path.into(),
            content,
        }
    }
}

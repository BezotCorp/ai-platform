use std::path::PathBuf;

pub(crate) struct PreparedChange {
    pub(crate) before_path: Option<PathBuf>,
    pub(crate) after_path: Option<PathBuf>,
    pub(crate) before: Option<String>,
    pub(crate) after: Option<String>,
}

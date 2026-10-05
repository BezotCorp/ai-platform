use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct ImportIssue {
    pub(crate) importer: PathBuf,
    pub(crate) line: usize,
    pub(crate) code: String,
    pub(crate) symbol: String,
    pub(crate) current_module: String,
    pub(crate) kind: Option<String>,
    pub(crate) target: Option<PathBuf>,
    pub(crate) status: String,
}

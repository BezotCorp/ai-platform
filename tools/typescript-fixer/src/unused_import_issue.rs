use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) struct UnusedImportIssue {
    pub(crate) importer: PathBuf,
    pub(crate) line: usize,
    pub(crate) code: String,
    pub(crate) symbol: Option<String>,
    pub(crate) whole_import: bool,
}

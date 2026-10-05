use std::path::PathBuf;

#[derive(Debug)]
pub struct TscDiagnostic {
    pub file: PathBuf,
    pub line: usize,
    pub column: usize,
    pub code: String,
    pub message: String,
    pub symbol: Option<String>,
    pub current_module: Option<String>,
    pub unused_symbol: Option<String>,
    pub whole_import_unused: bool,
    pub missing_module: Option<String>,
}

use std::path::PathBuf;

#[derive(Debug, Clone)]
pub(crate) enum FixAction {
    RemoveUnusedImport {
        importer: PathBuf,
        symbol: Option<String>,
        whole_import: bool,
        line: usize,
        reason: String,
    },
    RedirectModule {
        importer: PathBuf,
        current_module: String,
        target: PathBuf,
        line: usize,
        reason: String,
    },
    RedirectImport {
        importer: PathBuf,
        symbol: String,
        current_module: String,
        target: PathBuf,
        kind: String,
        line: usize,
        reason: String,
    },
    RenameType {
        from: String,
        to: String,
        reason: String,
    },
    RenameFile {
        from: PathBuf,
        to: PathBuf,
        reason: String,
    },
    ExtractDeclaration {
        source: PathBuf,
        target: PathBuf,
        kind: String,
        name: String,
        line: usize,
        reason: String,
    },
}

impl FixAction {
    pub(crate) fn describe(&self) -> String {
        match self {
            Self::RemoveUnusedImport {
                importer,
                symbol,
                line,
                ..
            } => match symbol {
                Some(symbol) => {
                    format!("REMOVE IMPORT {}:{}  {}", importer.display(), line, symbol,)
                }

                None => {
                    format!(
                        "REMOVE IMPORT {}:{}  entire declaration",
                        importer.display(),
                        line,
                    )
                }
            },

            Self::RedirectModule {
                importer,
                current_module,
                target,
                line,
                ..
            } => {
                format!(
                    "REDIRECT MODULE {}:{}  {} -> {}",
                    importer.display(),
                    line,
                    current_module,
                    target.display(),
                )
            }

            Self::RedirectImport {
                importer,
                symbol,
                target,
                line,
                ..
            } => {
                format!(
                    "REDIRECT     {}:{}  {} -> {}",
                    importer.display(),
                    line,
                    symbol,
                    target.display(),
                )
            }

            Self::RenameType { from, to, .. } => {
                format!("RENAME TYPE  {from} -> {to}")
            }

            Self::RenameFile { from, to, .. } => {
                format!("RENAME FILE  {} -> {}", from.display(), to.display(),)
            }

            Self::ExtractDeclaration {
                source,
                target,
                name,
                line,
                ..
            } => {
                format!(
                    "EXTRACT      {}:{}  {} -> {}",
                    source.display(),
                    line,
                    name,
                    target.display(),
                )
            }
        }
    }

    pub(crate) fn reason(&self) -> &str {
        match self {
            Self::RemoveUnusedImport { reason, .. }
            | Self::RedirectModule { reason, .. }
            | Self::RedirectImport { reason, .. }
            | Self::RenameType { reason, .. }
            | Self::RenameFile { reason, .. }
            | Self::ExtractDeclaration { reason, .. } => reason,
        }
    }
}

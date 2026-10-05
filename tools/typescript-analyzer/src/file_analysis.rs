use std::path::PathBuf;

use crate::{declaration::Declaration, declaration_kind::DeclarationKind};

#[derive(Debug)]
pub struct FileAnalysis {
    pub path: PathBuf,
    pub declarations: Vec<Declaration>,
    pub has_jsx: bool,
    pub parse_diagnostic_count: usize,
    pub fatal_parse_error: bool,
}

impl FileAnalysis {
    pub fn is_relevant(&self) -> bool {
        self.has_parse_errors() || !self.declarations.is_empty()
    }

    pub fn has_parse_errors(&self) -> bool {
        self.fatal_parse_error || self.parse_diagnostic_count > 0
    }

    pub fn is_ok(&self) -> bool {
        if self.has_parse_errors() || self.declarations.len() != 1 {
            return false;
        }

        let declaration = &self.declarations[0];

        if !DeclarationKind::is_declaration_name_normalized(&declaration.name) {
            return false;
        }

        let Some(file_name) = self.path.file_name().and_then(|name| name.to_str()) else {
            return false;
        };

        DeclarationKind::expected_file_name(&declaration.name, declaration.kind, self.has_jsx)
            == file_name
    }
}

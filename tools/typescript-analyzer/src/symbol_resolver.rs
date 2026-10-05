use std::{fs, path::PathBuf};

use crate::{declaration_kind::DeclarationKind, file_analysis::FileAnalysis};

pub fn resolve(analyses: &[FileAnalysis], symbol: &str) -> Vec<(PathBuf, DeclarationKind)> {
    let mut matches = Vec::new();

    for analysis in analyses {
        /*
         * Une cible proposée par l'analyzer doit elle-même
         * être structurellement conforme.
         */
        if !analysis.is_ok() {
            continue;
        }

        let Some(declaration) = analysis.declarations.first() else {
            continue;
        };

        if declaration.name != symbol {
            continue;
        }

        /*
         * On ne redirige jamais vers une simple déclaration
         * locale ou vers un export default lorsqu'un import
         * nommé est recherché.
         */
        if !is_named_export(&analysis.path, declaration.line) {
            continue;
        }

        matches.push((analysis.path.clone(), declaration.kind));
    }

    matches
}

fn is_named_export(path: &std::path::Path, line: usize) -> bool {
    let Ok(source) = fs::read_to_string(path) else {
        return false;
    };

    let Some(source_line) = source.lines().nth(line.saturating_sub(1)) else {
        return false;
    };

    let trimmed = source_line.trim_start();

    trimmed.starts_with("export ") && !trimmed.starts_with("export default ")
}

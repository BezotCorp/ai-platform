use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
};

use regex::Regex;

use crate::{file_analysis::FileAnalysis, symbol_resolver};

pub fn resolve(importer: &Path, missing_module: &str, analyses: &[FileAnalysis]) -> Vec<PathBuf> {
    /*
     * Premier cas : le fichier existe toujours sous le même
     * chemin logique, avec uniquement une différence de casse.
     *
     * githubUpdater -> gitHubUpdater par exemple.
     */
    let mut case_matches = Vec::new();

    for analysis in analyses {
        let Ok(specifier) = relative_module_specifier(importer, &analysis.path) else {
            continue;
        };

        if specifier.eq_ignore_ascii_case(missing_module) {
            case_matches.push(analysis.path.clone());
        }
    }

    case_matches.sort();
    case_matches.dedup();

    if !case_matches.is_empty() {
        return case_matches;
    }

    /*
     * Deuxième cas : on lit exactement l'import cassé et on
     * récupère ses symboles nommés.
     *
     * Si tous les symboles résolubles pointent vers un seul et
     * même fichier canonique, cette cible est prouvée.
     */
    let Ok(source) = fs::read_to_string(importer) else {
        return Vec::new();
    };

    let symbols = imported_symbols(&source, missing_module);

    if symbols.is_empty() {
        return Vec::new();
    }

    let mut targets = BTreeSet::new();

    for symbol in symbols {
        let matches = symbol_resolver::resolve(analyses, &symbol);

        if matches.len() != 1 {
            continue;
        }

        targets.insert(matches[0].0.clone());
    }

    targets.into_iter().collect()
}

fn imported_symbols(source: &str, module: &str) -> Vec<String> {
    let Ok(pattern) = Regex::new(&format!(
        r#"(?s)import\s+(?:type\s+)?(?:([A-Za-z_$][A-Za-z0-9_$]*)\s*,\s*)?\{{([^}}]*)\}}\s+from\s+["']{}["']\s*;"#,
        regex::escape(module,),
    )) else {
        return Vec::new();
    };

    let Some(captures) = pattern.captures(source) else {
        return Vec::new();
    };

    let mut result = Vec::new();

    if let Some(default) = captures.get(1) {
        result.push(default.as_str().to_string());
    }

    let body = captures.get(2).map(|value| value.as_str()).unwrap_or("");

    for part in body.split(',') {
        let part = part.trim();

        if part.is_empty() {
            continue;
        }

        let part = part.strip_prefix("type ").unwrap_or(part).trim();

        let Some(name) = part.split_whitespace().next() else {
            continue;
        };

        result.push(name.to_string());
    }

    result
}

fn relative_module_specifier(importer: &Path, target: &Path) -> Result<String, String> {
    let importer_directory = importer
        .parent()
        .ok_or_else(|| format!("importer has no parent directory: {}", importer.display(),))?;

    let target = target.with_extension("");

    let relative = pathdiff::diff_paths(target, importer_directory).ok_or_else(|| {
        format!(
            "cannot compute relative module path from {}",
            importer.display(),
        )
    })?;

    let mut text = relative.to_string_lossy().replace('\\', "/");

    if !text.starts_with('.') {
        text = format!("./{text}");
    }

    Ok(text)
}

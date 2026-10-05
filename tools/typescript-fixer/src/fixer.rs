use std::{collections::HashSet, fs, path::Path};

use crate::{
    file_issue::FileIssue, fix_action::FixAction, fix_plan::FixPlan, import_issue::ImportIssue,
    module_issue::ModuleIssue, unused_import_issue::UnusedImportIssue,
};

pub(crate) fn plan(
    root: &Path,
    issues: &[FileIssue],
    import_issues: &[ImportIssue],
    unused_import_issues: &[UnusedImportIssue],
    module_issues: &[ModuleIssue],
) -> Result<FixPlan, String> {
    let mut actions = Vec::new();
    let mut renamed_types = HashSet::new();
    let mut redirected_imports = HashSet::new();
    let mut removed_imports = HashSet::new();
    let mut redirected_modules = HashSet::new();

    for issue in unused_import_issues {
        let key = (
            issue.importer.clone(),
            issue.line,
            issue.symbol.clone(),
            issue.whole_import,
        );

        if !removed_imports.insert(key) {
            continue;
        }

        actions.push(FixAction::RemoveUnusedImport {
            importer: issue.importer.clone(),
            symbol: issue.symbol.clone(),
            whole_import: issue.whole_import,
            line: issue.line,
            reason: format!("{} proves this import is unused", issue.code,),
        });
    }

    for issue in module_issues {
        if issue.status != "RESOLVED" {
            continue;
        }

        let Some(target) = &issue.target else {
            continue;
        };

        if !redirected_modules.insert((
            issue.importer.clone(),
            issue.current_module.clone(),
            target.clone(),
        )) {
            continue;
        }

        actions.push(FixAction::RedirectModule {
            importer: issue.importer.clone(),
            current_module: issue.current_module.clone(),
            target: target.clone(),
            line: issue.line,
            reason: format!(
                "{} reports missing module `{}`; analyzer resolved the unique target to {}",
                issue.code,
                issue.current_module,
                target.display(),
            ),
        });
    }

    /*
     * Ces actions proviennent exclusivement d'une
     * résolution effectuée et prouvée par l'analyzer.
     */
    for issue in import_issues {
        if issue.status != "RESOLVED" {
            continue;
        }

        let (Some(kind), Some(target)) = (&issue.kind, &issue.target) else {
            continue;
        };

        if !redirected_imports.insert((
            issue.importer.clone(),
            issue.symbol.clone(),
            issue.current_module.clone(),
            target.clone(),
        )) {
            continue;
        }

        actions.push(
            FixAction::RedirectImport {
                importer:
                    issue.importer
                        .clone(),
                symbol:
                    issue.symbol
                        .clone(),
                current_module:
                    issue.current_module
                        .clone(),
                target:
                    target.clone(),
                kind:
                    kind.clone(),
                line:
                    issue.line,
                reason:
                    format!(
                        "{} reports `{}` is not exported by `{}`; analyzer resolved the unique canonical declaration to {}",
                        issue.code,
                        issue.symbol,
                        issue.current_module,
                        target.display(),
                    ),
            },
        );
    }

    for file in issues {
        for declaration in &file.declarations {
            if !declaration.type_ok
                && renamed_types.insert((
                    declaration.found_name.clone(),
                    declaration.expected_name.clone(),
                ))
            {
                actions.push(FixAction::RenameType {
                    from: declaration.found_name.clone(),
                    to: declaration.expected_name.clone(),
                    reason: format!(
                        "analyzer expects declaration `{}` instead of `{}`",
                        declaration.expected_name, declaration.found_name,
                    ),
                });
            }
        }

        if file.declarations.len() == 1 {
            let declaration = &file.declarations[0];

            if declaration.file_ok {
                continue;
            }

            let target = file.path.with_file_name(&declaration.expected_file_name);

            if filename_matches_expected(&file.path, &declaration.expected_file_name)
                || file_contains_only_declaration(&file.path, declaration.line, &declaration.kind)?
            {
                actions.push(
                    FixAction::RenameFile {
                        from:
                            file.path.clone(),
                        to: target,
                        reason: format!(
                            "the file represents `{}` and its filename is close to the expected canonical filename",
                            declaration.expected_name,
                        ),
                    },
                );
            } else {
                actions.push(
                    FixAction::ExtractDeclaration {
                        source:
                            file.path.clone(),
                        target,
                        kind:
                            declaration
                                .kind
                                .clone(),
                        name:
                            declaration
                                .expected_name
                                .clone(),
                        line:
                            declaration.line,
                        reason: format!(
                            "`{}` is a secondary declaration inside a module whose filename does not identify it",
                            declaration.expected_name,
                        ),
                    },
                );
            }

            continue;
        }

        for declaration in &file.declarations {
            if declaration.file_ok {
                continue;
            }

            actions.push(FixAction::ExtractDeclaration {
                source: file.path.clone(),
                target: file.path.with_file_name(&declaration.expected_file_name),
                kind: declaration.kind.clone(),
                name: declaration.expected_name.clone(),
                line: declaration.line,
                reason: format!(
                    "the source contains {} named declarations; `{}` must live in its own file",
                    file.declarations.len(),
                    declaration.expected_name,
                ),
            });
        }
    }

    validate_plan(root, &actions)?;

    Ok(FixPlan::new(actions))
}

fn validate_plan(root: &Path, actions: &[FixAction]) -> Result<(), String> {
    let mut destinations = HashSet::new();

    for action in actions {
        match action {
            FixAction::RemoveUnusedImport { importer, .. } => {
                if !importer.starts_with(root) {
                    return Err(format!(
                        "importer outside target root: {}",
                        importer.display(),
                    ));
                }
            }

            FixAction::RedirectModule {
                importer, target, ..
            } => {
                if !importer.starts_with(root) {
                    return Err(format!(
                        "importer outside target root: {}",
                        importer.display(),
                    ));
                }

                if !target.starts_with(root) {
                    return Err(format!(
                        "resolved module target outside target root: {}",
                        target.display(),
                    ));
                }
            }

            FixAction::RedirectImport {
                importer, target, ..
            } => {
                if !importer.starts_with(root) {
                    return Err(format!(
                        "importer outside target root: {}",
                        importer.display(),
                    ));
                }

                if !target.starts_with(root) {
                    return Err(format!(
                        "resolved import target outside target root: {}",
                        target.display(),
                    ));
                }
            }

            FixAction::RenameType { .. } => {}

            FixAction::RenameFile { from, to, .. } => {
                if !from.starts_with(root) {
                    return Err(format!("source outside target root: {}", from.display(),));
                }

                validate_destination(to, &mut destinations)?;
            }

            FixAction::ExtractDeclaration { source, target, .. } => {
                if !source.starts_with(root) {
                    return Err(format!("source outside target root: {}", source.display(),));
                }

                validate_destination(target, &mut destinations)?;
            }
        }
    }

    Ok(())
}

fn validate_destination(
    path: &Path,
    destinations: &mut HashSet<std::path::PathBuf>,
) -> Result<(), String> {
    if path.exists() {
        return Err(format!("destination already exists: {}", path.display(),));
    }

    if !destinations.insert(path.to_path_buf()) {
        return Err(format!("duplicate destination: {}", path.display(),));
    }

    Ok(())
}

fn filename_matches_expected(source: &Path, expected_file_name: &str) -> bool {
    let Some(source_stem) = source.file_stem().and_then(|value| value.to_str()) else {
        return false;
    };

    let Some(expected_stem) = Path::new(expected_file_name)
        .file_stem()
        .and_then(|value| value.to_str())
    else {
        return false;
    };

    let source = normalize_filename(source_stem);

    let expected = normalize_filename(expected_stem);

    if source == expected {
        return true;
    }

    let longest = source.chars().count().max(expected.chars().count());

    longest >= 8 && levenshtein(&source, &expected) <= 2
}

fn normalize_filename(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(|character| character.to_lowercase())
        .collect()
}

fn levenshtein(left: &str, right: &str) -> usize {
    let right: Vec<char> = right.chars().collect();

    let mut previous: Vec<usize> = (0..=right.len()).collect();

    for (left_index, left_char) in left.chars().enumerate() {
        let mut current = Vec::with_capacity(right.len() + 1);

        current.push(left_index + 1);

        for (right_index, right_char) in right.iter().enumerate() {
            let insertion = current[right_index] + 1;

            let deletion = previous[right_index + 1] + 1;

            let substitution = previous[right_index] + usize::from(left_char != *right_char);

            current.push(insertion.min(deletion).min(substitution));
        }

        previous = current;
    }

    previous[right.len()]
}

fn file_contains_only_declaration(path: &Path, line: usize, kind: &str) -> Result<bool, String> {
    let text = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display(),))?;

    let (start, end) = declaration_span_from_line(&text, line, kind)?;

    let mut remainder = String::new();

    remainder.push_str(&text[..start]);
    remainder.push_str(&text[end..]);

    Ok(strip_non_runtime_text(&remainder).trim().is_empty())
}

fn strip_non_runtime_text(text: &str) -> String {
    let mut result = String::new();

    let mut skipping_import = false;

    for line in text.lines() {
        let trimmed = line.trim();

        if skipping_import {
            if trimmed.ends_with(';') {
                skipping_import = false;
            }

            continue;
        }

        if trimmed.starts_with("import ") {
            if !trimmed.ends_with(';') {
                skipping_import = true;
            }

            continue;
        }

        if trimmed.is_empty()
            || trimmed.starts_with("//")
            || trimmed.starts_with("/*")
            || trimmed.starts_with('*')
            || trimmed.starts_with("*/")
        {
            continue;
        }

        result.push_str(line);
        result.push('\n');
    }

    result
}

fn declaration_span_from_line(
    text: &str,
    line: usize,
    kind: &str,
) -> Result<(usize, usize), String> {
    let start = line_start_offset(text, line)?;

    match kind {
        "type" => scan_type_end(text, start),

        "interface" | "enum" | "class" => scan_braced_end(text, start),

        other => Err(format!("unsupported declaration kind: {other}",)),
    }
}

fn line_start_offset(text: &str, target_line: usize) -> Result<usize, String> {
    if target_line == 0 {
        return Err("line numbers are 1-based".into());
    }

    if target_line == 1 {
        return Ok(0);
    }

    let mut line = 1;

    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' {
            line += 1;

            if line == target_line {
                return Ok(index + 1);
            }
        }
    }

    Err(format!("line {target_line} is outside source file",))
}

fn scan_braced_end(text: &str, start: usize) -> Result<(usize, usize), String> {
    let bytes = text.as_bytes();

    let mut open = None;

    for index in start..bytes.len() {
        if bytes[index] == b'{' {
            open = Some(index);
            break;
        }
    }

    let open = open.ok_or("declaration opening brace not found")?;

    let mut depth = 0usize;
    let mut index = open;

    while index < bytes.len() {
        match bytes[index] {
            b'{' => {
                depth += 1;
            }

            b'}' => {
                depth -= 1;

                if depth == 0 {
                    let mut end = index + 1;

                    while end < bytes.len() && matches!(bytes[end], b' ' | b'\t' | b'\r') {
                        end += 1;
                    }

                    if end < bytes.len() && bytes[end] == b';' {
                        end += 1;
                    }

                    if end < bytes.len() && bytes[end] == b'\n' {
                        end += 1;
                    }

                    return Ok((start, end));
                }
            }

            b'\'' | b'"' | b'`' => {
                index = skip_string(bytes, index)?;
            }

            _ => {}
        }

        index += 1;
    }

    Err("declaration closing brace not found".into())
}

fn scan_type_end(text: &str, start: usize) -> Result<(usize, usize), String> {
    let bytes = text.as_bytes();

    let mut round = 0usize;
    let mut square = 0usize;
    let mut curly = 0usize;
    let mut angle = 0usize;
    let mut index = start;

    while index < bytes.len() {
        match bytes[index] {
            b'(' => round += 1,

            b')' => {
                round = round.saturating_sub(1);
            }

            b'[' => square += 1,

            b']' => {
                square = square.saturating_sub(1);
            }

            b'{' => curly += 1,

            b'}' => {
                curly = curly.saturating_sub(1);
            }

            b'<' => angle += 1,

            b'>' => {
                angle = angle.saturating_sub(1);
            }

            b';' if round == 0 && square == 0 && curly == 0 && angle == 0 => {
                let mut end = index + 1;

                if end < bytes.len() && bytes[end] == b'\n' {
                    end += 1;
                }

                return Ok((start, end));
            }

            b'\'' | b'"' | b'`' => {
                index = skip_string(bytes, index)?;
            }

            _ => {}
        }

        index += 1;
    }

    Err("type declaration terminator not found".into())
}

fn skip_string(bytes: &[u8], start: usize) -> Result<usize, String> {
    let quote = bytes[start];

    let mut index = start + 1;

    while index < bytes.len() {
        if bytes[index] == b'\\' {
            index += 2;
            continue;
        }

        if bytes[index] == quote {
            return Ok(index);
        }

        index += 1;
    }

    Err("unterminated string while scanning declaration".into())
}

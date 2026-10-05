use std::path::PathBuf;

use crate::{declaration_issue::DeclarationIssue, file_issue::FileIssue};

pub(crate) fn parse(report: &str) -> Result<Vec<FileIssue>, String> {
    let lines: Vec<&str> = report.lines().collect();
    let mut files = Vec::new();
    let mut index = 0;

    while index < lines.len() {
        let header = lines[index].trim();

        if !is_file_header(header) {
            index += 1;
            continue;
        }

        let path = PathBuf::from(header);
        index += 1;

        if index >= lines.len() || lines[index].trim() != "KO" {
            continue;
        }

        index += 1;
        let mut declarations = Vec::new();

        while index < lines.len() {
            let line = lines[index].trim();

            if is_file_header(line) || line == "Résumé" {
                break;
            }

            let Some((kind, fallback_name, line_number)) = declaration_header(line) else {
                index += 1;
                continue;
            };

            index += 1;

            let mut expected_name = None;
            let mut found_name = None;
            let mut expected_file_name = None;
            let mut type_ok = false;
            let mut file_ok = false;

            while index < lines.len() {
                let detail = lines[index].trim();

                if declaration_header(detail).is_some()
                    || is_file_header(detail)
                    || detail.starts_with("KO:")
                    || detail == "Résumé"
                {
                    break;
                }

                if let Some(value) = detail.strip_prefix("type attendu:") {
                    expected_name = Some(value.trim().to_string());
                } else if let Some(value) = detail.strip_prefix("type trouvé :") {
                    found_name = Some(value.trim().to_string());
                } else if let Some(value) = detail.strip_prefix("type        :") {
                    type_ok = value.trim() == "OK";
                } else if let Some(value) = detail.strip_prefix("fichier attendu:") {
                    expected_file_name = Some(value.trim().to_string());
                } else if let Some(value) = detail.strip_prefix("fichier        :") {
                    file_ok = value.trim() == "OK";
                }

                index += 1;
            }

            declarations.push(DeclarationIssue {
                kind,
                line: line_number,
                found_name: found_name.unwrap_or(fallback_name),
                expected_name: expected_name
                    .ok_or_else(|| format!("missing expected type for {}", path.display()))?,
                expected_file_name: expected_file_name
                    .ok_or_else(|| format!("missing expected filename for {}", path.display()))?,
                type_ok,
                file_ok,
            });
        }

        files.push(FileIssue { path, declarations });
    }

    Ok(files)
}

fn is_file_header(line: &str) -> bool {
    if line.starts_with("fichier attendu:") || line.starts_with("fichier trouvé :") {
        return false;
    }

    line.ends_with(".ts") || line.ends_with(".tsx")
}

fn declaration_header(line: &str) -> Option<(String, String, usize)> {
    for kind in ["type", "interface", "enum", "class"] {
        let prefix = format!("{kind} ");

        let Some(rest) = line.strip_prefix(&prefix) else {
            continue;
        };

        let marker = " (ligne ";

        let position = rest.rfind(marker)?;
        let name = rest[..position].trim().to_string();

        let line_number = rest[position + marker.len()..rest.len().checked_sub(1)?]
            .parse()
            .ok()?;

        return Some((kind.to_string(), name, line_number));
    }

    None
}

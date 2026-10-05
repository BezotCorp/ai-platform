use std::{
    error::Error,
    io,
    path::{Path, PathBuf},
    process::Command,
};

use regex::Regex;

use crate::tsc_diagnostic::TscDiagnostic;

pub fn run(target: &Path) -> Result<Vec<TscDiagnostic>, Box<dyn Error>> {
    let project_root = find_project_root(target).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            format!("unable to locate tsconfig.json from {}", target.display(),),
        )
    })?;

    let output = Command::new("pnpm")
        .args(["exec", "tsc", "--noEmit", "--pretty", "false"])
        .current_dir(&project_root)
        .output()?;

    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();

    if !output.stderr.is_empty() {
        if !text.is_empty() && !text.ends_with('\n') {
            text.push('\n');
        }

        text.push_str(&String::from_utf8_lossy(&output.stderr));
    }

    let location = Regex::new(r"^(.+)\((\d+),(\d+)\): error (TS\d+): (.+)$")?;

    let missing_export = Regex::new(r#"Module '"([^"]+)"' has no exported member '([^']+)'"#)?;

    let missing_named_export =
        Regex::new(r#"^'"([^"]+)"' has no exported member named '([^']+)'"#)?;

    let local_not_exported =
        Regex::new(r#"Module '"([^"]+)"' declares '([^']+)' locally, but it is not exported"#)?;

    let unused_value = Regex::new(r#"^'([^']+)' is declared but its value is never read\.$"#)?;

    let unused_declaration = Regex::new(r#"^'([^']+)' is declared but never used\.$"#)?;

    let missing_module =
        Regex::new(r#"^Cannot find module '([^']+)' or its corresponding type declarations\.$"#)?;

    let mut diagnostics = Vec::new();

    for line in text.lines() {
        let Some(captures) = location.captures(line) else {
            continue;
        };

        let file_text = captures
            .get(1)
            .map(|value| value.as_str())
            .unwrap_or_default();

        let candidate = PathBuf::from(file_text);

        let file = if candidate.is_absolute() {
            candidate
        } else {
            project_root.join(candidate)
        };

        let line_number = captures
            .get(2)
            .and_then(|value| value.as_str().parse().ok())
            .unwrap_or(0);

        let column = captures
            .get(3)
            .and_then(|value| value.as_str().parse().ok())
            .unwrap_or(0);

        let code = captures
            .get(4)
            .map(|value| value.as_str().to_string())
            .unwrap_or_default();

        let message = captures
            .get(5)
            .map(|value| value.as_str().to_string())
            .unwrap_or_default();

        let import_problem = parse_import_problem(
            &message,
            &missing_export,
            &missing_named_export,
            &local_not_exported,
        );

        let (current_module, symbol) = import_problem
            .map(|(module, symbol)| (Some(module), Some(symbol)))
            .unwrap_or((None, None));

        let unused_symbol = unused_value
            .captures(&message)
            .or_else(|| unused_declaration.captures(&message))
            .and_then(|captures| captures.get(1))
            .map(|value| value.as_str().to_string());

        let whole_import_unused =
            code == "TS6192" && message == "All imports in import declaration are unused.";

        let missing_module = missing_module
            .captures(&message)
            .and_then(|captures| captures.get(1))
            .map(|value| value.as_str().to_string());

        diagnostics.push(TscDiagnostic {
            file,
            line: line_number,
            column,
            code,
            message,
            symbol,
            current_module,
            unused_symbol,
            whole_import_unused,
            missing_module,
        });
    }

    if !output.status.success() && diagnostics.is_empty() {
        return Err(
            io::Error::other(format!("tsc failed without parseable diagnostics:\n{text}")).into(),
        );
    }

    Ok(diagnostics)
}

fn find_project_root(target: &Path) -> Option<PathBuf> {
    target
        .ancestors()
        .find(|path| path.join("tsconfig.json").is_file())
        .map(Path::to_path_buf)
}

fn parse_import_problem(
    message: &str,
    missing_export: &Regex,
    missing_named_export: &Regex,
    local_not_exported: &Regex,
) -> Option<(String, String)> {
    for pattern in [missing_export, missing_named_export, local_not_exported] {
        let Some(captures) = pattern.captures(message) else {
            continue;
        };

        let module = captures.get(1)?.as_str().to_string();

        let symbol = captures.get(2)?.as_str().to_string();

        return Some((module, symbol));
    }

    None
}

use std::path::PathBuf;

use crate::module_issue::ModuleIssue;

pub(crate) fn parse(report: &str) -> Result<Vec<ModuleIssue>, String> {
    let mut issues = Vec::new();

    for line in report.lines() {
        let Some(payload) = line.strip_prefix("TSC_MODULE|") else {
            continue;
        };

        let fields: Vec<&str> = payload.split('|').collect();

        if fields.len() != 6 {
            return Err(format!("invalid TSC_MODULE analyzer record: {line}"));
        }

        issues.push(ModuleIssue {
            importer: PathBuf::from(fields[0]),
            line: fields[1]
                .parse()
                .map_err(|error| format!("invalid TSC_MODULE line number in `{line}`: {error}"))?,
            code: fields[2].to_string(),
            current_module: fields[3].to_string(),
            target: (!fields[4].is_empty()).then(|| PathBuf::from(fields[4])),
            status: fields[5].to_string(),
        });
    }

    Ok(issues)
}

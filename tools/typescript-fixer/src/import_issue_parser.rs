use std::path::PathBuf;

use crate::import_issue::ImportIssue;

pub(crate) fn parse(report: &str) -> Result<Vec<ImportIssue>, String> {
    let mut issues = Vec::new();

    for line in report.lines() {
        let Some(payload) = line.strip_prefix("TSC_IMPORT|") else {
            continue;
        };

        let fields: Vec<&str> = payload.split('|').collect();

        if fields.len() != 8 {
            return Err(format!("invalid TSC_IMPORT analyzer record: {line}"));
        }

        let kind = (!fields[5].is_empty()).then(|| fields[5].to_string());

        let target = (!fields[6].is_empty()).then(|| PathBuf::from(fields[6]));

        issues.push(ImportIssue {
            importer: PathBuf::from(fields[0]),
            line: fields[1]
                .parse()
                .map_err(|error| format!("invalid TSC_IMPORT line number in `{line}`: {error}"))?,
            code: fields[2].to_string(),
            symbol: fields[3].to_string(),
            current_module: fields[4].to_string(),
            kind,
            target,
            status: fields[7].to_string(),
        });
    }

    Ok(issues)
}

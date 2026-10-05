use std::path::PathBuf;

use crate::unused_import_issue::UnusedImportIssue;

pub(crate) fn parse(report: &str) -> Result<Vec<UnusedImportIssue>, String> {
    let mut issues = Vec::new();

    for line in report.lines() {
        let Some(payload) = line.strip_prefix("TSC_UNUSED|") else {
            continue;
        };

        let fields: Vec<&str> = payload.split('|').collect();

        if fields.len() != 5 {
            return Err(format!("invalid TSC_UNUSED analyzer record: {line}"));
        }

        let mode = fields[4];

        if !matches!(mode, "SYMBOL" | "ALL") {
            return Err(format!("invalid TSC_UNUSED mode in `{line}`"));
        }

        issues.push(UnusedImportIssue {
            importer: PathBuf::from(fields[0]),
            line: fields[1]
                .parse()
                .map_err(|error| format!("invalid TSC_UNUSED line number in `{line}`: {error}"))?,
            code: fields[2].to_string(),
            symbol: (!fields[3].is_empty()).then(|| fields[3].to_string()),
            whole_import: mode == "ALL",
        });
    }

    Ok(issues)
}

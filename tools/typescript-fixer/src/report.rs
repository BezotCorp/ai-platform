#[derive(Debug)]
pub(crate) struct ReportSummary {
    pub(crate) audited: usize,
    pub(crate) ok: usize,
    pub(crate) ko: usize,
    pub(crate) parse_errors: usize,
}

impl ReportSummary {
    pub(crate) fn parse(report: &str) -> Result<Self, String> {
        let audited = value(report, "fichiers audités :")?;

        let ok = value(report, "OK               :")?;

        let ko = value(report, "KO               :")?;

        let parse_errors = value(report, "erreurs parsing  :")?;

        Ok(Self {
            audited,
            ok,
            ko,
            parse_errors,
        })
    }
}

fn value(report: &str, prefix: &str) -> Result<usize, String> {
    report
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix(prefix).map(str::trim))
        .ok_or_else(|| format!("missing analyzer summary field: {prefix}"))?
        .parse()
        .map_err(|error| format!("invalid analyzer summary field {prefix}: {error}"))
}

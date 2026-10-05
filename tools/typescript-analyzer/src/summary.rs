use crate::file_analysis::FileAnalysis;

#[derive(Default)]
pub struct Summary {
    pub audited: usize,
    pub ok: usize,
    pub ko: usize,
    pub parse_errors: usize,
}

impl Summary {
    pub fn record(&mut self, analysis: &FileAnalysis) {
        self.audited += 1;
        if analysis.is_ok() {
            self.ok += 1;
        } else {
            self.ko += 1;
        }
        if analysis.has_parse_errors() {
            self.parse_errors += 1;
        }
    }
}

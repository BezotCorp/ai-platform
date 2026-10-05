use std::time::Duration;

#[derive(Default)]
pub(crate) struct TimingReport {
    pub(crate) analyzer: Duration,
    pub(crate) report_parsing: Duration,
    pub(crate) plan_construction: Duration,
    pub(crate) workspace_loading: Duration,
    pub(crate) preparation: Duration,
    pub(crate) change_collection: Duration,
    pub(crate) debug_rendering: Duration,
    pub(crate) total_before_write: Duration,
}

impl TimingReport {
    pub(crate) fn format_duration(duration: Duration) -> String {
        if duration.as_secs() > 0 {
            return format!("{:.3} s", duration.as_secs_f64(),);
        }

        if duration.as_millis() > 0 {
            return format!("{:.1} ms", duration.as_secs_f64() * 1_000.0,);
        }

        format!("{:.1} µs", duration.as_secs_f64() * 1_000_000.0,)
    }
}

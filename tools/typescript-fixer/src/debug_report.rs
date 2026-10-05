use std::{collections::HashMap, fmt::Write as _, path::Path};

use similar::TextDiff;

use crate::{
    fix_plan::FixPlan, prepared_change::PreparedChange, skipped_action::SkippedAction,
    timing_report::TimingReport,
};

pub(crate) fn render(
    plan: &FixPlan,
    changes: &[PreparedChange],
    skipped: &[SkippedAction],
    timings: &TimingReport,
) -> String {
    let mut output = String::new();

    let skipped_by_index: HashMap<usize, &SkippedAction> = skipped
        .iter()
        .map(|action| (action.action_index, action))
        .collect();

    writeln!(output, "SUMMARY").unwrap();

    writeln!(output, "=======").unwrap();

    writeln!(output, "planned:       {}", plan.len(),).unwrap();

    writeln!(
        output,
        "prepared:      {}",
        plan.len().saturating_sub(skipped.len(),),
    )
    .unwrap();

    writeln!(output, "skipped:       {}", skipped.len(),).unwrap();

    writeln!(output, "changed files: {}", changes.len(),).unwrap();

    render_timings(&mut output, timings);

    writeln!(output, "\nPLAN").unwrap();

    writeln!(output, "====").unwrap();

    for (index, action) in plan.actions().iter().enumerate() {
        writeln!(output, "\nACTION #{:03}", index + 1,).unwrap();

        writeln!(output, "{}", action.describe(),).unwrap();

        writeln!(output, "reason: {}", action.reason(),).unwrap();

        if let Some(skipped_action) = skipped_by_index.get(&index) {
            writeln!(output, "status: SKIPPED").unwrap();

            writeln!(output, "skip reason: {}", skipped_action.reason,).unwrap();
        } else {
            writeln!(output, "status: PREPARED").unwrap();
        }
    }

    if !skipped.is_empty() {
        writeln!(output, "\n\nSKIPPED").unwrap();

        writeln!(output, "=======").unwrap();

        for action in skipped {
            writeln!(output, "\nACTION #{:03}", action.action_index + 1,).unwrap();

            writeln!(output, "{}", action.description,).unwrap();

            writeln!(output, "reason: {}", action.reason,).unwrap();
        }
    }

    writeln!(output, "\n\nPREPARED CHANGES").unwrap();

    writeln!(output, "================").unwrap();

    for change in changes {
        render_change(&mut output, change);
    }

    output
}

pub(crate) fn render_final_timings(timings: &TimingReport) -> String {
    let mut output = String::new();

    writeln!(output, "\n\nFINAL TIMINGS").unwrap();

    writeln!(output, "=============").unwrap();

    timing_line(&mut output, "debug rendering", timings.debug_rendering);

    timing_line(
        &mut output,
        "total before write",
        timings.total_before_write,
    );

    output
}

fn render_timings(output: &mut String, timings: &TimingReport) {
    writeln!(output, "\nTIMINGS").unwrap();

    writeln!(output, "=======").unwrap();

    timing_line(output, "analyzer process", timings.analyzer);

    timing_line(output, "report parsing", timings.report_parsing);

    timing_line(output, "plan construction", timings.plan_construction);

    timing_line(output, "workspace loading", timings.workspace_loading);

    timing_line(output, "transform preparation", timings.preparation);

    timing_line(output, "change collection", timings.change_collection);
}

fn timing_line(output: &mut String, label: &str, duration: std::time::Duration) {
    writeln!(
        output,
        "{label:<24} {}",
        TimingReport::format_duration(duration,),
    )
    .unwrap();
}

fn render_change(output: &mut String, change: &PreparedChange) {
    let before_path = display_path(change.before_path.as_deref(), "/dev/null");

    let after_path = display_path(change.after_path.as_deref(), "/dev/null");

    writeln!(output, "\n---").unwrap();

    match (&change.before_path, &change.after_path) {
        (Some(before), Some(after)) if before != after => {
            writeln!(output, "RENAME {} -> {}", before.display(), after.display(),).unwrap();
        }

        (None, Some(after)) => {
            writeln!(output, "CREATE {}", after.display(),).unwrap();
        }

        (Some(before), None) => {
            writeln!(output, "DELETE {}", before.display(),).unwrap();
        }

        _ => {
            writeln!(output, "MODIFY {}", after_path,).unwrap();
        }
    }

    let before = change.before.as_deref().unwrap_or("");

    let after = change.after.as_deref().unwrap_or("");

    if before == after {
        writeln!(output, "content unchanged").unwrap();

        return;
    }

    let diff = TextDiff::from_lines(before, after);

    let unified = diff
        .unified_diff()
        .context_radius(3)
        .header(&before_path, &after_path)
        .to_string();

    output.push_str(&unified);
}

fn display_path(path: Option<&Path>, fallback: &str) -> String {
    path.map(|path| path.display().to_string())
        .unwrap_or_else(|| fallback.to_string())
}

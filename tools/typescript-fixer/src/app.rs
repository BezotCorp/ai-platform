use std::{collections::HashMap, fs, process::Command, time::Instant};

use crate::{
    analyzer_process, cli::Cli, debug_report, fixer, import_issue_parser, module_issue_parser,
    report_parser, timing_report::TimingReport, unused_import_issue_parser, workspace::Workspace,
};

pub(crate) fn run() -> Result<(), String> {
    let cli = Cli::parse()?;

    let total_start = Instant::now();

    let mut timings = TimingReport::default();

    let started = Instant::now();

    let analyzer_report = analyzer_process::run(&cli.target)?;

    timings.analyzer = started.elapsed();

    let started = Instant::now();

    let issues = report_parser::parse(&analyzer_report)?;

    let import_issues = import_issue_parser::parse(&analyzer_report)?;

    let unused_import_issues = unused_import_issue_parser::parse(&analyzer_report)?;

    let module_issues = module_issue_parser::parse(&analyzer_report)?;

    timings.report_parsing = started.elapsed();

    let started = Instant::now();

    let plan = fixer::plan(
        &cli.target,
        &issues,
        &import_issues,
        &unused_import_issues,
        &module_issues,
    )?;

    timings.plan_construction = started.elapsed();

    let started = Instant::now();

    let mut workspace = Workspace::load(&cli.target)?;

    timings.workspace_loading = started.elapsed();

    let started = Instant::now();

    let skipped = workspace.prepare(&plan)?;

    timings.preparation = started.elapsed();

    let started = Instant::now();

    let changes = workspace.changes();

    timings.change_collection = started.elapsed();

    if cli.debug {
        let render_started = Instant::now();

        let mut report = debug_report::render(&plan, &changes, &skipped, &timings);

        timings.debug_rendering = render_started.elapsed();

        timings.total_before_write = total_start.elapsed();

        report.push_str(&debug_report::render_final_timings(&timings));

        emit_debug_report(&cli, &report)?;
    } else {
        print_normal_summary(&plan, &skipped, changes.len());
    }

    if cli.dry_run {
        println!("dry-run : aucune écriture");

        return Ok(());
    }

    workspace.apply()?;

    println!("{} fichier(s) écrit(s)", changes.len(),);

    Ok(())
}

fn print_normal_summary(
    plan: &crate::fix_plan::FixPlan,
    skipped: &[crate::skipped_action::SkippedAction],
    changed_files: usize,
) {
    let skipped_by_index: HashMap<usize, _> = skipped
        .iter()
        .map(|action| (action.action_index, action))
        .collect();

    for (index, action) in plan.actions().iter().enumerate() {
        if let Some(skipped_action) = skipped_by_index.get(&index) {
            println!("SKIPPED      {}", action.describe(),);

            println!("             {}", skipped_action.reason,);

            continue;
        }

        println!("{}", action.describe(),);
    }

    println!();

    println!("{} modification(s) prévue(s)", plan.len(),);

    println!("{} préparée(s)", plan.len().saturating_sub(skipped.len(),),);

    println!("{} ignorée(s)", skipped.len(),);

    println!("{} fichier(s) réellement modifié(s)", changed_files,);
}

fn emit_debug_report(cli: &Cli, report: &str) -> Result<(), String> {
    if let Some(output) = &cli.output {
        fs::write(output, report).map_err(|error| {
            format!("failed to write debug report {}: {error}", output.display(),)
        })?;

        println!("debug report: {}", output.display(),);

        if let Some(program) = &cli.open_with {
            Command::new(program).arg(output).spawn().map_err(|error| {
                format!(
                    "failed to open {} with {}: {error}",
                    output.display(),
                    program,
                )
            })?;
        }

        return Ok(());
    }

    print!("{report}");

    Ok(())
}

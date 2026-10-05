mod analyzer_process;
mod app;
mod cli;
mod debug_report;
mod declaration_issue;
mod file_issue;
mod fix_action;
mod fix_plan;
mod fixer;
mod import_issue;
mod import_issue_parser;
mod module_issue;
mod module_issue_parser;
mod prepared_change;
mod report_parser;
mod skipped_action;
mod timing_report;
mod unused_import_issue;
mod unused_import_issue_parser;
mod workspace;

use std::process;

fn main() {
    if let Err(error) = app::run() {
        eprintln!("{error}");
        process::exit(1);
    }
}

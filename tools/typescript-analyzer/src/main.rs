mod analyzer;
mod app;
mod declaration;
mod declaration_kind;
mod file_analysis;
mod jsx_detector;
mod module_resolver;
mod reporter;
mod summary;
mod symbol_resolver;
mod traversal;
mod tsc_diagnostic;
mod tsc_runner;
use std::process::ExitCode;

fn main() -> ExitCode {
    match app::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

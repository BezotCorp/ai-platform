use std::{env, error::Error, path::PathBuf};

use crate::{
    analyzer::analyze_file,
    module_resolver,
    reporter::{print_analysis, print_summary},
    summary::Summary,
    symbol_resolver,
    traversal::collect_typescript_files,
    tsc_runner,
};

pub fn run() -> Result<(), Box<dyn Error>> {
    let arguments = env::args().skip(1).collect::<Vec<_>>();

    if arguments.len() > 1 {
        return Err("usage: typescript-analyzer [path]".to_string().into());
    }

    let root = arguments
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));

    if !root.exists() {
        return Err(format!("path does not exist: {}", root.display(),).into());
    }

    let files = collect_typescript_files(&root)?;

    let mut analyses = Vec::with_capacity(files.len());

    let mut summary = Summary::default();

    for file in files {
        analyses.push(analyze_file(&file)?);
    }

    for analysis in &analyses {
        if !analysis.is_relevant() {
            continue;
        }

        print_analysis(analysis);

        summary.record(analysis);
    }

    print_summary(&summary);

    let diagnostics = tsc_runner::run(&root)?;

    println!();
    println!("TSC");
    println!("  erreurs : {}", diagnostics.len(),);

    for diagnostic in diagnostics {
        println!(
            "  {}:{}:{} {} {}",
            diagnostic.file.display(),
            diagnostic.line,
            diagnostic.column,
            diagnostic.code,
            diagnostic.message,
        );

        if diagnostic.whole_import_unused {
            println!(
                "TSC_UNUSED|{}|{}|{}||ALL",
                diagnostic.file.display(),
                diagnostic.line,
                diagnostic.code,
            );

            continue;
        }

        if let Some(unused_symbol) = diagnostic.unused_symbol.as_deref()
            && matches!(diagnostic.code.as_str(), "TS6133" | "TS6196")
        {
            println!(
                "TSC_UNUSED|{}|{}|{}|{}|SYMBOL",
                diagnostic.file.display(),
                diagnostic.line,
                diagnostic.code,
                unused_symbol,
            );

            continue;
        }

        if let Some(missing_module) = diagnostic.missing_module.as_deref() {
            let candidates = module_resolver::resolve(&diagnostic.file, missing_module, &analyses);

            match candidates.as_slice() {
                [] => {
                    println!("    résolution module `{missing_module}` : NOT_FOUND",);

                    println!(
                        "TSC_MODULE|{}|{}|{}|{}||NOT_FOUND",
                        diagnostic.file.display(),
                        diagnostic.line,
                        diagnostic.code,
                        missing_module,
                    );
                }

                [target] => {
                    println!(
                        "    résolution module `{missing_module}` : `{}`",
                        target.display(),
                    );

                    println!(
                        "TSC_MODULE|{}|{}|{}|{}|{}|RESOLVED",
                        diagnostic.file.display(),
                        diagnostic.line,
                        diagnostic.code,
                        missing_module,
                        target.display(),
                    );
                }

                candidates => {
                    println!(
                        "    résolution module `{missing_module}` : AMBIGUOUS ({} candidats)",
                        candidates.len(),
                    );

                    println!(
                        "TSC_MODULE|{}|{}|{}|{}||AMBIGUOUS",
                        diagnostic.file.display(),
                        diagnostic.line,
                        diagnostic.code,
                        missing_module,
                    );
                }
            }

            continue;
        }

        let (Some(symbol), Some(current_module)) = (
            diagnostic.symbol.as_deref(),
            diagnostic.current_module.as_deref(),
        ) else {
            continue;
        };

        let candidates = symbol_resolver::resolve(&analyses, symbol);

        match candidates.as_slice() {
            [] => {
                println!("    résolution import `{symbol}` : NOT_FOUND",);

                println!(
                    "TSC_IMPORT|{}|{}|{}|{}|{}|||NOT_FOUND",
                    diagnostic.file.display(),
                    diagnostic.line,
                    diagnostic.code,
                    symbol,
                    current_module,
                );
            }

            [(target, kind)] => {
                println!(
                    "    résolution import `{symbol}` : `{}` ({kind})",
                    target.display(),
                );

                println!(
                    "TSC_IMPORT|{}|{}|{}|{}|{}|{}|{}|RESOLVED",
                    diagnostic.file.display(),
                    diagnostic.line,
                    diagnostic.code,
                    symbol,
                    current_module,
                    kind,
                    target.display(),
                );
            }

            candidates => {
                println!(
                    "    résolution import `{symbol}` : AMBIGUOUS ({} candidats)",
                    candidates.len(),
                );

                println!(
                    "TSC_IMPORT|{}|{}|{}|{}|{}|||AMBIGUOUS",
                    diagnostic.file.display(),
                    diagnostic.line,
                    diagnostic.code,
                    symbol,
                    current_module,
                );
            }
        }
    }

    Ok(())
}

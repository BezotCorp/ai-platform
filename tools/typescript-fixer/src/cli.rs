use std::{env, path::PathBuf};

pub(crate) struct Cli {
    pub(crate) target: PathBuf,
    pub(crate) dry_run: bool,
    pub(crate) debug: bool,
    pub(crate) output: Option<PathBuf>,
    pub(crate) open_with: Option<String>,
}

impl Cli {
    pub(crate) fn parse() -> Result<Self, String> {
        let mut target = None;
        let mut dry_run = false;
        let mut debug = false;
        let mut output = None;
        let mut open_with = None;

        let mut args = env::args().skip(1);

        while let Some(arg) = args.next() {
            match arg.as_str() {
                "--dry-run" => dry_run = true,
                "--debug" => debug = true,

                "--output" => {
                    output = Some(PathBuf::from(
                        args.next().ok_or("--output requires a path")?,
                    ));
                }

                "--open-with" => {
                    open_with = Some(args.next().ok_or("--open-with requires a program")?);
                }

                value if value.starts_with('-') => {
                    return Err(format!("unknown option: {value}"));
                }

                value => {
                    if target.is_some() {
                        return Err(usage().into());
                    }

                    target = Some(PathBuf::from(value));
                }
            }
        }

        let target = target.ok_or_else(|| usage().to_string())?;

        if !target.is_dir() {
            return Err(format!("not a directory: {}", target.display()));
        }

        if output.is_some() && !debug {
            return Err("--output requires --debug".into());
        }

        if open_with.is_some() && output.is_none() {
            return Err("--open-with requires --output".into());
        }

        Ok(Self {
            target,
            dry_run,
            debug,
            output,
            open_with,
        })
    }
}

fn usage() -> &'static str {
    "usage: typescript-fixer [--dry-run] [--debug] [--output <path>] [--open-with <program>] <path>"
}

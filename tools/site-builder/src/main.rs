mod build_diff;
mod build_engine;
mod build_output;
mod component_tree;
mod language;
mod language_region;
mod public_path;
mod region;
mod render_engine;
mod root_path;
mod slug;
mod source;
mod source_path;
mod target_path;

use std::path::Path;

use anyhow::Result;

use crate::build_diff::BuildDiff;
use crate::build_engine::BuildContext;
use crate::source_path::SourcePath;
use crate::target_path::TargetPath;

static SOURCE_ROOT: &str = "data/site";
static TARGET_ROOT: &str = "site";

fn main() -> Result<()> {
    let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("site-builder must live in tools/site-builder");

    let source_root = repository_root.join(Path::new(SOURCE_ROOT));

    let target_root = repository_root.join(Path::new(TARGET_ROOT));

    let mut context =
        BuildContext::load(SourcePath::from(source_root), TargetPath::from(target_root))?;

    context.assemble()?;
    context.validate()?;

    let output = context.build()?;

    let public_root = context.source.as_path().join("public");

    output.validate(&public_root)?;

    let target_root = context.target.as_path();

    let diff = output.into_diff(&public_root, target_root)?;

    if diff.is_empty() {
        println!("site is already up to date");
        return Ok(());
    }

    println!("site diff: {} change(s)", diff.len(),);

    diff.apply()?;

    BuildDiff::remove_empty_directories(target_root)?;

    Ok(())
}

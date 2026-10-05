use std::{path::Path, process::Command};

pub(crate) fn run(target: &Path) -> Result<String, String> {
    let output = Command::new("cargo")
        .args(["run", "-q", "-p", "typescript-analyzer", "--"])
        .arg(target)
        .output()
        .map_err(|error| format!("failed to launch typescript-analyzer: {error}"))?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned());
    }

    String::from_utf8(output.stdout)
        .map_err(|error| format!("typescript-analyzer returned invalid UTF-8: {error}"))
}

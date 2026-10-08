use anyhow::{Result, anyhow};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct RecipeFile {
    pub content: String,
    pub parent_dir: PathBuf,
    pub file_path: PathBuf,
}

pub fn read_recipe_file<P: AsRef<Path>>(recipe_path: P) -> Result<RecipeFile> {
    let raw_path = recipe_path.as_ref();
    let path = convert_path_with_tilde_expansion(raw_path);
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let parent_dir = parent.canonicalize().map_err(|e| {
        anyhow!(
            "Failed to resolve recipe directory {}: {}",
            parent.display(),
            e
        )
    })?;
    let file_name = path
        .file_name()
        .ok_or_else(|| anyhow!("Recipe path has no file name: {}", path.display()))?;
    let content = crate::skills::read_source_file(&parent_dir, Path::new(file_name))
        .map_err(|e| anyhow!("Failed to read recipe file {}: {}", path.display(), e))?;
    let file_path = parent_dir.join(file_name);

    Ok(RecipeFile {
        content,
        parent_dir,
        file_path,
    })
}

fn convert_path_with_tilde_expansion(path: &Path) -> PathBuf {
    if let Some(path_str) = path.to_str() {
        // Handle exact "~" (Windows only to avoid changing behavior on Unix)
        if cfg!(windows) && path_str == "~" {
            if let Some(home_dir) = dirs::home_dir() {
                return home_dir;
            }
        }
        // Handle Unix-style "~/..."
        if let Some(stripped) = path_str.strip_prefix("~/") {
            if let Some(home_dir) = dirs::home_dir() {
                return home_dir.join(stripped);
            }
        }
        // Handle Windows-style "~\\..." (Windows only)
        #[cfg(windows)]
        if let Some(stripped) = path_str.strip_prefix("~\\") {
            if let Some(home_dir) = dirs::home_dir() {
                return home_dir.join(stripped);
            }
        }
    }
    PathBuf::from(path)
}

pub fn read_parameter_file_content<P: AsRef<Path>>(file_path: P) -> Result<String> {
    let raw_path = file_path.as_ref();
    let path = convert_path_with_tilde_expansion(raw_path);

    let content = fs::read_to_string(&path)
        .map_err(|e| anyhow!("Failed to read parameter file {}: {}", path.display(), e))?;

    Ok(content)
}

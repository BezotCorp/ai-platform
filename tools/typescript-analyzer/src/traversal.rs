use std::{
    fs, io,
    path::{Path, PathBuf},
};

const IGNORED_DIRECTORIES: &[&str] = &[
    ".git",
    ".next",
    "build",
    "coverage",
    "dist",
    "node_modules",
    "out",
    "target",
];

const IGNORED_FILES: &[&str] = &["index.ts", "index.tsx", "main.ts", "main.tsx"];

pub fn collect_typescript_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    collect(root, &mut files)?;
    files.sort();
    Ok(files)
}

fn collect(path: &Path, files: &mut Vec<PathBuf>) -> io::Result<()> {
    if path.is_file() {
        if is_typescript_file(path) {
            files.push(path.to_path_buf());
        }
        return Ok(());
    }
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            if is_ignored_directory(&path) {
                continue;
            }
            collect(&path, files)?;
            continue;
        }
        if is_typescript_file(&path) {
            files.push(path);
        }
    }
    Ok(())
}

fn is_typescript_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    if IGNORED_FILES.contains(&file_name) {
        return false;
    }
    if file_name.ends_with(".d.ts")
        || file_name.ends_with(".test.ts")
        || file_name.ends_with(".test.tsx")
    {
        return false;
    }
    file_name.ends_with(".ts") || file_name.ends_with(".tsx")
}

fn is_ignored_directory(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| IGNORED_DIRECTORIES.contains(&name))
}

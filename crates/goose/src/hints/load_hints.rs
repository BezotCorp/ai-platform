use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

use crate::{config::paths::Paths, hints::import_files::read_referenced_files};
pub const GOOSE_HINTS_FILENAME: &str = ".goosehints";
pub const AGENTS_MD_FILENAME: &str = "AGENTS.md";

pub fn get_context_filenames() -> Vec<String> {
    use crate::config::Config;
    Config::global()
        .get_param::<Vec<String>>("CONTEXT_FILE_NAMES")
        .unwrap_or_else(|_| {
            vec![
                GOOSE_HINTS_FILENAME.to_string(),
                AGENTS_MD_FILENAME.to_string(),
            ]
        })
}

#[derive(Default)]
pub struct SubdirectoryHintTracker {
    loaded_dirs: HashSet<PathBuf>,
    pending_dirs: Vec<PathBuf>,
    hints_filenames: Vec<String>,
}

impl SubdirectoryHintTracker {
    pub fn new() -> Self {
        Self {
            loaded_dirs: HashSet::new(),
            pending_dirs: Vec::new(),
            hints_filenames: get_context_filenames(),
        }
    }

    pub fn record_tool_arguments(
        &mut self,
        arguments: &Option<serde_json::Map<String, serde_json::Value>>,
        working_dir: &Path,
    ) {
        let args = match arguments.as_ref() {
            Some(a) => a,
            None => return,
        };

        if let Some(path_str) = args.get("path").and_then(|v| v.as_str()) {
            if let Some(dir) = resolve_to_parent_dir(path_str, working_dir) {
                self.pending_dirs.push(dir);
            }
        }

        if let Some(cmd) = args.get("command").and_then(|v| v.as_str()) {
            for token in shell_words::split(cmd).unwrap_or_default() {
                if token.starts_with('-') {
                    continue;
                }
                if token.contains(std::path::MAIN_SEPARATOR) || token.contains('.') {
                    if let Some(dir) = resolve_to_parent_dir(&token, working_dir) {
                        self.pending_dirs.push(dir);
                    }
                }
            }
        }
    }

    pub fn load_new_hints(&mut self, working_dir: &Path) -> Vec<(String, String)> {
        let pending = std::mem::take(&mut self.pending_dirs);
        if pending.is_empty() {
            return Vec::new();
        }

        let Ok(working_dir) = working_dir.canonicalize() else {
            return Vec::new();
        };

        let mut results = Vec::new();
        for dir in pending {
            let Ok(dir) = dir.canonicalize() else {
                continue;
            };
            if !dir.starts_with(&working_dir) || dir == working_dir {
                continue;
            }
            if self.loaded_dirs.contains(&dir) {
                continue;
            }
            if let Some(content) =
                load_hints_from_directory(&dir, &working_dir, &self.hints_filenames)
            {
                let key = format!("subdir_hints:{}", dir.display());
                results.push((key, content));
            }
            self.loaded_dirs.insert(dir);
        }
        results
    }
}

fn resolve_to_parent_dir(token: &str, working_dir: &Path) -> Option<PathBuf> {
    let path = Path::new(token);
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        working_dir.join(path)
    };
    resolved.parent().map(|d| d.to_path_buf())
}

fn load_hints_from_directory(
    directory: &Path,
    working_dir: &Path,
    hints_filenames: &[String],
) -> Option<String> {
    if !directory.is_dir() || !directory.is_absolute() {
        return None;
    }

    if !directory.starts_with(working_dir) || directory == working_dir {
        return None;
    }

    let git_root = find_git_root(working_dir);
    let import_boundary = git_root.unwrap_or(working_dir);
    let gitignore = Gitignore::empty();

    let mut directories: Vec<PathBuf> = directory
        .ancestors()
        .take_while(|d| d.starts_with(working_dir) && *d != working_dir)
        .map(|d| d.to_path_buf())
        .collect();
    directories.reverse();

    let mut contents = Vec::new();
    for dir in &directories {
        for hints_filename in hints_filenames {
            let hints_path = dir.join(hints_filename);
            if hints_path.is_file() {
                let mut visited = HashSet::new();
                let expanded = read_referenced_files(
                    &hints_path,
                    import_boundary,
                    &mut visited,
                    0,
                    &gitignore,
                );
                if !expanded.is_empty() {
                    contents.push(expanded);
                }
            }
        }
    }

    if contents.is_empty() {
        None
    } else {
        Some(format!(
            "### Subdirectory Hints ({})\n{}",
            directory.display(),
            contents.join("\n")
        ))
    }
}

fn find_git_root(start_dir: &Path) -> Option<&Path> {
    let mut check_dir = start_dir;

    loop {
        if check_dir.join(".git").exists() {
            return Some(check_dir);
        }
        if let Some(parent) = check_dir.parent() {
            check_dir = parent;
        } else {
            break;
        }
    }

    None
}

fn get_local_directories(git_root: Option<&Path>, cwd: &Path) -> Vec<PathBuf> {
    match git_root {
        Some(git_root) => {
            let mut directories = Vec::new();
            let mut current_dir = cwd;

            loop {
                directories.push(current_dir.to_path_buf());
                if current_dir == git_root {
                    break;
                }
                if let Some(parent) = current_dir.parent() {
                    current_dir = parent;
                } else {
                    break;
                }
            }
            directories.reverse();
            directories
        }
        None => vec![cwd.to_path_buf()],
    }
}

/// Build a `Gitignore` that includes `.gitignore` files from the git root
/// down to `cwd`, matching git's hierarchical ignore semantics. When there
/// is no git root, only `cwd/.gitignore` is loaded.
pub fn build_gitignore(cwd: &Path) -> Gitignore {
    let git_root = find_git_root(cwd);
    let directories = get_local_directories(git_root, cwd);

    let mut builder = GitignoreBuilder::new(cwd);
    for dir in &directories {
        let gitignore_path = dir.join(".gitignore");
        if gitignore_path.is_file() {
            builder.add(&gitignore_path);
        }
    }
    builder.build().unwrap_or_else(|_| {
        GitignoreBuilder::new(cwd)
            .build()
            .expect("Failed to build default gitignore")
    })
}

pub fn load_hint_files(
    cwd: &Path,
    hints_filenames: &[String],
    ignore_patterns: &Gitignore,
) -> String {
    let mut global_hints_contents = Vec::with_capacity(hints_filenames.len());
    let mut local_hints_contents = Vec::with_capacity(hints_filenames.len());

    let mut global_hints_paths: Vec<PathBuf> = hints_filenames
        .iter()
        .map(|name| Paths::in_config_dir(name))
        .collect();
    if hints_filenames
        .iter()
        .any(|name| name == AGENTS_MD_FILENAME)
    {
        global_hints_paths.push(Paths::in_agents_home_dir(AGENTS_MD_FILENAME));
    }

    for global_hints_path in &global_hints_paths {
        if global_hints_path.is_file() {
            let mut visited = HashSet::new();
            let hints_dir = global_hints_path.parent().unwrap();
            let global_ignore_patterns = GitignoreBuilder::new(hints_dir)
                .build()
                .unwrap_or_else(|_| Gitignore::empty());
            let expanded_content = read_referenced_files(
                global_hints_path,
                hints_dir,
                &mut visited,
                0,
                &global_ignore_patterns,
            );
            if !expanded_content.is_empty() {
                global_hints_contents.push(expanded_content);
            }
        }
    }
    let git_root = find_git_root(cwd);
    let local_directories = get_local_directories(git_root, cwd);

    let import_boundary = git_root.unwrap_or(cwd);

    for directory in &local_directories {
        for hints_filename in hints_filenames {
            let hints_path = directory.join(hints_filename);
            if hints_path.is_file() {
                let mut visited = HashSet::new();
                let expanded_content = read_referenced_files(
                    &hints_path,
                    import_boundary,
                    &mut visited,
                    0,
                    ignore_patterns,
                );
                if !expanded_content.is_empty() {
                    local_hints_contents.push(expanded_content);
                }
            }
        }
    }

    let mut hints = String::new();
    if !global_hints_contents.is_empty() {
        hints.push_str("\n### Global Hints\nThese are my global goose hints.\n");
        hints.push_str(&global_hints_contents.join("\n"));
    }

    if !local_hints_contents.is_empty() {
        if !hints.is_empty() {
            hints.push_str("\n\n");
        }
        hints.push_str(
            "### Project Hints\nThese are hints for working on the project in this directory.\n",
        );
        hints.push_str(&local_hints_contents.join("\n"));
    }

    hints
}

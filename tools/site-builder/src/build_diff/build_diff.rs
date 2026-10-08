use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use walkdir::WalkDir;

use crate::build_diff::BuildChange;

pub(crate) struct BuildDiff {
    changes: Vec<BuildChange>,
}

impl BuildDiff {
    pub(crate) fn new(changes: Vec<BuildChange>) -> Self {
        Self { changes }
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.changes.is_empty()
    }

    pub(crate) fn len(&self) -> usize {
        self.changes.len()
    }

    pub(crate) fn apply(self) -> Result<()> {
        for change in self.changes {
            match change {
                BuildChange::WriteGenerated { target, file } => {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }

                    fs::write(&target, file.content).with_context(|| {
                        format!("failed to write generated file {}", target.display(),)
                    })?;
                }

                BuildChange::CopyPublic { target, file } => {
                    if let Some(parent) = target.parent() {
                        fs::create_dir_all(parent)?;
                    }

                    fs::copy(file.source(), &target).with_context(|| {
                        format!(
                            "failed to copy {} to {}",
                            file.source().display(),
                            target.display(),
                        )
                    })?;
                }

                BuildChange::Remove { target } => {
                    if target.exists() {
                        fs::remove_file(&target)
                            .with_context(|| format!("failed to remove {}", target.display(),))?;
                    }
                }
            }
        }

        Ok(())
    }

    pub(crate) fn remove_empty_directories(target_root: &Path) -> Result<()> {
        if !target_root.exists() {
            return Ok(());
        }

        let directories = WalkDir::new(target_root)
            .contents_first(true)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_dir() && entry.path() != target_root)
            .map(|entry| entry.into_path())
            .collect::<Vec<_>>();

        for directory in directories {
            if directory.read_dir()?.next().is_none() {
                fs::remove_dir(directory)?;
            }
        }

        Ok(())
    }
}

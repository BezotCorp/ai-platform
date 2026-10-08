use std::collections::BTreeSet;
use std::fs;
use std::io::{BufReader, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use walkdir::WalkDir;

use crate::build_diff::{BuildChange, BuildDiff};
use crate::build_output::{GeneratedFile, PublicFile};

pub(crate) struct BuildOutput {
    generated_files: Vec<GeneratedFile>,
    public_files: Vec<PublicFile>,
}

impl BuildOutput {
    pub(crate) fn new(generated_files: Vec<GeneratedFile>, public_files: Vec<PublicFile>) -> Self {
        Self {
            generated_files,
            public_files,
        }
    }

    pub(crate) fn validate(&self, public_root: &Path) -> Result<()> {
        let mut paths = BTreeSet::<PathBuf>::new();

        for file in &self.generated_files {
            validate_relative_path(&file.path)?;

            if !paths.insert(file.path.clone()) {
                bail!("duplicate generated path: {}", file.path.display(),);
            }

            if file.content.is_empty() {
                bail!("generated file is empty: {}", file.path.display(),);
            }

            if file.path.extension().and_then(|value| value.to_str()) == Some("html") {
                validate_html(&file.path, &file.content)?;
            }
        }

        for file in &self.public_files {
            if !file.source().is_file() {
                bail!("public source does not exist: {}", file.source().display(),);
            }

            let relative = file.relative_path(public_root)?;

            validate_relative_path(relative)?;

            if !paths.insert(relative.to_path_buf()) {
                bail!("output path collision: {}", relative.display(),);
            }
        }

        for required in [
            Path::new("index.html"),
            Path::new("404.html"),
            Path::new("robots.txt"),
            Path::new("sitemap.xml"),
            Path::new("CNAME"),
            Path::new(".well-known/oauth-cimd"),
        ] {
            if !paths.contains(required) {
                bail!("required output is missing: {}", required.display(),);
            }
        }

        Ok(())
    }

    pub(crate) fn into_diff(self, public_root: &Path, target_root: &Path) -> Result<BuildDiff> {
        let expected = self.expected_paths(public_root)?;

        let mut changes = Vec::new();

        for file in self.generated_files {
            let target = target_root.join(&file.path);

            if generated_file_differs(&file, &target)? {
                changes.push(BuildChange::WriteGenerated { target, file });
            }
        }

        for file in self.public_files {
            let relative = file.relative_path(public_root)?.to_path_buf();

            let target = target_root.join(relative);

            if public_file_differs(file.source(), &target)? {
                changes.push(BuildChange::CopyPublic { target, file });
            }
        }

        if target_root.exists() {
            for entry in WalkDir::new(target_root).follow_links(false) {
                let entry = entry?;

                if entry.file_type().is_symlink() {
                    bail!("symlink found in site output: {}", entry.path().display(),);
                }

                if !entry.file_type().is_file() {
                    continue;
                }

                let relative = entry.path().strip_prefix(target_root)?;

                if !expected.contains(relative) {
                    changes.push(BuildChange::Remove {
                        target: entry.path().to_path_buf(),
                    });
                }
            }
        }

        Ok(BuildDiff::new(changes))
    }

    fn expected_paths(&self, public_root: &Path) -> Result<BTreeSet<PathBuf>> {
        let mut paths = BTreeSet::new();

        for file in &self.generated_files {
            paths.insert(file.path.clone());
        }

        for file in &self.public_files {
            paths.insert(file.relative_path(public_root)?.to_path_buf());
        }

        Ok(paths)
    }
}

fn generated_file_differs(generated: &GeneratedFile, target: &Path) -> Result<bool> {
    let current = match fs::read(target) {
        Ok(current) => current,

        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(true);
        }

        Err(error) => {
            return Err(error).with_context(|| format!("failed to read {}", target.display(),));
        }
    };

    Ok(current != generated.content.as_bytes())
}

fn public_file_differs(source: &Path, target: &Path) -> Result<bool> {
    let source_metadata = fs::metadata(source)
        .with_context(|| format!("failed to read metadata for {}", source.display(),))?;

    let target_metadata = match fs::metadata(target) {
        Ok(metadata) => metadata,

        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(true);
        }

        Err(error) => {
            return Err(error)
                .with_context(|| format!("failed to read metadata for {}", target.display(),));
        }
    };

    if !target_metadata.is_file() {
        return Ok(true);
    }

    if source_metadata.len() != target_metadata.len() {
        return Ok(true);
    }

    files_differ(source, target)
}

fn files_differ(source: &Path, target: &Path) -> Result<bool> {
    const BUFFER_SIZE: usize = 64 * 1024;

    let source_file = fs::File::open(source)?;

    let target_file = fs::File::open(target)?;

    let mut source_reader = BufReader::new(source_file);

    let mut target_reader = BufReader::new(target_file);

    let mut source_buffer = [0_u8; BUFFER_SIZE];

    let mut target_buffer = [0_u8; BUFFER_SIZE];

    loop {
        let source_read = source_reader.read(&mut source_buffer)?;

        let target_read = target_reader.read(&mut target_buffer)?;

        if source_read != target_read {
            return Ok(true);
        }

        if source_read == 0 {
            return Ok(false);
        }

        if source_buffer[..source_read] != target_buffer[..target_read] {
            return Ok(true);
        }
    }
}

fn validate_relative_path(path: &Path) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("empty output path");
    }

    if path.is_absolute() {
        bail!("output path must be relative: {}", path.display(),);
    }

    if path.components().any(|component| {
        matches!(
            component,
            std::path::Component::ParentDir
                | std::path::Component::RootDir
                | std::path::Component::Prefix(_)
        )
    }) {
        bail!("unsafe output path: {}", path.display(),);
    }

    Ok(())
}

fn validate_html(path: &Path, html: &str) -> Result<()> {
    if !html.contains("<title>") || !html.contains("</title>") {
        bail!("{} has no title", path.display(),);
    }

    if !html.contains(r#"<meta name="description""#) {
        bail!("{} has no meta description", path.display(),);
    }

    if !html.contains("<html lang=") {
        bail!("{} has no html lang", path.display(),);
    }

    let h1_count = html.match_indices("<h1").count();

    if h1_count != 1 {
        bail!(
            "{} must contain exactly one h1, found {}",
            path.display(),
            h1_count,
        );
    }

    if path != Path::new("404.html") && !html.contains(r#"<link rel="canonical""#) {
        bail!("{} has no canonical link", path.display(),);
    }

    Ok(())
}

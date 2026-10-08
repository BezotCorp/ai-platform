use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use serde::de::DeserializeOwned;

use crate::build_engine::BuildContext;
use crate::component_tree::Website;
use crate::language_region::LanguageRegion;
use crate::source::{Blog, ContentIndex, PageSource, SectionIndex, Site, WebsiteSource};
use crate::source_path::SourcePath;
use crate::target_path::TargetPath;

impl BuildContext {
    pub(crate) fn load(source: SourcePath, target: TargetPath) -> Result<Self> {
        let website_source = read_ron::<WebsiteSource>(&source.as_path().join("site.ron"))?;

        let index = read_ron::<ContentIndex>(&source.as_path().join("index.ron"))?;

        let default_language_region = LanguageRegion::new(
            website_source.default_language,
            website_source.default_region,
        )?;

        let site_index_path = resolve_reference(source.as_path(), &index.site)?;

        let blog_index_path = resolve_reference(source.as_path(), &index.blog)?;

        let site_index = read_ron::<SectionIndex>(&site_index_path)?;

        let blog_index = read_ron::<SectionIndex>(&blog_index_path)?;

        Ok(Self {
            website: Website {
                name: website_source.name,
                description: website_source.description,
                base_url: website_source.base_url,
                default_language_region,
                pages: Vec::new(),
            },

            site: Site {
                root_path: site_index.root_path,
                pages: load_pages(&site_index_path, &site_index.documents)?,
            },

            blog: Blog {
                root_path: blog_index.root_path,
                pages: load_pages(&blog_index_path, &blog_index.documents)?,
            },

            source,
            target,
        })
    }
}

fn load_pages(index_path: &Path, references: &[String]) -> Result<Vec<PageSource>> {
    let directory = index_path.parent().ok_or_else(|| {
        anyhow::anyhow!("index has no parent directory: {}", index_path.display(),)
    })?;

    references
        .iter()
        .map(|reference| {
            let path = resolve_reference(directory, reference)?;

            read_ron::<PageSource>(&path)
        })
        .collect()
}

fn resolve_reference(base: &Path, reference: &str) -> Result<PathBuf> {
    if reference.trim().is_empty() {
        bail!("empty source reference");
    }

    let relative = Path::new(reference);

    if relative.is_absolute() {
        bail!("source reference must be relative: {reference}");
    }

    if relative.components().any(|component| {
        matches!(
            component,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        bail!("unsafe source reference: {reference}");
    }

    Ok(base.join(relative))
}

fn read_ron<T>(path: &Path) -> Result<T>
where
    T: DeserializeOwned,
{
    let source =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display(),))?;

    ron::from_str(&source).with_context(|| format!("failed to parse {}", path.display(),))
}

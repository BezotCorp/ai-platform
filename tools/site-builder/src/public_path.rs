use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::language_region::LanguageRegion;
use crate::root_path::RootPath;
use crate::slug::Slug;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct PublicPath(String);

impl PublicPath {
    pub(crate) fn new(
        language_region: &LanguageRegion,
        root_path: &RootPath,
        slug: &Slug,
    ) -> Result<Self> {
        let root = root_path.as_str();
        let slug = slug.as_str();

        if !root.starts_with('/') {
            bail!("root path must start with /: {root}");
        }

        if root != "/" && !root.ends_with('/') {
            bail!("root path must end with /: {root}");
        }

        if root.contains("..")
            || root.contains('\\')
            || root.contains('?')
            || root.contains('#')
            || root.contains("//")
        {
            bail!("invalid root path: {root}");
        }

        if slug.starts_with('/')
            || slug.ends_with('/')
            || slug.contains("..")
            || slug.contains('\\')
            || slug.contains('?')
            || slug.contains('#')
            || slug.contains("//")
        {
            bail!("invalid slug: {slug}");
        }

        let locale = language_region.route_segment();
        let mut path = format!("/{locale}/");

        if root != "/" {
            path.push_str(root.trim_matches('/'));
            path.push('/');
        }

        if !slug.is_empty() {
            path.push_str(slug);
            path.push('/');
        }

        Ok(Self(path))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn output_path(&self) -> PathBuf {
        PathBuf::from(self.0.trim_matches('/')).join("index.html")
    }
}

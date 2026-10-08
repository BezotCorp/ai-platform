use std::path::Path;

use anyhow::{Result, bail};
use walkdir::WalkDir;

use crate::build_engine::BuildContext;
use crate::build_output::{BuildOutput, GeneratedFile, PublicFile};
use crate::render_engine::RenderContext;

impl BuildContext {
    pub(crate) fn build(&self) -> Result<BuildOutput> {
        let renderer = RenderContext::new(&self.website);

        let mut generated_files = Vec::new();

        generated_files.push(GeneratedFile::new("index.html", renderer.render_root()));

        generated_files.push(GeneratedFile::new("404.html", renderer.render_not_found()));

        for page in &self.website.pages {
            generated_files.push(GeneratedFile::new(
                page.public_path.output_path(),
                renderer.render_page(page),
            ));
        }

        generated_files.push(GeneratedFile::new("robots.txt", self.render_robots()));

        generated_files.push(GeneratedFile::new("sitemap.xml", self.render_sitemap()?));

        let public_root = self.source.as_path().join("public");

        let public_files = collect_public_files(&public_root)?;

        Ok(BuildOutput::new(generated_files, public_files))
    }

    fn render_robots(&self) -> String {
        let mut content = String::from("User-agent: *\nAllow: /\n");

        if let Some(base_url) = self.website.base_url.as_deref() {
            content.push_str(&format!("\nSitemap: {base_url}/sitemap.xml\n"));
        }

        content
    }

    fn render_sitemap(&self) -> Result<String> {
        let base_url = self
            .website
            .base_url
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("base_url is required to generate sitemap.xml"))?;

        let mut xml = String::from(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\
<urlset xmlns=\"http://www.sitemaps.org/schemas/sitemap/0.9\">",
        );

        xml.push_str("<url><loc>");
        xml.push_str(&escape_xml(&format!("{base_url}/")));
        xml.push_str("</loc></url>");

        for page in self.website.pages.iter().filter(|page| page.index) {
            xml.push_str("<url><loc>");

            xml.push_str(&escape_xml(&format!(
                "{base_url}{}",
                page.public_path.as_str(),
            )));

            xml.push_str("</loc></url>");
        }

        xml.push_str("</urlset>\n");

        Ok(xml)
    }
}

fn collect_public_files(public_root: &Path) -> Result<Vec<PublicFile>> {
    if !public_root.is_dir() {
        bail!("public root does not exist: {}", public_root.display(),);
    }

    let mut files = Vec::new();

    for entry in WalkDir::new(public_root).follow_links(false) {
        let entry = entry?;

        if entry.file_type().is_symlink() {
            bail!(
                "symlink is not allowed in public sources: {}",
                entry.path().display(),
            );
        }

        if entry.file_type().is_file() {
            files.push(PublicFile::new(entry.path().to_path_buf()));
        }
    }

    files.sort_by(|left, right| left.source().cmp(right.source()));

    Ok(files)
}

fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

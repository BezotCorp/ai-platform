use std::collections::HashSet;

use anyhow::{Result, bail};

use crate::build_engine::BuildContext;
use crate::component_tree::{Block, TextStyle};

impl BuildContext {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.website.name.trim().is_empty() {
            bail!("website name cannot be empty");
        }

        if self.website.pages.is_empty() {
            bail!("website contains no pages");
        }

        let Some(base_url) = self.website.base_url.as_deref() else {
            bail!("website base_url is required");
        };

        if !base_url.starts_with("https://") && !base_url.starts_with("http://") {
            bail!("base_url must start with http:// or https://");
        }

        if base_url.ends_with('/') {
            bail!("base_url must not end with /");
        }

        let mut paths = HashSet::new();

        for page in &self.website.pages {
            if page.title.trim().is_empty() {
                bail!("page {} has an empty title", page.public_path.as_str(),);
            }

            if page.blocks.is_empty() {
                bail!("page {} contains no blocks", page.public_path.as_str(),);
            }

            if !paths.insert(page.public_path.as_str()) {
                bail!("duplicate public path: {}", page.public_path.as_str(),);
            }
        }

        let default_root = format!("/{}/", self.website.default_language_region.route_segment(),);

        if !self
            .website
            .pages
            .iter()
            .any(|page| page.public_path.as_str() == default_root)
        {
            bail!(
                "default language-region {} has no root page",
                self.website.default_language_region,
            );
        }

        for page in &self.website.pages {
            self.validate_blocks(&page.blocks, &paths)?;
        }

        Ok(())
    }

    fn validate_blocks(&self, blocks: &[Block], paths: &HashSet<&str>) -> Result<()> {
        for block in blocks {
            match block {
                Block::Hero {
                    title,
                    title_style,
                    actions,
                    ..
                } => {
                    if title.trim().is_empty() {
                        bail!("hero title cannot be empty");
                    }

                    if let Some(style) = title_style {
                        validate_text_style(style)?;
                    }

                    for action in actions {
                        self.validate_link(&action.href, paths)?;
                    }
                }

                Block::Heading { level, text, style } => {
                    if !(1..=6).contains(level) {
                        bail!("heading level must be between 1 and 6");
                    }

                    if text.trim().is_empty() {
                        bail!("heading text cannot be empty");
                    }

                    if let Some(style) = style {
                        validate_text_style(style)?;
                    }
                }

                Block::Paragraph { text, style } => {
                    if text.trim().is_empty() {
                        bail!("paragraph text cannot be empty");
                    }

                    if let Some(style) = style {
                        validate_text_style(style)?;
                    }
                }

                Block::Container { blocks } => {
                    if blocks.is_empty() {
                        bail!("container cannot be empty");
                    }

                    self.validate_blocks(blocks, paths)?;
                }

                Block::Link { label, href } => {
                    if label.trim().is_empty() {
                        bail!("link label cannot be empty");
                    }

                    self.validate_link(href, paths)?;
                }

                Block::Image { src, alt, .. } => {
                    if alt.trim().is_empty() {
                        bail!("image alt cannot be empty: {src}");
                    }

                    self.validate_asset(src)?;
                }

                Block::Code { code, .. } => {
                    if code.is_empty() {
                        bail!("code block cannot be empty");
                    }
                }

                Block::CardGrid { cards } => {
                    if cards.is_empty() {
                        bail!("card grid cannot be empty");
                    }

                    for card in cards {
                        if card.title.trim().is_empty() {
                            bail!("card title cannot be empty");
                        }

                        if let Some(href) = card.href.as_deref() {
                            self.validate_link(href, paths)?;
                        }
                    }
                }

                Block::Divider => {}
            }
        }

        Ok(())
    }

    fn validate_link(&self, href: &str, paths: &HashSet<&str>) -> Result<()> {
        if href.starts_with("https://")
            || href.starts_with("http://")
            || href.starts_with("mailto:")
            || href.starts_with("tel:")
            || href.starts_with('#')
        {
            return Ok(());
        }

        if href.starts_with("/assets/") {
            return self.validate_asset(href);
        }

        if !href.starts_with('/') {
            bail!("internal link must start with /: {href}");
        }

        let raw = href.split(['?', '#']).next().unwrap_or(href);

        let normalized = if raw == "/" {
            "/".to_owned()
        } else if raw.ends_with('/') {
            raw.to_owned()
        } else {
            format!("{raw}/")
        };

        if normalized == "/" {
            return Ok(());
        }

        if !paths.contains(normalized.as_str()) {
            bail!("internal link targets no page: {href}");
        }

        Ok(())
    }

    fn validate_asset(&self, source: &str) -> Result<()> {
        if source.starts_with("https://")
            || source.starts_with("http://")
            || source.starts_with("data:")
        {
            return Ok(());
        }

        let relative = source
            .strip_prefix('/')
            .ok_or_else(|| anyhow::anyhow!("local asset must start with /: {source}"))?;

        if relative.contains("..") || relative.contains('\\') {
            bail!("invalid asset path: {source}");
        }

        let path = self.source.as_path().join("public").join(relative);

        if !path.is_file() {
            bail!("asset does not exist: {}", path.display(),);
        }

        Ok(())
    }
}

fn validate_text_style(style: &TextStyle) -> Result<()> {
    for value in [style.color.as_deref(), style.font_size.as_deref()]
        .into_iter()
        .flatten()
    {
        if value.contains(';') || value.contains('{') || value.contains('}') {
            bail!("invalid text style value: {value}");
        }
    }

    if let Some(weight) = style.font_weight
        && (!(100..=900).contains(&weight) || weight % 100 != 0)
    {
        bail!("font_weight must be 100, 200, ... 900");
    }

    Ok(())
}

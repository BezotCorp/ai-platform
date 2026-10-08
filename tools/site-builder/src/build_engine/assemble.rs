use std::mem;

use anyhow::Result;

use crate::build_engine::BuildContext;
use crate::component_tree::Page;
use crate::language_region::LanguageRegion;
use crate::public_path::PublicPath;
use crate::root_path::RootPath;
use crate::source::PageSource;

impl BuildContext {
    pub(crate) fn assemble(&mut self) -> Result<()> {
        let site_pages = mem::take(&mut self.site.pages);
        let blog_pages = mem::take(&mut self.blog.pages);

        let mut pages = Vec::with_capacity(site_pages.len() + blog_pages.len());

        for source in site_pages {
            pages.push(build_page(source, &self.site.root_path)?);
        }

        for source in blog_pages {
            pages.push(build_page(source, &self.blog.root_path)?);
        }

        pages.sort_by(|left, right| {
            left.language_region
                .cmp(&right.language_region)
                .then_with(|| navigation_order(left).cmp(&navigation_order(right)))
                .then_with(|| left.public_path.cmp(&right.public_path))
        });

        self.website.pages = pages;

        Ok(())
    }
}

fn build_page(source: PageSource, root_path: &RootPath) -> Result<Page> {
    let language_region = LanguageRegion::new(source.language, source.region)?;

    let public_path = PublicPath::new(&language_region, root_path, &source.slug)?;

    Ok(Page {
        public_path,
        language_region,
        title: source.title,
        description: source.description,
        navigation: source.navigation,
        index: source.index,
        blocks: source.blocks,
    })
}

fn navigation_order(page: &Page) -> i32 {
    page.navigation
        .as_ref()
        .map(|navigation| navigation.order)
        .unwrap_or(i32::MAX)
}

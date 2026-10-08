use crate::component_tree::Website;

pub(crate) struct RenderContext<'a> {
    pub(crate) website: &'a Website,
}

impl<'a> RenderContext<'a> {
    pub(crate) fn new(website: &'a Website) -> Self {
        Self { website }
    }
}

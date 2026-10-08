use crate::component_tree::Page;
use crate::language_region::LanguageRegion;

#[derive(Debug)]
pub(crate) struct Website {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) base_url: Option<String>,
    pub(crate) default_language_region: LanguageRegion,
    pub(crate) pages: Vec<Page>,
}

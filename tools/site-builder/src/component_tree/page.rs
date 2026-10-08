use crate::component_tree::{Block, Navigation};
use crate::language_region::LanguageRegion;
use crate::public_path::PublicPath;

#[derive(Debug)]
pub(crate) struct Page {
    pub(crate) public_path: PublicPath,
    pub(crate) language_region: LanguageRegion,
    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) navigation: Option<Navigation>,
    pub(crate) index: bool,
    pub(crate) blocks: Vec<Block>,
}

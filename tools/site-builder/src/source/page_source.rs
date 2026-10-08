use serde::Deserialize;

use crate::component_tree::{Block, Navigation};
use crate::language::Language;
use crate::region::Region;
use crate::slug::Slug;

#[derive(Debug, Deserialize)]
pub(crate) struct PageSource {
    pub(crate) language: Language,
    pub(crate) region: Region,
    pub(crate) slug: Slug,

    #[serde(default = "default_index")]
    pub(crate) index: bool,

    pub(crate) title: String,
    pub(crate) description: Option<String>,
    pub(crate) navigation: Option<Navigation>,
    pub(crate) blocks: Vec<Block>,
}

fn default_index() -> bool {
    true
}

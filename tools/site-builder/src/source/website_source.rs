use serde::Deserialize;

use crate::language::Language;
use crate::region::Region;

#[derive(Debug, Deserialize)]
pub(crate) struct WebsiteSource {
    pub(crate) name: String,
    pub(crate) description: Option<String>,
    pub(crate) base_url: Option<String>,
    pub(crate) default_language: Language,
    pub(crate) default_region: Region,
}

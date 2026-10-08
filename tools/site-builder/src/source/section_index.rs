use serde::Deserialize;

use crate::root_path::RootPath;

#[derive(Debug, Deserialize)]
pub(crate) struct SectionIndex {
    pub(crate) root_path: RootPath,
    pub(crate) documents: Vec<String>,
}

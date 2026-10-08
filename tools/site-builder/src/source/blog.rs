use crate::root_path::RootPath;
use crate::source::PageSource;

#[derive(Debug)]
pub(crate) struct Blog {
    pub(crate) root_path: RootPath,
    pub(crate) pages: Vec<PageSource>,
}

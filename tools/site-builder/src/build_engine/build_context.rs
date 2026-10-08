use crate::component_tree::Website;
use crate::source::{Blog, Site};
use crate::source_path::SourcePath;
use crate::target_path::TargetPath;

pub(crate) struct BuildContext {
    pub(crate) website: Website,
    pub(crate) site: Site,
    pub(crate) blog: Blog,
    pub(crate) source: SourcePath,
    pub(crate) target: TargetPath,
}

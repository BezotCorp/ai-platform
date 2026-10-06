use crate::model::ModelSettings;
use std::path::PathBuf;
#[derive(Clone)]
pub(crate) struct ResolvedModelPaths {
    pub(crate) model_path: PathBuf,
    pub(crate) context_limit: usize,
    pub(crate) settings: ModelSettings,
    pub(crate) mmproj_path: Option<PathBuf>,
    pub(crate) backend_id: Option<String>,
    pub(crate) draft_model_path: Option<PathBuf>,
}

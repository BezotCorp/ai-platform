use std::path::PathBuf;
pub(crate) struct ExplicitModelPath {
    pub(crate) model_path: PathBuf,
    pub(crate) backend_id: &'static str,
}

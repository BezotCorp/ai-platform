use std::path::PathBuf;

#[derive(Debug, Clone)]
pub struct CachedLocalModel {
    pub id: String,
    pub repo_id: String,
    pub filename: String,
    pub quantization: String,
    pub backend_id: String,
    pub model_path: PathBuf,
    pub size_bytes: u64,
    pub mmproj_path: Option<PathBuf>,
    pub mmproj_size_bytes: u64,
}

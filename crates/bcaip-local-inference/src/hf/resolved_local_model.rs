use std::path;

use crate::hf::resolved_model::ResolvedModel;
use crate::local_model_format::model_id_from_repo;

#[derive(Debug, Clone)]
pub enum ResolvedLocalModel {
    Gguf {
        repo_id: String,
        quantization: String,
        resolved: ResolvedModel,
        local_paths: Vec<path::PathBuf>,
        mmproj_path: Option<path::PathBuf>,
    },
    Mlx {
        repo_id: String,
        variant_id: String,
        snapshot_path: path::PathBuf,
        total_size: u64,
    },
}

impl ResolvedLocalModel {
    pub fn model_id(&self) -> String {
        match self {
            Self::Gguf {
                repo_id,
                quantization,
                ..
            } => model_id_from_repo(repo_id, quantization),
            Self::Mlx { repo_id, .. } => repo_id.clone(),
        }
    }

    pub fn total_size(&self) -> u64 {
        match self {
            Self::Gguf { resolved, .. } => resolved.total_size,
            Self::Mlx { total_size, .. } => *total_size,
        }
    }
}

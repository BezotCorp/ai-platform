use super::{mlx_error::mlx_error, snapshot_validation::validate_snapshot_files};
use goose_provider_types::errors::ProviderError;
use safemlx_lm::models::LoadedModel;
use serde_json::json;
use std::path::{Path, PathBuf};

pub(crate) fn validate_model_directory(path: &Path) -> Result<(), ProviderError> {
    super::validate_snapshot_files(path)?;
    let config_path = path.join("config.json");
    let config = std::fs::read(&config_path).map_err(mlx_error)?;
    let config: serde_json::Value = serde_json::from_slice(&config).map_err(mlx_error)?;
    if let Some(reason) = safemlx_lm::check_model_config(&config).unsupported_reason() {
        return Err(ProviderError::ExecutionError(format!(
            "Unsupported MLX model at '{}': {}",
            path.display(),
            reason
        )));
    }
    Ok(())
}
pub(crate) fn model_dir_from_path(path: &Path) -> Result<PathBuf, ProviderError> {
    if path.is_dir() {
        Ok(path.to_path_buf())
    } else {
        path.parent()
            .map(Path::to_path_buf)
            .ok_or_else(|| mlx_error("MLX model path has no parent directory"))
    }
}
pub(crate) fn mlx_stop_token_ids(model: &LoadedModel, model_dir: &Path) -> Vec<u32> {
    let mut ids = model.eos_token_ids().to_vec();
    for id in generation_config_eos_token_ids(model_dir) {
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids
}
pub(crate) fn generation_config_eos_token_ids(model_dir: &Path) -> Vec<u32> {
    let Ok(config_json) = std::fs::read_to_string(model_dir.join("generation_config.json")) else {
        return Vec::new();
    };
    let Ok(config) = serde_json::from_str::<serde_json::Value>(&config_json) else {
        return Vec::new();
    };
    match config.get("eos_token_id") {
        Some(value) => token_id_or_ids(value),
        None => Vec::new(),
    }
}
pub(crate) fn token_id_or_ids(value: &serde_json::Value) -> Vec<u32> {
    if let Some(id) = value.as_u64().and_then(|id| u32::try_from(id).ok()) {
        return vec![id];
    }
    value
        .as_array()
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_u64().and_then(|id| u32::try_from(id).ok()))
                .collect()
        })
        .unwrap_or_default()
}

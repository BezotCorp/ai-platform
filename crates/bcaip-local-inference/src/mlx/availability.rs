use std::path::Path;

use bcaip_provider_types::errors::ProviderError;

use crate::mlx::snapshot_validation::validate_snapshot_files;

pub(crate) fn validate_model_directory(path: &Path) -> Result<(), ProviderError> {
    #[cfg(all(feature = "mlx", target_os = "macos"))]
    return crate::mlx::model_validation::validate_model_directory(path);

    #[cfg(not(all(feature = "mlx", target_os = "macos")))]
    {
        validate_snapshot_files(path)?;
        Err(ProviderError::ExecutionError(unavailable_reason()))
    }
}

#[cfg(not(all(feature = "mlx", target_os = "macos")))]
pub(crate) fn unavailable_error() -> ProviderError {
    ProviderError::ExecutionError(unavailable_reason())
}

#[cfg(not(all(feature = "mlx", target_os = "macos")))]
fn unavailable_reason() -> String {
    if cfg!(target_os = "macos") {
        "MLX support was not compiled in. Rebuild with the `mlx` feature.".to_string()
    } else {
        "MLX backend requires macOS.".to_string()
    }
}

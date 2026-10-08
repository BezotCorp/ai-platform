mod snapshot_validation;

#[cfg(all(feature = "mlx", target_os = "macos"))]
mod gemma4;
#[cfg(all(feature = "mlx", target_os = "macos"))]
mod generation;
#[cfg(all(feature = "mlx", target_os = "macos"))]
mod mlx_backend;
#[cfg(all(feature = "mlx", target_os = "macos"))]
mod mlx_error;
#[cfg(all(feature = "mlx", target_os = "macos"))]
mod mlx_generation;
#[cfg(all(feature = "mlx", target_os = "macos"))]
mod mlx_loaded_model;
#[cfg(all(feature = "mlx", target_os = "macos"))]
mod mlx_stream_emitter;
#[cfg(all(feature = "mlx", target_os = "macos"))]
mod model_validation;
#[cfg(all(feature = "mlx", target_os = "macos"))]
mod output;
#[cfg(all(feature = "mlx", target_os = "macos"))]
mod prompt;
#[cfg(all(feature = "mlx", target_os = "macos"))]
mod sampling;
#[cfg(all(feature = "mlx", target_os = "macos"))]
mod tool_mode;

#[cfg(not(all(feature = "mlx", target_os = "macos")))]
mod mlx_backend;

#[cfg(all(feature = "mlx", target_os = "macos"))]
pub(crate) use mlx_backend::{MLX_BACKEND_ID, MlxBackend};

#[cfg(all(feature = "mlx", target_os = "macos"))]
pub(crate) use model_validation::validate_model_directory;

#[cfg(all(feature = "mlx", target_os = "macos"))]
pub(crate) use output::*;

#[cfg(not(all(feature = "mlx", target_os = "macos")))]
pub(crate) use mlx_backend::{MLX_BACKEND_ID, MlxBackend, validate_model_directory};

pub(crate) use snapshot_validation::mlx_snapshot_files_are_complete;

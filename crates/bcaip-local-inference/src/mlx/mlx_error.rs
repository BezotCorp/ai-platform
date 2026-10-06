use goose_provider_types::errors::ProviderError;
pub(crate) fn mlx_error(error: impl std::fmt::Display) -> ProviderError {
    ProviderError::ExecutionError(format!("MLX backend error: {}", error))
}

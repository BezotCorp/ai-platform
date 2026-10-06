use super::mlx_error::mlx_error;
use crate::model::ModelSettings;
use goose_provider_types::errors::ProviderError;
use safemlx::{Array, random};

pub(crate) fn sampling(settings: &ModelSettings) -> (f32, Option<u32>) {
    match &settings.sampling {
        crate::model::SamplingConfig::Greedy => (0.0, None),
        crate::model::SamplingConfig::Temperature {
            temperature, seed, ..
        } => (*temperature, *seed),
        crate::model::SamplingConfig::MirostatV2 { seed, .. } => (0.0, *seed),
    }
}
pub(crate) fn prng_key(temp: f32, seed: Option<u32>) -> Result<Option<Array>, ProviderError> {
    if temp == 0.0 {
        return Ok(None);
    }
    random::key(seed.unwrap_or(0) as u64)
        .map(Some)
        .map_err(mlx_error)
}

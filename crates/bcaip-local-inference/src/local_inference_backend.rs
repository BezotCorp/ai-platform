use bcaip_provider_types::errors::ProviderError;

use crate::local_generation_request::{BackendLoadedModel, LocalGenerationRequest};
use crate::model::ModelSettings;
use crate::resolved_model_paths::ResolvedModelPaths;

pub(crate) trait LocalInferenceBackend: Send + Sync {
    fn id(&self) -> &'static str;

    fn load_model(
        &self,
        model_id: &str,
        resolved: &ResolvedModelPaths,
        settings: &ModelSettings,
    ) -> Result<Box<dyn BackendLoadedModel>, ProviderError>;

    fn generate(
        &self,
        loaded: &mut dyn BackendLoadedModel,
        request: LocalGenerationRequest<'_>,
    ) -> Result<(), ProviderError>;

    fn available_memory_bytes(&self) -> u64;
}

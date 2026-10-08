use std::any::Any;

use llama_cpp_2::{model::LlamaModel, mtmd::MtmdContext};

use crate::{
    llamacpp::inference_engine::LoadedChatTemplates, local_generation_request::BackendLoadedModel,
};

pub(crate) struct LoadedModel {
    pub model: LlamaModel,
    pub templates: LoadedChatTemplates,
    /// Multimodal context for vision models. None for text-only models.
    pub mtmd_ctx: Option<MtmdContext>,
}

impl BackendLoadedModel for LoadedModel {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

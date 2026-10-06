use crate::backend::BackendLoadedModel;
use safemlx_lm::models::LoadedModel;
use safemlx_lm_utils::tokenizer::Tokenizer;
use std::{any::Any, path::PathBuf};
pub(crate) struct MlxLoadedModel {
    pub(crate) model: LoadedModel,
    pub(crate) tokenizer: Tokenizer,
    pub(crate) model_dir: PathBuf,
    pub(crate) stop_token_ids: Vec<u32>,
}

impl BackendLoadedModel for MlxLoadedModel {
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

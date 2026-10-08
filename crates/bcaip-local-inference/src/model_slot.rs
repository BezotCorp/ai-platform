use crate::{
    local_generation_request::BackendLoadedModel, resolved_model_paths::ResolvedModelPaths,
};
use std::sync::Arc;
use tokio::sync::{Mutex, Notify};

pub(crate) type ModelSlotHandle = Arc<ModelSlot>;

pub(crate) struct ModelSlot {
    pub(crate) state: Mutex<ModelSlotState>,
    pub(crate) notify: Notify,
}

pub(crate) enum ModelSlotState {
    Empty,
    Loading,
    Loaded {
        model: Box<dyn BackendLoadedModel>,
        resolved: Box<ResolvedModelPaths>,
    },
}

impl ModelSlot {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(ModelSlotState::Empty),
            notify: Notify::new(),
        }
    }
}

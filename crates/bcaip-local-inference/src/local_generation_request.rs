use crate::{StreamSender, model::ModelSettings, resolved_model_paths::ResolvedModelPaths};
use bcaip_provider_types::{conversations::Message, request_log::RequestLogHandle};
use rmcp::model::Tool;
use std::any::Any;
pub(crate) trait BackendLoadedModel: Send {
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

#[cfg_attr(not(feature = "mlx"), allow(dead_code))]
pub(crate) struct LocalGenerationRequest<'a> {
    pub model_name: String,
    pub system: &'a str,
    pub messages: &'a [Message],
    pub tools: &'a [Tool],
    pub settings: &'a ModelSettings,
    pub temperature: Option<f32>,
    pub max_tokens: Option<i32>,
    pub context_limit: usize,
    pub model_load_ms: Option<u64>,
    pub resolved_model: &'a ResolvedModelPaths,
    pub draft_model_path: Option<std::path::PathBuf>,
    pub message_id: &'a str,
    pub tx: &'a StreamSender,
    pub log: &'a mut Option<Box<dyn RequestLogHandle>>,
}

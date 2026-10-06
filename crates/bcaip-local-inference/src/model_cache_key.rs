use crate::model::ChatTemplate;
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub(crate) struct ModelCacheKey {
    pub(crate) backend_id: &'static str,
    pub(crate) model_id: String,
    pub(crate) chat_template: ChatTemplate,
}

impl ModelCacheKey {
    pub(crate) fn new(
        backend_id: &'static str,
        model_id: impl Into<String>,
        chat_template: ChatTemplate,
    ) -> Self {
        Self {
            backend_id,
            model_id: model_id.into(),
            chat_template,
        }
    }
}

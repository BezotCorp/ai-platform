use goose_provider_types::conversations::DraftStats;
pub(crate) struct MlxGeneration {
    pub(crate) generated_ids: Vec<u32>,
    pub(crate) generated_text: String,
    pub(crate) draft_stats: Option<DraftStats>,
    pub(crate) time_to_first_token_ms: Option<u64>,
    pub(crate) streamed_response: bool,
}

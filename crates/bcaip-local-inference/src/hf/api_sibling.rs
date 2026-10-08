#[derive(Debug, Clone, serde::Deserialize)]
pub(crate) struct HfApiSibling {
    pub(crate) rfilename: String,
    #[serde(default)]
    pub(crate) size: Option<u64>,
}

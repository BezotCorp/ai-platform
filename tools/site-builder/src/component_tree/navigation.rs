use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct Navigation {
    pub(crate) label: String,

    #[serde(default)]
    pub(crate) order: i32,
}

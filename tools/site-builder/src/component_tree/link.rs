use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct Link {
    pub(crate) label: String,
    pub(crate) href: String,
}

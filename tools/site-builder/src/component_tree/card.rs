use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct Card {
    pub(crate) title: String,
    pub(crate) text: Option<String>,
    pub(crate) href: Option<String>,
}

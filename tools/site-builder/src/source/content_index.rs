use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub(crate) struct ContentIndex {
    pub(crate) site: String,
    pub(crate) blog: String,
}

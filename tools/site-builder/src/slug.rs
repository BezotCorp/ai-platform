use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub(crate) struct Slug(String);

impl Slug {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

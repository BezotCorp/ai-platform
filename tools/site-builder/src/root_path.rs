use serde::Deserialize;

#[derive(Debug, Deserialize)]
#[serde(transparent)]
pub(crate) struct RootPath(String);

impl RootPath {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

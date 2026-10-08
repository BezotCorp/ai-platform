use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub(crate) struct Region(String);

impl Region {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

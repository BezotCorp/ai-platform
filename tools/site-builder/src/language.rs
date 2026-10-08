use serde::Deserialize;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd)]
#[serde(transparent)]
pub(crate) struct Language(String);

impl Language {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

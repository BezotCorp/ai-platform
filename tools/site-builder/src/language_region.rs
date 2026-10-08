use std::fmt;

use anyhow::{Result, bail};

use crate::language::Language;
use crate::region::Region;

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(crate) struct LanguageRegion {
    language: Language,
    region: Region,
}

impl LanguageRegion {
    pub(crate) fn new(language: Language, region: Region) -> Result<Self> {
        if language.as_str().trim().is_empty() {
            bail!("language cannot be empty");
        }

        if region.as_str().trim().is_empty() {
            bail!("region cannot be empty");
        }

        Ok(Self { language, region })
    }

    pub(crate) fn route_segment(&self) -> String {
        format!(
            "{}-{}",
            self.language.as_str().to_ascii_lowercase(),
            self.region.as_str().to_ascii_lowercase(),
        )
    }
}

impl fmt::Display for LanguageRegion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}-{}",
            self.language.as_str().to_ascii_lowercase(),
            self.region.as_str().to_ascii_uppercase(),
        )
    }
}

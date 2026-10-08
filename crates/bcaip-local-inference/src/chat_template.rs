use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatTemplate {
    #[serde(alias = "auto")]
    #[default]
    Embedded,
    Builtin {
        name: String,
    },
    CustomInline {
        template: String,
    },
}

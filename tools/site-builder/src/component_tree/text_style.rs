use serde::Deserialize;

use crate::component_tree::TextAlign;

#[derive(Debug, Deserialize)]
pub(crate) struct TextStyle {
    pub(crate) color: Option<String>,
    pub(crate) font_size: Option<String>,
    pub(crate) font_weight: Option<u16>,
    pub(crate) align: Option<TextAlign>,
}

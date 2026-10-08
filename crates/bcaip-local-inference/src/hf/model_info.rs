use serde::{Deserialize, Serialize};

use crate::hf::gguf_file::HfGgufFile;
use crate::hf::model_variant::HfModelVariant;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfModelInfo {
    pub repo_id: String,
    pub author: String,
    pub model_name: String,
    pub downloads: u64,
    pub gguf_files: Vec<HfGgufFile>,
    #[serde(default)]
    pub variants: Vec<HfModelVariant>,
}

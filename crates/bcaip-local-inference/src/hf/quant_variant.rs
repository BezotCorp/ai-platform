use serde::{Deserialize, Serialize};

use crate::hf::model_variant::HfModelVariant;
use crate::local_model_format::{GGUF_FORMAT, LLAMACPP_BACKEND_ID, model_id_from_repo};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HfQuantVariant {
    pub quantization: String,
    pub size_bytes: u64,
    pub filename: String,
    pub download_url: String,
    pub description: &'static str,
    pub quality_rank: u8,
    #[serde(default)]
    pub sharded: bool,
}

impl HfQuantVariant {
    pub fn to_model_variant(&self, repo_id: &str) -> HfModelVariant {
        let model_id = model_id_from_repo(repo_id, &self.quantization);
        HfModelVariant {
            variant_id: self.quantization.clone(),
            label: self.quantization.clone(),
            backend_id: LLAMACPP_BACKEND_ID.to_string(),
            format: GGUF_FORMAT.to_string(),
            model_id: model_id.clone(),
            download_id: model_id,
            size_bytes: self.size_bytes,
            filename: Some(self.filename.clone()),
            download_url: Some(self.download_url.clone()),
            description: self.description.to_string(),
            quality_rank: self.quality_rank,
            sharded: self.sharded,
            supported: true,
            unsupported_reason: None,
        }
    }
}

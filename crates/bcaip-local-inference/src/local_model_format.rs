#[cfg(feature = "hf-hub")]
pub(crate) const HF_DOWNLOAD_BASE: &str = "https://huggingface.co";
#[cfg(feature = "hf-hub")]
pub(crate) const LLAMACPP_BACKEND_ID: &str = "llamacpp";
#[cfg(feature = "hf-hub")]
pub(crate) const GGUF_FORMAT: &str = "gguf";

pub fn model_id_from_repo(repo_id: &str, quantization: &str) -> String {
    format!("{repo_id}:{quantization}")
}

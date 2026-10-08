use crate::hf::gguf_file::HfGgufFile;

#[derive(Debug, Clone)]
pub struct ResolvedModel {
    pub files: Vec<HfGgufFile>,
    pub total_size: u64,
    pub mmproj: Option<HfGgufFile>,
}

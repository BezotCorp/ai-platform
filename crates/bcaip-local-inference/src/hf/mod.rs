pub(crate) mod api_sibling;
pub mod cached_local_model;
mod catalog;
pub(crate) mod gguf_catalog;
pub mod gguf_file;
pub(crate) mod gguf_variants;
pub(crate) mod hub_client;
pub mod model_info;
pub(crate) mod model_resolution;
pub mod model_variant;
pub mod quant_variant;
pub(crate) mod repo_siblings;
pub mod resolved_local_model;
pub mod resolved_model;

pub use crate::local_model_format::model_id_from_repo;
pub use cached_local_model::CachedLocalModel;
pub use catalog::{
    cached_local_model, cached_local_models, delete_cached_local_model, get_repo_gguf_files,
    get_repo_gguf_variants, get_repo_local_variants, resolve_local_model_selection,
    resolve_local_model_spec, search_gguf_models, search_local_models,
};
pub(crate) use gguf_catalog::canonicalize_quantization;
pub use gguf_catalog::parse_quantization_from_filename;
pub use gguf_file::HfGgufFile;
pub use mlx_catalog::get_repo_mlx_variants;
pub use model_info::HfModelInfo;
pub use model_resolution::{
    parse_model_spec, recommend_variant, resolve_model_spec, resolve_model_spec_full,
};
pub use model_variant::HfModelVariant;
pub use quant_variant::HfQuantVariant;
pub use resolved_local_model::ResolvedLocalModel;
pub use resolved_model::ResolvedModel;

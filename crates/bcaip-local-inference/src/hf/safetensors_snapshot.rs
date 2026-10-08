use anyhow::Result;
use hf_hub::repository::{ModelInfo, RepoSibling};
use hf_hub::{HFRepository, RepoTypeModel};

use crate::hf::hub_client::{hf_client, model_repo};
use crate::hf::model_variant::HfModelVariant;

const MLX_BACKEND_ID: &str = "mlx";
const MLX_FORMAT: &str = "mlx-safetensors";
const MLX_VARIANT_ID: &str = "default";

pub async fn get_repo_mlx_variants(repo_id: &str) -> Result<Vec<HfModelVariant>> {
    let client = hf_client().await?;
    let repo = model_repo(&client, repo_id)?;
    let info = repo
        .info()
        .expand(vec![
            "siblings".to_string(),
            "config".to_string(),
            "safetensors".to_string(),
        ])
        .send()
        .await?;
    if !is_mlx_compatible_model_info(&info) {
        return Ok(Vec::new());
    }
    let mlx_config = load_repo_config_json(&repo)
        .await
        .unwrap_or_else(|_| info.config.clone());
    Ok(mlx_variants_from_model_info(repo_id, &info, &mlx_config))
}

pub(crate) async fn load_repo_config_json(
    repo: &HFRepository<RepoTypeModel>,
) -> Result<Option<serde_json::Value>> {
    let config_path = repo
        .download_file()
        .filename("config.json".to_string())
        .send()
        .await?;
    let config_json = tokio::fs::read_to_string(config_path).await?;
    Ok(Some(serde_json::from_str(&config_json)?))
}

pub(crate) fn mlx_variants_from_model_info(
    repo_id: &str,
    info: &ModelInfo,
    mlx_config: &Option<serde_json::Value>,
) -> Vec<HfModelVariant> {
    let siblings = info.siblings.as_deref().unwrap_or(&[]);

    if !is_mlx_compatible_repo(&info.config, siblings) {
        return Vec::new();
    }

    let size_bytes = mlx_download_size_bytes(info, siblings);
    let variant_id = mlx_variant_id(repo_id, &info.config);

    vec![HfModelVariant {
        variant_id: variant_id.clone(),
        label: mlx_variant_label(&variant_id),
        backend_id: MLX_BACKEND_ID.to_string(),
        format: MLX_FORMAT.to_string(),
        model_id: repo_id.to_string(),
        download_id: repo_id.to_string(),
        size_bytes,
        filename: None,
        download_url: None,
        description: mlx_variant_description(mlx_config),
        quality_rank: 91,
        sharded: siblings
            .iter()
            .filter(|s| s.rfilename.ends_with(".safetensors"))
            .count()
            > 1,
        supported: is_mlx_runtime_supported(mlx_config),
        unsupported_reason: mlx_unsupported_reason(mlx_config),
    }]
}

pub(crate) fn is_mlx_compatible_model_info(info: &ModelInfo) -> bool {
    is_mlx_compatible_repo(&info.config, info.siblings.as_deref().unwrap_or_default())
}

pub(crate) fn is_mlx_compatible_repo(
    config: &Option<serde_json::Value>,
    siblings: &[RepoSibling],
) -> bool {
    let has_config = siblings.iter().any(|s| s.rfilename == "config.json");
    let has_tokenizer = has_mlx_tokenizer(siblings);
    let has_safetensors = siblings
        .iter()
        .any(|s| s.rfilename.ends_with(".safetensors"));

    has_config && has_tokenizer && has_safetensors && mlx_model_type(config).is_some()
}

pub(crate) fn mlx_download_size_bytes(info: &ModelInfo, siblings: &[RepoSibling]) -> u64 {
    let sibling_size: u64 = mlx_download_filenames(siblings)
        .into_iter()
        .filter_map(|filename| {
            siblings
                .iter()
                .find(|s| s.rfilename == filename)
                .and_then(|s| s.size)
        })
        .sum();
    sibling_size.max(estimated_safetensors_size_bytes(info))
}

fn estimated_safetensors_size_bytes(info: &ModelInfo) -> u64 {
    info.safetensors
        .as_ref()
        .map(|safetensors| {
            safetensors
                .parameters
                .iter()
                .map(|(dtype, count)| count.saturating_mul(dtype_size_bytes(dtype)))
                .sum()
        })
        .unwrap_or(0)
}

fn dtype_size_bytes(dtype: &str) -> u64 {
    match dtype.to_ascii_uppercase().as_str() {
        "BOOL" | "I8" | "U8" | "F8_E4M3" | "F8_E4M3FN" | "F8_E5M2" | "F8_E5M2FNUZ" => 1,
        "BF16" | "F16" | "I16" | "U16" => 2,
        "F32" | "I32" | "U32" => 4,
        "F64" | "I64" | "U64" => 8,
        _ => 0,
    }
}

fn has_mlx_tokenizer(siblings: &[RepoSibling]) -> bool {
    siblings
        .iter()
        .any(|s| is_standalone_mlx_tokenizer_file(&s.rfilename))
}

fn is_standalone_mlx_tokenizer_file(filename: &str) -> bool {
    filename == "tokenizer.json"
}

fn mlx_model_type(config: &Option<serde_json::Value>) -> Option<&str> {
    config
        .as_ref()
        .and_then(|config| config.get("model_type"))
        .and_then(|value| value.as_str())
}

pub(crate) fn is_mlx_runtime_supported(config: &Option<serde_json::Value>) -> bool {
    mlx_unsupported_reason(config).is_none()
}

fn mlx_unsupported_reason(config: &Option<serde_json::Value>) -> Option<String> {
    if !cfg!(target_os = "macos") {
        return Some("MLX requires macOS".to_string());
    }
    if !cfg!(feature = "mlx") {
        return Some("MLX support was not compiled in".to_string());
    }

    mlx_config_support(config)
}

fn mlx_config_support(config: &Option<serde_json::Value>) -> Option<String> {
    let config = config.as_ref()?;
    mlx_config_support_for_value(config)
}

#[cfg(all(feature = "mlx", target_os = "macos"))]
fn mlx_config_support_for_value(config: &serde_json::Value) -> Option<String> {
    safemlx_lm::check_model_config(config)
        .unsupported_reason()
        .map(str::to_string)
}

#[cfg(not(all(feature = "mlx", target_os = "macos")))]
fn mlx_config_support_for_value(_config: &serde_json::Value) -> Option<String> {
    None
}

fn mlx_variant_description(config: &Option<serde_json::Value>) -> String {
    match mlx_unsupported_reason(config) {
        None => "MLX safetensors snapshot".to_string(),
        Some(reason) => format!("MLX safetensors snapshot ({reason})"),
    }
}

pub(crate) fn mlx_download_filenames(siblings: &[RepoSibling]) -> Vec<String> {
    siblings
        .iter()
        .filter(|s| should_download_for_mlx(&s.rfilename))
        .map(|s| s.rfilename.clone())
        .collect()
}

pub(crate) fn should_download_for_mlx(filename: &str) -> bool {
    filename.ends_with(".safetensors")
        || filename == "config.json"
        || is_standalone_mlx_tokenizer_file(filename)
        || filename == "tokenizer_config.json"
        || filename == "generation_config.json"
        || filename == "configuration.json"
        || filename == "chat_template.jinja"
        || filename == "preprocessor_config.json"
        || filename == "video_preprocessor_config.json"
        || filename == "special_tokens_map.json"
        || filename == "model.safetensors.index.json"
        || filename == "vocab.json"
        || filename == "merges.txt"
        || filename == "added_tokens.json"
}

pub(crate) fn mlx_variant_id(repo_id: &str, config: &Option<serde_json::Value>) -> String {
    let repo_lower = repo_id.to_lowercase();
    for marker in ["bf16", "f16", "fp16", "f32", "fp32", "fp8", "4bit", "8bit"] {
        if repo_lower.contains(marker) {
            return marker.to_string();
        }
    }
    config
        .as_ref()
        .and_then(|config| config.get("torch_dtype"))
        .and_then(|value| value.as_str())
        .map(|dtype| dtype.replace("float", "f"))
        .unwrap_or_else(|| MLX_VARIANT_ID.to_string())
}

fn mlx_variant_label(variant_id: &str) -> String {
    if variant_id == MLX_VARIANT_ID {
        "MLX".to_string()
    } else {
        format!("MLX {}", variant_id.to_uppercase())
    }
}

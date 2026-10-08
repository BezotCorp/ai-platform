use anyhow::{Result, bail};

use crate::hf::api_sibling::HfApiSibling;
use crate::hf::gguf_catalog::{
    build_download_url, is_shard_file, parse_quantization, parse_shard_index, parse_shard_total,
};
use crate::hf::gguf_file::HfGgufFile;
use crate::hf::gguf_variants::{
    ensure_unambiguous_model_family, is_auxiliary_gguf_file, select_best_mmproj,
    select_preferred_shard_set,
};
use crate::hf::quant_variant::HfQuantVariant;
use crate::hf::repo_siblings::get_repo_siblings;
use crate::hf::resolved_model::ResolvedModel;

pub fn parse_model_spec(spec: &str) -> Result<(String, String)> {
    let (repo_id, quant) = spec.rsplit_once(':').ok_or_else(|| {
        anyhow::anyhow!(
            "Invalid model spec '{}': expected format 'user/repo:quantization'",
            spec
        )
    })?;

    if !repo_id.contains('/') {
        bail!("Invalid repo_id '{}': expected format 'user/repo'", repo_id);
    }

    if quant.is_empty() {
        bail!(
            "Invalid model spec '{}': expected format 'user/repo:quantization'",
            spec
        );
    }

    Ok((repo_id.to_string(), quant.to_string()))
}

pub async fn resolve_model_spec_full(spec: &str) -> Result<(String, ResolvedModel)> {
    let (repo_id, quant) = parse_model_spec(spec)?;
    let siblings = get_repo_siblings(&repo_id).await?;
    let matching = matching_quantization_files(&siblings, &quant);

    if matching.is_empty() {
        bail!(
            "No GGUF file with quantization '{}' found in {}",
            quant,
            repo_id
        );
    }
    ensure_unambiguous_model_family(&repo_id, &quant, &matching)?;

    let (mut single_files, shard_files) = partition_shards(matching);
    single_files.sort_by_key(|s| (s.rfilename.len(), s.rfilename.clone()));

    if let Some(single) = single_files.first() {
        let file = HfGgufFile {
            filename: single.rfilename.clone(),
            size_bytes: single.size.unwrap_or(0),
            quantization: quant.clone(),
            download_url: build_download_url(&repo_id, &single.rfilename),
        };
        let mmproj = select_best_mmproj(&repo_id, &siblings, &single.rfilename, &quant);
        return Ok((
            repo_id,
            ResolvedModel {
                total_size: file.size_bytes,
                files: vec![file],
                mmproj,
            },
        ));
    }

    let mut shard_files = select_preferred_shard_set(shard_files);
    shard_files.sort_by(|a, b| a.rfilename.cmp(&b.rfilename));
    validate_shard_set(&repo_id, &quant, &shard_files)?;

    let files: Vec<HfGgufFile> = shard_files
        .iter()
        .map(|s| HfGgufFile {
            filename: s.rfilename.clone(),
            size_bytes: s.size.unwrap_or(0),
            quantization: quant.clone(),
            download_url: build_download_url(&repo_id, &s.rfilename),
        })
        .collect();
    let total_size = files.iter().map(|f| f.size_bytes).sum();
    let mmproj = select_best_mmproj(&repo_id, &siblings, &files[0].filename, &quant);

    Ok((
        repo_id,
        ResolvedModel {
            files,
            total_size,
            mmproj,
        },
    ))
}

pub async fn resolve_model_spec(spec: &str) -> Result<(String, HfGgufFile)> {
    let (repo_id, resolved) = resolve_model_spec_full(spec).await?;
    if resolved.files.len() > 1 {
        bail!(
            "Model '{}' is sharded ({} files) - use resolve_model_spec_full instead",
            spec,
            resolved.files.len()
        );
    }
    Ok((repo_id, resolved.files.into_iter().next().unwrap()))
}

pub fn recommend_variant(
    variants: &[HfQuantVariant],
    available_memory_bytes: u64,
) -> Option<usize> {
    let usable = (available_memory_bytes as f64 * 0.85) as u64;
    let mut best: Option<usize> = None;
    for (index, variant) in variants.iter().enumerate() {
        if variant.size_bytes <= usable {
            match best {
                Some(best_index) if variants[best_index].quality_rank < variant.quality_rank => {
                    best = Some(index);
                }
                None => best = Some(index),
                _ => {}
            }
        }
    }
    best
}

fn matching_quantization_files<'a>(
    siblings: &'a [HfApiSibling],
    quant: &str,
) -> Vec<&'a HfApiSibling> {
    siblings
        .iter()
        .filter(|s| {
            s.rfilename.ends_with(".gguf")
                && !is_auxiliary_gguf_file(&s.rfilename)
                && parse_quantization(&s.rfilename).eq_ignore_ascii_case(quant)
        })
        .collect()
}

fn partition_shards<'a>(
    files: Vec<&'a HfApiSibling>,
) -> (Vec<&'a HfApiSibling>, Vec<&'a HfApiSibling>) {
    files
        .into_iter()
        .partition(|file| !is_shard_file(&file.rfilename))
}

fn validate_shard_set(repo_id: &str, quant: &str, shard_files: &[&HfApiSibling]) -> Result<()> {
    let expected_total = parse_shard_total(&shard_files[0].rfilename).ok_or_else(|| {
        anyhow::anyhow!(
            "Cannot parse shard total from '{}'",
            shard_files[0].rfilename
        )
    })?;
    if shard_files.len() != expected_total as usize {
        bail!(
            "Incomplete shard set for '{}' in {}: found {} of {} shards",
            quant,
            repo_id,
            shard_files.len(),
            expected_total
        );
    }
    for (i, shard) in shard_files.iter().enumerate() {
        let shard_total = parse_shard_total(&shard.rfilename);
        if shard_total != Some(expected_total) {
            bail!(
                "Inconsistent shard totals for '{}' in {}: shard '{}' has total {:?}, expected {}",
                quant,
                repo_id,
                shard.rfilename,
                shard_total,
                expected_total
            );
        }
        let index = parse_shard_index(&shard.rfilename);
        if index != Some((i + 1) as u32) {
            bail!(
                "Non-contiguous shard set for '{}' in {}: expected shard {} but found {:?}",
                quant,
                repo_id,
                i + 1,
                index
            );
        }
    }
    Ok(())
}

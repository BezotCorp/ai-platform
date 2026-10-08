use anyhow::{Result, bail};
use std::{collections, iter};

use crate::hf::api_sibling::HfApiSibling;
use crate::hf::gguf_catalog::{
    build_download_url, is_shard_file, mmproj_precision_preference, parse_quantization,
    parse_shard_index, parse_shard_total, quant_bits, quant_info, shard_set_key,
};
use crate::hf::gguf_file::HfGgufFile;
use crate::hf::quant_variant::HfQuantVariant;

fn is_complete_shard_set(files: &[&HfApiSibling]) -> bool {
    let Some(expected_total) = files
        .first()
        .and_then(|file| parse_shard_total(&file.rfilename))
    else {
        return false;
    };
    if files.len() != expected_total as usize {
        return false;
    }

    let mut indices = collections::HashSet::with_capacity(files.len());
    files.iter().all(|file| {
        parse_shard_total(&file.rfilename) == Some(expected_total)
            && parse_shard_index(&file.rfilename)
                .is_some_and(|index| index > 0 && index <= expected_total && indices.insert(index))
    })
}

pub(crate) fn select_preferred_shard_set(files: Vec<&HfApiSibling>) -> Vec<&HfApiSibling> {
    let mut shard_sets: collections::HashMap<(&str, &str), Vec<&HfApiSibling>> =
        collections::HashMap::new();

    for file in files {
        let key = shard_set_key(&file.rfilename)
            .expect("files passed to select_preferred_shard_set must be shards");
        shard_sets.entry(key).or_default().push(file);
    }

    shard_sets
        .into_iter()
        .min_by(|(key_a, files_a), (key_b, files_b)| {
            is_complete_shard_set(files_b)
                .cmp(&is_complete_shard_set(files_a))
                .then_with(|| (key_a.0.len() + key_a.1.len()).cmp(&(key_b.0.len() + key_b.1.len())))
                .then_with(|| key_a.cmp(key_b))
        })
        .map(|(_, files)| files)
        .unwrap_or_default()
}

fn parent_components(filename: &str) -> Vec<&str> {
    filename.rsplit_once('/').map_or(Vec::new(), |(parent, _)| {
        parent.split('/').filter(|part| !part.is_empty()).collect()
    })
}

fn is_prefix(prefix: &[&str], parts: &[&str]) -> bool {
    prefix.len() <= parts.len() && prefix.iter().zip(parts).all(|(a, b)| a == b)
}

fn normalize_family_fragment(value: &str, quantization: &str, projector: bool) -> String {
    let mut family = value.to_ascii_lowercase();
    if quantization != "unknown"
        && let Some(position) = family.rfind(quantization)
    {
        family.replace_range(position..position + quantization.len(), "");
    }
    if projector && let Some(position) = family.rfind("mmproj") {
        family.replace_range(position..position + "mmproj".len(), "");
    }

    family.retain(|character| character.is_ascii_alphanumeric());
    family
}

fn gguf_family_key(filename: &str, projector: bool) -> String {
    let basename = filename.rsplit('/').next().unwrap_or(filename);
    let mut stem = basename.trim_end_matches(".gguf");
    if let Some(pos) = stem.rfind("-of-") {
        stem = stem
            .get(..pos)
            .and_then(|prefix| prefix.rsplit_once('-').map(|(prefix, _)| prefix))
            .unwrap_or(stem);
    }

    let quantization = parse_quantization(filename).to_ascii_lowercase();
    normalize_family_fragment(stem, &quantization, projector)
}

fn gguf_family_identity(filename: &str) -> String {
    let quantization = parse_quantization(filename).to_ascii_lowercase();
    parent_components(filename)
        .into_iter()
        .map(|component| normalize_family_fragment(component, &quantization, false))
        .chain(iter::once(gguf_family_key(filename, false)))
        .filter(|component| !component.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

pub(crate) fn ensure_unambiguous_model_family(
    repo_id: &str,
    quantization: &str,
    files: &[&HfApiSibling],
) -> Result<()> {
    let families: collections::HashSet<String> = files
        .iter()
        .map(|file| gguf_family_identity(&file.rfilename))
        .collect();
    if families.len() > 1 {
        let mut filenames: Vec<&str> = files.iter().map(|file| file.rfilename.as_str()).collect();
        filenames.sort_unstable();
        bail!(
            "Quantization '{}' is ambiguous in {} because it belongs to multiple GGUF model families: {}",
            quantization,
            repo_id,
            filenames.join(", ")
        );
    }
    Ok(())
}

fn mmproj_matches_model_family(
    siblings: &[HfApiSibling],
    mmproj_filename: &str,
    model_filename: &str,
) -> bool {
    let mmproj_dir = parent_components(mmproj_filename);
    let model_families: collections::HashSet<String> = siblings
        .iter()
        .filter(|sibling| {
            sibling.rfilename.ends_with(".gguf")
                && !is_auxiliary_gguf_file(&sibling.rfilename)
                && is_prefix(&mmproj_dir, &parent_components(&sibling.rfilename))
        })
        .map(|sibling| gguf_family_identity(&sibling.rfilename))
        .collect();

    if model_families.len() <= 1 {
        return true;
    }

    let projector_family = gguf_family_key(mmproj_filename, true);
    !projector_family.is_empty() && projector_family == gguf_family_key(model_filename, false)
}

pub(crate) fn select_best_mmproj(
    repo_id: &str,
    siblings: &[HfApiSibling],
    model_filename: &str,
    model_quantization: &str,
) -> Option<HfGgufFile> {
    let model_dir = parent_components(model_filename);
    let model_bits = quant_bits(model_quantization);

    siblings
        .iter()
        .filter(|s| {
            let lowercase = s.rfilename.to_lowercase();
            lowercase.ends_with(".gguf") && lowercase.contains("mmproj")
        })
        .filter_map(|s| {
            let mmproj_dir = parent_components(&s.rfilename);
            if !is_prefix(&mmproj_dir, &model_dir)
                || !mmproj_matches_model_family(siblings, &s.rfilename, model_filename)
            {
                return None;
            }

            let quantization = parse_quantization(&s.rfilename);
            let bits = quant_bits(&quantization);
            let diff = bits.abs_diff(model_bits);
            let proximity = u8::MAX - diff;

            Some((
                mmproj_dir.len(),
                proximity,
                mmproj_precision_preference(&quantization),
                s,
                quantization,
            ))
        })
        .max_by(|a, b| {
            a.0.cmp(&b.0)
                .then_with(|| a.1.cmp(&b.1))
                .then_with(|| a.2.cmp(&b.2))
                .then_with(|| b.3.rfilename.cmp(&a.3.rfilename))
        })
        .map(|(_, _, _, sibling, quantization)| HfGgufFile {
            filename: sibling.rfilename.clone(),
            size_bytes: sibling.size.unwrap_or(0),
            quantization,
            download_url: build_download_url(repo_id, &sibling.rfilename),
        })
}

const AUXILIARY_TOKENS: &[&str] = &["encoder", "draft", "drafter", "adapter", "lora"];

fn contains_auxiliary_token(value: &str) -> bool {
    value
        .split(['-', '_', '.'])
        .any(|token| AUXILIARY_TOKENS.contains(&token))
}

/// Check whether a GGUF file ships alongside the weights rather than being a
/// downloadable variant itself (projectors, vision encoders, speculative drafters).
///
/// Filenames are matched by exclusion rather than by a repo-name prefix, because
/// publishers routinely rename files relative to the repo — Google drops the `qat`
/// segment, unsloth drops `MTP`, Qwen writes `Qwen3VL` for `Qwen3-VL`.
///
/// MTP drafters are published either under an `MTP/` directory or with a leading
/// `mtp-` on the basename; models whose *name* contains MTP carry it mid-name and
/// are real weights, so only the leading/directory forms count as auxiliary.
pub fn is_auxiliary_gguf_file(filename: &str) -> bool {
    let lowercase = filename.to_lowercase();

    if lowercase.contains("mmproj") {
        return true;
    }

    if parent_components(&lowercase)
        .into_iter()
        .any(|component| component == "mtp" || contains_auxiliary_token(component))
    {
        return true;
    }

    let basename = lowercase.rsplit('/').next().unwrap_or(&lowercase);
    let stem = basename.trim_end_matches(".gguf");

    stem.split(['-', '_', '.']).next() == Some("mtp") || contains_auxiliary_token(stem)
}

/// Collect GGUF files into quantization variants.
/// Single-file quants use the file directly.
/// Sharded quants (multiple files for one quantization) aggregate sizes and use the
/// first shard filename as the representative — the download path must handle all shards.
pub(crate) fn group_into_variants(
    repo_id: &str,
    files: Vec<HfApiSibling>,
) -> Result<Vec<HfQuantVariant>> {
    use std::collections::HashMap;
    let gguf_files: Vec<_> = files
        .into_iter()
        .filter(|s| {
            s.rfilename.ends_with(".gguf")
                && !is_auxiliary_gguf_file(&s.rfilename)
                && parse_quantization(&s.rfilename) != "unknown"
        })
        .collect();

    let mut files_by_quant: HashMap<String, Vec<&HfApiSibling>> = HashMap::new();
    for file in &gguf_files {
        files_by_quant
            .entry(parse_quantization(&file.rfilename))
            .or_default()
            .push(file);
    }
    for (quantization, files) in files_by_quant {
        ensure_unambiguous_model_family(repo_id, &quantization, &files)?;
    }

    // Separate single files from shards
    let mut single_files: Vec<&HfApiSibling> = Vec::new();
    let mut shard_groups: HashMap<String, Vec<&HfApiSibling>> = HashMap::new();

    for file in &gguf_files {
        if is_shard_file(&file.rfilename) {
            let quant = parse_quantization(&file.rfilename);
            shard_groups.entry(quant).or_default().push(file);
        } else {
            single_files.push(file);
        }
    }

    // A repo can expose the same family and quantization through multiple paths;
    // keep the plainest path so each quantization appears once.
    single_files.sort_by_key(|s| {
        (
            parse_quantization(&s.rfilename),
            s.rfilename.len(),
            s.rfilename.clone(),
        )
    });
    single_files.dedup_by_key(|s| parse_quantization(&s.rfilename));

    let mut variants: Vec<HfQuantVariant> = Vec::new();
    let mut seen_quants: collections::HashSet<String> = collections::HashSet::new();

    // Add single-file variants
    for s in single_files {
        let quant = parse_quantization(&s.rfilename);
        seen_quants.insert(quant.clone());
        let info = quant_info(&quant);
        let download_url = build_download_url(repo_id, &s.rfilename);
        variants.push(HfQuantVariant {
            quantization: quant,
            size_bytes: s.size.unwrap_or(0),
            filename: s.rfilename.clone(),
            download_url,
            description: info.description,
            quality_rank: info.quality_rank,
            sharded: false,
        });
    }

    // Add shard-only variants (quants that only exist as sharded files)
    for (quant, shards) in shard_groups {
        if seen_quants.contains(&quant) {
            continue;
        }
        let mut shards = select_preferred_shard_set(shards);
        shards.sort_by(|a, b| a.rfilename.cmp(&b.rfilename));
        let total_size: u64 = shards.iter().map(|s| s.size.unwrap_or(0)).sum();
        let info = quant_info(&quant);
        let first_filename = &shards[0].rfilename;
        let download_url = build_download_url(repo_id, first_filename);
        variants.push(HfQuantVariant {
            quantization: quant,
            size_bytes: total_size,
            filename: first_filename.clone(),
            download_url,
            description: info.description,
            quality_rank: info.quality_rank,
            sharded: true,
        });
    }

    // Sort descending by quality_rank, then by size descending as tiebreaker
    variants.sort_by(|a, b| {
        b.quality_rank
            .cmp(&a.quality_rank)
            .then_with(|| b.size_bytes.cmp(&a.size_bytes))
    });
    Ok(variants)
}

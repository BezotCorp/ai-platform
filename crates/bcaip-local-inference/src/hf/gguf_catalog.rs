use crate::local_model_format::HF_DOWNLOAD_BASE;

#[derive(Clone, Copy)]
pub(crate) struct QuantInfo {
    pub(crate) description: &'static str,
    pub(crate) quality_rank: u8,
}

// quality_rank groups quants by bit-level so that all N-bit variants sort
// together. Within a group, higher rank = higher quality.
//
//   1-bit:  10–19      4-bit:  40–49      8-bit:  80–89
//   2-bit:  20–29      5-bit:  50–59      16-bit: 90–94
//   3-bit:  30–39      6-bit:  60–69      32-bit: 95–99
//
const QUANT_TABLE: &[(&str, &str, u8)] = &[
    // 1-bit
    ("TQ1_0", "Tiny, ternary quantization", 10),
    ("IQ1_S", "Extremely small, very low quality", 11),
    ("IQ1_M", "Extremely small, very low quality", 12),
    // 2-bit
    ("IQ2_XXS", "Very small, low quality", 20),
    ("IQ2_XS", "Very small, low quality", 21),
    ("IQ2_S", "Very small, low quality", 22),
    ("IQ2_M", "Very small, low quality", 23),
    ("Q2_K", "Small, low quality", 24),
    ("Q2_K_S", "Small, low quality", 24),
    ("Q2_K_L", "Small, low quality", 25),
    ("Q2_K_XL", "Small, low quality", 26),
    // 3-bit
    ("IQ3_XXS", "Very small, moderate quality loss", 30),
    ("IQ3_XS", "Small, moderate quality loss", 31),
    ("IQ3_S", "Small, moderate quality loss", 32),
    ("IQ3_M", "Small, moderate quality loss", 33),
    ("Q3_K_S", "Small, moderate quality loss", 34),
    ("Q3_K_M", "Small, balanced quality/size", 35),
    ("Q3_K_L", "Medium-small, decent quality", 36),
    ("Q3_K_XL", "Medium-small, decent quality", 37),
    // 4-bit
    ("IQ4_XS", "Medium, good quality", 40),
    ("IQ4_NL", "Medium, good quality", 41),
    ("Q4_0", "Medium, good quality", 42),
    ("Q4_1", "Medium, good quality", 43),
    ("Q4_K_S", "Medium, good quality/size balance", 44),
    (
        "Q4_K_M",
        "Medium, recommended balance of quality and size",
        45,
    ),
    ("Q4_K_L", "Medium, good quality", 46),
    ("Q4_K_XL", "Medium, good quality", 47),
    (
        "MXFP4_MOE",
        "Medium, mixed-precision 4-bit for MoE models",
        48,
    ),
    // 5-bit
    ("Q5_0", "Medium-large, high quality", 50),
    ("Q5_1", "Medium-large, high quality", 51),
    ("Q5_K_S", "Medium-large, high quality", 52),
    ("Q5_K_M", "Medium-large, very high quality", 53),
    ("Q5_K_XL", "Medium-large, very high quality", 54),
    // 6-bit
    ("Q6_K", "Large, near-lossless quality", 60),
    ("Q6_K_XL", "Large, near-lossless quality", 61),
    // 8-bit
    ("Q8_0", "Large, near-lossless quality", 80),
    ("Q8_K_XL", "Large, near-lossless quality", 81),
    // 16-bit
    ("F16", "Full size, original quality (16-bit)", 90),
    ("BF16", "Full size, original quality (bfloat16)", 91),
    // 32-bit
    ("F32", "Full size, original quality (32-bit)", 95),
];

pub(crate) fn quant_info(quant: &str) -> QuantInfo {
    QUANT_TABLE
        .iter()
        .find(|(name, _, _)| *name == quant)
        .map(|(_, description, quality_rank)| QuantInfo {
            description,
            quality_rank: *quality_rank,
        })
        .unwrap_or(QuantInfo {
            description: "",
            quality_rank: 45,
        })
}

pub fn parse_quantization_from_filename(filename: &str) -> String {
    parse_quantization(filename)
}

pub(crate) fn parse_quantization(filename: &str) -> String {
    // Strip directory prefix (e.g. "Q5_K_M/Model-Q5_K_M-00001-of-00002.gguf")
    let basename = filename.rsplit('/').next().unwrap_or(filename);
    let stem = basename.trim_end_matches(".gguf");

    // Strip shard suffix like "-00001-of-00004"
    let stem = if let Some(pos) = stem.rfind("-of-") {
        stem.get(..pos)
            .and_then(|s| s.rsplit_once('-').map(|(prefix, _)| prefix))
            .unwrap_or(stem)
    } else {
        stem
    };

    // Publishers do not agree on where the quantization tag goes. Most append it
    // ("Model-Q4_K_M"), but some bury it mid-name ("gemma-4-26B_q4_0-it"). Scan
    // name components right-to-left and, within each, peel `_`-separated prefixes
    // so a tag fused to a neighbouring token still resolves. Whole components are
    // tested before their suffixes so tags absent from QUANT_TABLE survive intact
    // ("Q6_K_L" must not degrade to the "Q6_K" inside it).
    for component in stem.rsplit(['-', '.']) {
        let mut candidate = component;
        loop {
            if let Some(canonical) = canonical_quant(candidate) {
                return canonical.to_string();
            }
            if looks_like_quant(candidate) {
                return candidate.to_ascii_uppercase();
            }
            match candidate.split_once('_') {
                Some((_, rest)) => candidate = rest,
                None => break,
            }
        }
    }

    // Some publishers put a named preset rather than a quantization in the tag
    // position ("...-APEX-I-Quality"). Keep accepting a trailing Q-word so those
    // repos still expose the single variant they ship.
    if let Some((_, tail)) = stem.rsplit_once(['-', '.'])
        && tail.starts_with(['Q', 'q'])
    {
        return tail.to_string();
    }

    "unknown".to_string()
}

/// Resolve a tag to its QUANT_TABLE spelling, so casing differences between
/// publishers still hit the description and quality-rank lookup.
fn canonical_quant(candidate: &str) -> Option<&'static str> {
    QUANT_TABLE
        .iter()
        .map(|(name, _, _)| *name)
        .find(|name| name.eq_ignore_ascii_case(candidate))
}

pub(crate) fn quant_bits(quantization: &str) -> u8 {
    let digits: String = quantization
        .chars()
        .skip_while(|c| !c.is_ascii_digit())
        .take_while(|c| c.is_ascii_digit())
        .collect();
    digits.parse().unwrap_or(0)
}

pub(crate) fn mmproj_precision_preference(quantization: &str) -> u8 {
    match quantization.to_uppercase().as_str() {
        "BF16" => 3,
        "F16" => 2,
        "F32" => 1,
        _ => 0,
    }
}

/// Shape check for quantization tags missing from QUANT_TABLE (e.g. "Q6_K_L",
/// "Q4_0_4_4"). The family prefix must be followed by a digit, so model names
/// like "Qwen3" are not mistaken for quantizations.
fn looks_like_quant(s: &str) -> bool {
    let upper = s.to_uppercase();
    let digit_follows = |prefix: &str| {
        upper
            .strip_prefix(prefix)
            .and_then(|rest| rest.chars().next())
            .is_some_and(|c| c.is_ascii_digit())
    };

    digit_follows("Q")
        || digit_follows("IQ")
        || digit_follows("TQ")
        || digit_follows("MXFP")
        || upper == "F16"
        || upper == "F32"
        || upper == "BF16"
}

pub(crate) fn canonicalize_quantization(quantization: &str) -> String {
    canonical_quant(quantization)
        .map(str::to_string)
        .unwrap_or_else(|| {
            if looks_like_quant(quantization) {
                quantization.to_ascii_uppercase()
            } else {
                quantization.to_string()
            }
        })
}

pub(crate) fn is_shard_file(filename: &str) -> bool {
    // Matches patterns like "-00001-of-00003.gguf"
    parse_shard_index(filename).is_some()
}

pub(crate) fn shard_set_key(filename: &str) -> Option<(&str, &str)> {
    let stem = filename.trim_end_matches(".gguf");
    let pos = stem.rfind("-of-")?;
    let before = stem.get(..pos)?;
    let total = stem.get(pos + 4..)?;
    let (prefix, index) = before.rsplit_once('-')?;
    if !index.is_empty() && index.chars().all(|c| c.is_ascii_digit()) {
        Some((prefix, total))
    } else {
        None
    }
}

/// Parse the shard index (1-based) from a filename like "model-BF16-00001-of-00002.gguf".
pub(crate) fn parse_shard_index(filename: &str) -> Option<u32> {
    let basename = filename.rsplit('/').next().unwrap_or(filename);
    let stem = basename.trim_end_matches(".gguf");
    let pos = stem.rfind("-of-")?;
    let before = stem.get(..pos)?;
    let idx_str = before.rsplit('-').next()?;
    if !idx_str.is_empty() && idx_str.chars().all(|c| c.is_ascii_digit()) {
        idx_str.parse().ok()
    } else {
        None
    }
}

/// Parse the total shard count from a filename like "model-BF16-00001-of-00002.gguf".
pub(crate) fn parse_shard_total(filename: &str) -> Option<u32> {
    let basename = filename.rsplit('/').next().unwrap_or(filename);
    let stem = basename.trim_end_matches(".gguf");
    let pos = stem.rfind("-of-")?;
    let total_str = stem.get(pos + 4..)?;
    total_str.parse().ok()
}

pub(crate) fn build_download_url(repo_id: &str, filename: &str) -> String {
    format!("{}/{}/resolve/main/{}", HF_DOWNLOAD_BASE, repo_id, filename)
}

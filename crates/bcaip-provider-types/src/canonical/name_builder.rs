use once_cell::sync::Lazy;
use regex::Regex;
// Patterns for normalizing version numbers and stripping suffixes
static NORMALIZE_VERSION_RE: Lazy<Regex> = Lazy::new(|| Regex::new(r"-(\d)-(\d)(-|@|$)").unwrap());

static STRIP_PATTERNS: Lazy<Vec<Regex>> = Lazy::new(|| {
    vec![
        Regex::new(r"-latest$").unwrap(),
        Regex::new(r"-\d{8}$").unwrap(),
        Regex::new(r"@\d{8}$").unwrap(),
        Regex::new(r"-\d{4}$").unwrap(),
        Regex::new(r"-\d{4}-\d{2}-\d{2}$").unwrap(),
        Regex::new(r"-bedrock$").unwrap(),
    ]
});

static CLAUDE_PATTERNS: Lazy<Vec<(Regex, Regex, &'static str)>> = Lazy::new(|| {
    ["sonnet", "opus", "haiku", "fable"]
        .iter()
        .map(|&size| {
            (
                Regex::new(&format!("claude-([0-9.-]+)-{}", size)).unwrap(),
                Regex::new(&format!("claude-{}-([0-9.-]+)", size)).unwrap(),
                size,
            )
        })
        .collect()
});

/// Build canonical model name from provider and model identifiers
pub fn canonical_name(provider: &str, model: &str) -> String {
    let model_base = strip_version_suffix(model);
    format!("{}/{}", provider, model_base)
}

pub fn is_meta_provider(provider: &str) -> bool {
    matches!(
        provider,
        "databricks" | "databricks_v2" | "tetrate" | "bedrock" | "azure" | "azure_foundry"
    )
}

pub fn map_provider_name(provider: &str) -> &str {
    match provider {
        // Goose provider names that differ from models.dev names
        "xai" | "xai_oauth" => "x-ai",
        "azure_openai" | "azure_foundry" => "azure",
        "aws_bedrock" => "amazon-bedrock",
        "gcp_vertex_ai" => "google-vertex",
        "gemini_oauth" => "google",
        "chatgpt_codex" => "openai",
        "databricks_v2" => "databricks",
        "zhipu" => "zhipuai",
        "together" => "togetherai",
        "novita" => "novita-ai",
        "opencode_go" => "opencode-go",
        "opencode_zen" => "opencode",
        "ollama_cloud" => "ollama-cloud",
        "kimi_code" => "kimi-code-plan-cn",
        "muse_code" => "meta",
        _ => provider,
    }
}

/// Try to map a provider/model pair to a canonical model
pub fn map_to_canonical_model(
    provider: &str,
    model: &str,
    registry: &super::CanonicalModelRegistry,
) -> Option<String> {
    let registry_provider = map_provider_name(provider);

    if provider == "gcp_vertex_ai" {
        let normalized_model = strip_version_suffix(model);
        if let Some(canonical) = registry.get(registry_provider, &normalized_model) {
            return Some(canonical.id.clone());
        }
        if let Some(canonical) = registry.get(registry_provider, model) {
            return Some(canonical.id.clone());
        }
        if model.starts_with("gemini-") {
            return None;
        }
    }

    // For normal providers (anthropic, openai, google, openrouter, etc.), just do direct lookup
    if !is_meta_provider(provider) && provider != "gcp_vertex_ai" {
        let normalized_model = strip_version_suffix(model);
        if let Some(canonical) = registry.get(registry_provider, &normalized_model) {
            return Some(canonical.id.clone());
        }
        // Also try original model name
        if let Some(canonical) = registry.get(registry_provider, model) {
            return Some(canonical.id.clone());
        }
        // If direct lookup failed, fall through to inference logic below
    }

    // For hosting/meta-providers (or unknown providers), do string matching magic to figure out the real provider and model
    let model_stripped = strip_common_prefixes(model);

    if let Some(swapped) = swap_claude_word_order(&model_stripped)
        && let Some(inferred_provider) = infer_provider_from_model(&swapped)
    {
        let normalized = strip_version_suffix(&swapped);
        if let Some(canonical) = registry.get(inferred_provider, &normalized) {
            return Some(canonical.id.clone());
        }
    }

    if let Some(inferred_provider) = infer_provider_from_model(&model_stripped) {
        let normalized = strip_version_suffix(&model_stripped);
        if let Some(canonical) = registry.get(inferred_provider, &normalized) {
            return Some(canonical.id.clone());
        }
    }

    if let Some(inferred_provider) = infer_provider_from_model(model) {
        let normalized = strip_version_suffix(model);
        if let Some(canonical) = registry.get(inferred_provider, &normalized) {
            return Some(canonical.id.clone());
        }
    }

    if let Some((extracted_provider, extracted_model)) = extract_provider_prefix(&model_stripped) {
        let normalized = strip_version_suffix(extracted_model);
        if let Some(canonical) = registry.get(extracted_provider, &normalized) {
            return Some(canonical.id.clone());
        }
    }

    // Fallback for meta-providers: some native aliases are keyed under the
    // meta-provider itself (e.g. "databricks/databricks-gpt-oss-120b") and do
    // not infer back to a first-party provider. Only try this after inference,
    // so models that DO infer (e.g. databricks-claude-* -> anthropic/*) keep
    // resolving to the richer first-party catalog entry.
    if is_meta_provider(provider) {
        let normalized_model = strip_version_suffix(model);
        let lowercased_model = normalized_model.to_ascii_lowercase();
        // Registry keys are lowercase; deployment names may carry mixed case ("Phi-4").
        for key in [model, normalized_model.as_str(), lowercased_model.as_str()] {
            if let Some(canonical) = registry.get(registry_provider, key) {
                return Some(canonical.id.clone());
            }
        }
    }

    None
}

/// Swap word order for Claude models to handle both naming conventions
fn swap_claude_word_order(model: &str) -> Option<String> {
    if !model.starts_with("claude-") {
        return None;
    }

    for (forward_re, reverse_re, size) in CLAUDE_PATTERNS.iter() {
        if let Some(captures) = forward_re.captures(model) {
            let version = &captures[1];
            return Some(format!("claude-{}-{}", size, version));
        }

        if let Some(captures) = reverse_re.captures(model) {
            let version = &captures[1];
            return Some(format!("claude-{}-{}", version, size));
        }
    }

    None
}

/// Infer the real provider from model name patterns
fn infer_provider_from_model(model: &str) -> Option<&'static str> {
    let model_lower = model.to_lowercase();

    if model_lower.contains("claude") {
        return Some("anthropic");
    }

    if model_lower.starts_with("gpt-")
        || model_lower.starts_with("o1")
        || model_lower.starts_with("o3")
        || model_lower.starts_with("o4")
        || model_lower.starts_with("chatgpt-")
    {
        return Some("openai");
    }

    if model_lower.starts_with("gemini-") || model_lower.starts_with("gemma-") {
        return Some("google");
    }

    if model_lower.contains("llama") {
        return Some("meta-llama");
    }

    if model_lower.starts_with("mistral")
        || model_lower.starts_with("mixtral")
        || model_lower.starts_with("codestral")
        || model_lower.starts_with("ministral")
        || model_lower.starts_with("pixtral")
        || model_lower.starts_with("devstral")
        || model_lower.starts_with("voxtral")
    {
        return Some("mistralai");
    }

    if model_lower.contains("deepseek") {
        return Some("deepseek");
    }

    if model_lower.contains("qwen") {
        return Some("qwen");
    }

    if model_lower.contains("grok") {
        return Some("x-ai");
    }

    if model_lower.contains("jamba") {
        return Some("ai21");
    }

    if model_lower.contains("command") {
        return Some("cohere");
    }

    None
}

/// Strip common prefixes from model names using pattern matching
/// Looks for known model family patterns and strips everything before them
fn strip_common_prefixes(model: &str) -> String {
    let model_patterns = [
        "claude-",
        "gpt-",
        "gemini-",
        "gemma-",
        "o1-",
        "o1",
        "o3-",
        "o3",
        "o4-",
        "llama-",
        "mistral-",
        "mixtral-",
        "chatgpt-",
        "deepseek-",
        "qwen-",
        "grok-",
        "jamba-",
        "command-",
        "codestral",
        "ministral-",
        "pixtral-",
        "devstral-",
    ];

    let mut earliest_pos = None;

    for pattern in &model_patterns {
        if let Some(pos) = model.to_lowercase().find(pattern)
            && (earliest_pos.is_none() || pos < earliest_pos.unwrap())
        {
            earliest_pos = Some(pos);
        }
    }

    // If we found a pattern, strip everything before it
    if let Some(pos) = earliest_pos {
        return model.get(pos..).unwrap_or(model).to_string();
    }

    model.to_string()
}

/// Try to extract provider prefix from model names like "databricks-meta-llama-3-1-70b"
/// Returns (provider, model) tuple if found
fn extract_provider_prefix(model: &str) -> Option<(&'static str, &str)> {
    let known_providers = [
        "anthropic",
        "openai",
        "google",
        "meta-llama",
        "mistralai",
        "cohere",
        "ai21",
        "amazon",
        "deepseek",
        "qwen",
        "x-ai",
        "nvidia",
        "microsoft",
        "perplexity",
    ];

    for provider in &known_providers {
        let prefix = format!("{}-", provider);
        if model.starts_with(&prefix)
            && let Some(model_part) = model.strip_prefix(&prefix)
        {
            return Some((provider, model_part));
        }
    }

    None
}

/// Strip version suffixes from model names and normalize version numbers
pub fn strip_version_suffix(model: &str) -> String {
    let mut result = NORMALIZE_VERSION_RE
        .replace_all(model, "-$1.$2$3")
        .to_string();

    let mut changed = true;
    while changed {
        let before = result.clone();
        for pattern in STRIP_PATTERNS.iter() {
            result = pattern.replace(&result, "").to_string();
        }
        changed = result != before;
    }

    result
}

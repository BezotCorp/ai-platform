//! Cost estimation for model usage.
//!
//! Price resolution precedence (highest first):
//! 1. provider-reported costs (handled by callers before this module)
//! 2. prices the user declared in a custom provider config file — users of
//!    custom endpoints (negotiated rates, gateways, self-hosting) know their
//!    real prices better than a name-matched catalog entry
//! 3. the bundled canonical registry
//! 4. prices declared in bundled declarative provider definitions, which only
//!    fill registry gaps — they are vendored and may lag registry syncs. A
//!    registry price that is unset or zero (e.g. a name-inferred cross-provider
//!    match against a free listing) counts as a gap here.
//!
//! Note: a non-zero price the registry finds by cross-provider name inference
//! still outranks bundled provider-declared prices (a host may declare a
//! higher negotiated rate than the inferred catalog row). Demoting inferred
//! matches belongs in the canonical mapping layer, not here.
//!
//! Canonical cache rates are kept whenever the winning source does not declare
//! cache pricing, so cached tokens are not overestimated at the full input
//! rate. Config files are read live (price edits take effect immediately);
//! bundled definitions are immutable and cached for the process.

use crate::config::declarative_providers::custom_provider_file_path;
use bcaip_provider_types::base::ModelInfo;
use bcaip_provider_types::conversations::{CostSource, ProviderUsage, Usage};
use bcaip_provider_types::{Pricing, maybe_get_canonical_model};
use goose_providers::declarative::{
    DeclarativeProviderConfig, deserialize_provider_config, fixed_provider_configs,
};
use std::sync::OnceLock;
use tracing::warn;
const DEFAULT_CURRENCY: &str = "$";

/// Estimate the USD cost of a model invocation.
pub fn estimate_model_cost(provider: &str, model: &str, usage: &Usage) -> Option<f64> {
    resolve_pricing(provider, model).and_then(|pricing| pricing.estimate_cost(usage))
}

/// Resolve the cost of a provider-reported usage chunk: the provider's own
/// figure when present, otherwise a public-price estimate for the model, or
/// nothing when the model cannot be priced. Both agent loops call this so
/// estimated and provider-reported costs behave identically on either path.
pub(crate) fn resolve_usage_cost(
    provider: Option<&str>,
    usage: &ProviderUsage,
) -> (Option<f64>, Option<CostSource>) {
    if let Some(cost) = usage.cost {
        return (Some(cost), Some(CostSource::ProviderReported));
    }
    match provider.and_then(|provider| estimate_model_cost(provider, &usage.model, &usage.usage)) {
        Some(cost) => (Some(cost), Some(CostSource::Estimated)),
        None => (None, None),
    }
}

/// Resolve the pricing for a provider/model honoring the precedence described
/// in this module's documentation.
pub(crate) fn resolve_pricing(provider: &str, model: &str) -> Option<Pricing> {
    let canonical = maybe_get_canonical_model(provider, model).map(|c| c.cost);
    let bundled =
        bundled_model_info(provider, model).and_then(|info| pricing_from_model_info(&info));
    let base = match (canonical, bundled) {
        (Some(mut canonical), Some(bundled)) => {
            if is_price_gap(canonical.input) {
                canonical.input = bundled.input;
            }
            if is_price_gap(canonical.output) {
                canonical.output = bundled.output;
            }
            Some(canonical)
        }
        (Some(canonical), None) => Some(canonical),
        (None, bundled) => bundled,
    };

    let declared =
        custom_file_model_info(provider, model).and_then(|info| pricing_from_model_info(&info));
    match (declared, base) {
        (Some(declared), Some(base)) => Some(merge_pricing(declared, &base)),
        (Some(declared), None) => Some(declared),
        (None, base) => base,
    }
}

/// [`ModelInfo`] for a pricing-declared model — custom provider config file
/// first, then bundled definitions. Only models declaring both input and
/// output prices qualify: partial pricing cannot be estimated correctly
/// without silently pricing one direction at zero.
pub(crate) fn configured_model_info(provider: &str, model: &str) -> Option<ModelInfo> {
    custom_file_model_info(provider, model).or_else(|| bundled_model_info(provider, model))
}

/// The currency clients render alongside config-declared prices. Clients print
/// this verbatim as a symbol, so the ISO codes configs commonly use (bundled
/// definitions declare `USD`) are mapped to their symbol; anything else is
/// shown exactly as declared.
pub(crate) fn display_currency(info: Option<&ModelInfo>) -> String {
    info.and_then(|info| info.currency.as_deref())
        .map(currency_symbol)
        .unwrap_or_else(|| DEFAULT_CURRENCY.to_string())
}

fn currency_symbol(declared: &str) -> String {
    let declared = declared.trim();
    match declared.to_ascii_uppercase().as_str() {
        "" | "USD" => DEFAULT_CURRENCY.to_string(),
        "EUR" => "€".to_string(),
        "GBP" => "£".to_string(),
        "JPY" => "¥".to_string(),
        _ => declared.to_string(),
    }
}

/// Merge user-declared pricing with registry/bundled pricing: declared
/// input/output prices win; canonical cache rates fill the gaps so cached
/// tokens are not overestimated at the full input rate.
fn merge_pricing(mut declared: Pricing, base: &Pricing) -> Pricing {
    // A declared zero price means "free here": registry cache rates must not
    // reintroduce cost for cached tokens.
    if matches!(declared.input, Some(0.0)) && matches!(declared.output, Some(0.0)) {
        return declared;
    }
    if declared.cache_read.is_none() {
        declared.cache_read = base.cache_read;
    }
    if declared.cache_write.is_none() {
        declared.cache_write = base.cache_write;
    }
    declared
}

/// Convert a declarative [`ModelInfo`]'s per-token USD costs into canonical
/// [`Pricing`] (per-million-token USD).
fn pricing_from_model_info(info: &ModelInfo) -> Option<Pricing> {
    Some(Pricing {
        input: Some(info.input_token_cost.map(|c| c * 1_000_000.0)?),
        output: Some(info.output_token_cost.map(|c| c * 1_000_000.0)?),
        cache_read: None,
        cache_write: None,
    })
}

/// Read the custom provider config file for `provider` from disk. Reads are
/// live: price edits take effect without a restart.
fn custom_file_model_info(provider: &str, model: &str) -> Option<ModelInfo> {
    let path = custom_provider_file_path(provider).ok()?;
    if !path.exists() {
        return None;
    }
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(e) => {
            warn!(path = %path.display(), error = %e, "custom provider config unreadable; cost fallback disabled");
            return None;
        }
    };
    let config = match deserialize_provider_config(&content) {
        Ok(config) => config,
        Err(e) => {
            warn!(provider = %provider, error = %e, "custom provider config failed to parse; cost fallback disabled");
            return None;
        }
    };
    full_pricing_model(&config, model)
}

/// Bundled declarative provider definitions are embedded and immutable, so
/// they are cached for the lifetime of the process.
fn bundled_model_info(provider: &str, model: &str) -> Option<ModelInfo> {
    static CONFIGS: OnceLock<Vec<DeclarativeProviderConfig>> = OnceLock::new();
    let configs = CONFIGS.get_or_init(|| fixed_provider_configs().unwrap_or_default());
    configs
        .iter()
        .find(|config| config.name == provider)
        .and_then(|config| full_pricing_model(config, model))
}

/// A registry price that is unset or zero provides no usable rate signal
/// (e.g. a name-inferred cross-provider match against a free listing).
fn is_price_gap(price: Option<f64>) -> bool {
    matches!(price, None | Some(0.0))
}

fn full_pricing_model(config: &DeclarativeProviderConfig, model: &str) -> Option<ModelInfo> {
    config
        .models
        .iter()
        .find(|m| m.name == model)
        .filter(|info| info.input_token_cost.is_some() && info.output_token_cost.is_some())
        .cloned()
}

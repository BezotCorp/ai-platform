use serde::{Deserialize, Serialize};

use crate::conversations::Usage;
/// Modality types for model input/output
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Modality {
    Text,
    Image,
    Audio,
    Video,
    Pdf,
}

fn deserialize_modalities<'de, D>(deserializer: D) -> Result<Vec<Modality>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let strings: Vec<String> = Vec::deserialize(deserializer)?;
    Ok(strings
        .into_iter()
        .filter_map(|s| serde_json::from_value(serde_json::Value::String(s)).ok())
        .collect())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Modalities {
    /// Input modalities (e.g., [Text, Image, Pdf])
    #[serde(default, deserialize_with = "deserialize_modalities")]
    pub input: Vec<Modality>,

    /// Output modalities (e.g., [Text])
    #[serde(default, deserialize_with = "deserialize_modalities")]
    pub output: Vec<Modality>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Pricing {
    /// Cost in USD per million input tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input: Option<f64>,

    /// Cost in USD per million output tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<f64>,

    /// Cost per million cached read tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_read: Option<f64>,

    /// Cost per million cached write tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_write: Option<f64>,
}

impl Pricing {
    /// True when the entry carries no usable rate signal: `estimate_cost` needs both
    /// an input and an output price, so an unset or literal-zero value in either field
    /// makes the whole estimate wrong-low rather than merely incomplete. Mirrors the
    /// `is_price_gap` predicate in `bcaip::providers::canonical_cost`.
    pub fn has_no_usable_rate(&self) -> bool {
        fn is_gap(price: Option<f64>) -> bool {
            matches!(price, None | Some(0.0))
        }
        is_gap(self.input) || is_gap(self.output)
    }

    pub fn estimate_cost(&self, usage: &Usage) -> Option<f64> {
        let input_price = self.input?;
        let output_price = self.output?;
        let cache_read_price = self.cache_read.unwrap_or(input_price);
        let cache_write_price = self.cache_write.unwrap_or(input_price);

        let input_tokens = usage.input_tokens.unwrap_or(0).max(0) as f64;
        let output_tokens = usage.output_tokens.unwrap_or(0).max(0) as f64;
        let cache_read_tokens = usage.cache_read_input_tokens.unwrap_or(0).max(0) as f64;
        let cache_write_tokens = usage.cache_write_input_tokens.unwrap_or(0).max(0) as f64;
        let uncached_input_tokens =
            (input_tokens - cache_read_tokens - cache_write_tokens).max(0.0);

        Some(
            (uncached_input_tokens * input_price
                + cache_read_tokens * cache_read_price
                + cache_write_tokens * cache_write_price
                + output_tokens * output_price)
                / 1_000_000.0,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Limit {
    /// Maximum context window size in tokens
    pub context: usize,

    /// Maximum output/completion tokens
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingMode {
    Enabled,
    Adaptive,
    AlwaysOnAdaptive,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalModel {
    /// Model identifier (e.g., "anthropic/claude-3-5-sonnet")
    pub id: String,

    /// Human-readable name (e.g., "Claude Sonnet 3.5 v2")
    pub name: String,

    /// Model family (e.g., "claude-sonnet", "gpt")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,

    /// Whether the model supports attachments
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attachment: Option<bool>,

    /// Whether the model supports reasoning/thinking
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<bool>,

    /// Request shape to use when enabling thinking/reasoning.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub thinking_mode: Option<ThinkingMode>,

    /// Whether the model supports tool calling
    #[serde(default)]
    pub tool_call: bool,

    /// Whether the model supports temperature parameter
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<bool>,

    /// Knowledge cutoff date (e.g., "2024-04-30")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub knowledge: Option<String>,

    /// Release date (e.g., "2024-10-22")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_date: Option<String>,

    /// Last updated date (e.g., "2024-10-22")
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_updated: Option<String>,

    /// Input and output modalities
    #[serde(default)]
    pub modalities: Modalities,

    /// Whether the model has open weights
    #[serde(skip_serializing_if = "Option::is_none")]
    pub open_weights: Option<bool>,

    /// Pricing information
    #[serde(default)]
    pub cost: Pricing,

    /// Token limits
    #[serde(default)]
    pub limit: Limit,
}

use std::ops::{Add, AddAssign};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub model: String,
    pub usage: Usage,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stats: Option<ProviderStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost_source: Option<CostSource>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finish_reasons: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub response_id: Option<String>,
    /// Provider-specific response fields that have no canonical equivalent, kept
    /// unstructured so new fields flow through without changing this type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub additional_data: Option<Map<String, Value>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    ProviderReported,
    Estimated,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProviderStats {
    pub time_to_first_token_ms: Option<u64>,
    pub model_load_ms: Option<u64>,
    pub elapsed_ms: Option<u64>,
    pub output_tokens: Option<usize>,
    pub draft: Option<DraftStats>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DraftStats {
    pub model: Option<String>,
    pub draft_tokens: usize,
    pub accepted_tokens: usize,
    pub target_tokens: usize,
    pub rounds: usize,
    pub accept_rate: f64,
}

impl ProviderUsage {
    pub fn new(model: String, usage: Usage) -> Self {
        Self {
            model,
            usage,
            stats: None,
            cost: None,
            cost_source: None,
            finish_reasons: None,
            response_id: None,
            additional_data: None,
        }
    }

    pub fn with_stats(mut self, stats: ProviderStats) -> Self {
        self.stats = Some(stats);
        self
    }

    pub fn with_cost(mut self, cost: f64, source: CostSource) -> Self {
        self.cost = Some(cost);
        self.cost_source = Some(source);
        self
    }

    pub fn with_finish_reasons(mut self, reasons: Vec<String>) -> Self {
        self.finish_reasons = Some(reasons);
        self
    }

    pub fn with_response_id(mut self, id: String) -> Self {
        self.response_id = Some(id);
        self
    }
}

/// `input_tokens` is the total input including cache read/write tokens;
/// the cache fields are breakdown subsets of it. Parsers for providers
/// that report cache tokens separately from input (e.g. Anthropic,
/// Bedrock) must fold them into `input_tokens`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, Copy, PartialEq, Eq)]
pub struct Usage {
    /// All prompt tokens, including any served from or written to cache.
    /// `cache_read_input_tokens` and `cache_write_input_tokens` are subsets of this.
    pub input_tokens: Option<i32>,
    pub output_tokens: Option<i32>,
    pub total_tokens: Option<i32>,
    pub cache_read_input_tokens: Option<i32>,
    pub cache_write_input_tokens: Option<i32>,
}

fn sum_optionals<T>(a: Option<T>, b: Option<T>) -> Option<T>
where
    T: Add<Output = T>,
{
    match (a, b) {
        (Some(x), Some(y)) => Some(x + y),
        (Some(x), None) => Some(x),
        (None, Some(y)) => Some(y),
        (None, None) => None,
    }
}

impl Add for Usage {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self::new(
            sum_optionals(self.input_tokens, other.input_tokens),
            sum_optionals(self.output_tokens, other.output_tokens),
            sum_optionals(self.total_tokens, other.total_tokens),
        )
        .with_cache_tokens(
            sum_optionals(self.cache_read_input_tokens, other.cache_read_input_tokens),
            sum_optionals(
                self.cache_write_input_tokens,
                other.cache_write_input_tokens,
            ),
        )
    }
}

impl AddAssign for Usage {
    fn add_assign(&mut self, rhs: Self) {
        *self = *self + rhs;
    }
}

impl Usage {
    pub fn new(
        input_tokens: Option<i32>,
        output_tokens: Option<i32>,
        total_tokens: Option<i32>,
    ) -> Self {
        let calculated_total = if total_tokens.is_none() {
            match (input_tokens, output_tokens) {
                (Some(input), Some(output)) => Some(input.saturating_add(output)),
                (Some(input), None) => Some(input),
                (None, Some(output)) => Some(output),
                (None, None) => None,
            }
        } else {
            total_tokens
        };

        Self {
            input_tokens,
            output_tokens,
            total_tokens: calculated_total,
            cache_read_input_tokens: None,
            cache_write_input_tokens: None,
        }
    }

    pub fn with_cache_tokens(
        mut self,
        cache_read_input_tokens: Option<i32>,
        cache_write_input_tokens: Option<i32>,
    ) -> Self {
        self.cache_read_input_tokens = cache_read_input_tokens;
        self.cache_write_input_tokens = cache_write_input_tokens;
        self
    }

    /// For providers whose reported `input_tokens`/`total_tokens` exclude
    /// cache tokens (e.g. Anthropic, Bedrock): folds the cache breakdown in.
    pub fn from_cache_exclusive_input(
        input_tokens: Option<i32>,
        output_tokens: Option<i32>,
        total_tokens: Option<i32>,
        cache_read_input_tokens: Option<i32>,
        cache_write_input_tokens: Option<i32>,
    ) -> Self {
        let cache_tokens = cache_read_input_tokens
            .unwrap_or(0)
            .saturating_add(cache_write_input_tokens.unwrap_or(0));
        Self::new(
            input_tokens.map(|v| v.saturating_add(cache_tokens)),
            output_tokens,
            total_tokens.map(|v| v.saturating_add(cache_tokens)),
        )
        .with_cache_tokens(cache_read_input_tokens, cache_write_input_tokens)
    }
}

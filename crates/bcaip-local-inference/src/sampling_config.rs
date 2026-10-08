use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum SamplingConfig {
    Greedy,
    Temperature {
        temperature: f32,
        top_k: i32,
        top_p: f32,
        min_p: f32,
        seed: Option<u32>,
    },
    MirostatV2 {
        tau: f32,
        eta: f32,
        seed: Option<u32>,
    },
}

impl Default for SamplingConfig {
    fn default() -> Self {
        SamplingConfig::Temperature {
            temperature: 0.8,
            top_k: 40,
            top_p: 0.95,
            min_p: 0.05,
            seed: None,
        }
    }
}

use crate::mlx::mlx_error::mlx_error;
use crate::mlx::mlx_generation::MlxGeneration;
use crate::mlx::mlx_stream_emitter::MlxStreamEmitter;
use crate::model::ModelSettings;
use bcaip_provider_types::errors::ProviderError;
use safemlx::transforms::eval;
use safemlx::{Array, Stream};
use safemlx_lm::models::LoadedModel;
use safemlx_lm_utils::tokenizer::Tokenizer;

pub(crate) fn generate_single_model(
    model: &mut LoadedModel,
    tokenizer: &Tokenizer,
    prompt_array: &Array,
    eos_token_ids: &[u32],
    max_tokens: usize,
    temp: f32,
    prng_key: Option<Array>,
    stream: &Stream,
    generation_started: std::time::Instant,
    mut emitter: MlxStreamEmitter<'_>,
) -> Result<MlxGeneration, ProviderError> {
    let mut cache = model.new_cache();
    let mut generated_ids = Vec::new();
    let mut streamed_text = String::new();
    let mut time_to_first_token_ms = None;
    let stream_generation = emitter.can_stream();
    let mut decode_stream = tokenizer.decode_stream(true);
    {
        let generator = model
            .generate_with_cache(&mut cache, temp, prompt_array, prng_key, stream)
            .take(max_tokens);
        for token in generator {
            let token = token.map_err(mlx_error)?;
            eval([&token]).map_err(mlx_error)?;
            let token_id = token.item::<u32>(stream);
            time_to_first_token_ms.get_or_insert_with(|| {
                u64::try_from(generation_started.elapsed().as_millis()).unwrap_or(u64::MAX)
            });
            if eos_token_ids.contains(&token_id) {
                break;
            }
            generated_ids.push(token_id);
            if stream_generation {
                if let Some(piece) = decode_stream.step(token_id).map_err(mlx_error)? {
                    if !piece.is_empty() {
                        let should_continue = emitter.push_text(&piece)?;
                        streamed_text.push_str(&piece);
                        if !should_continue {
                            break;
                        }
                    }
                }
            }
        }
    }
    let generated_text = tokenizer.decode(&generated_ids, true).map_err(mlx_error)?;
    let streamed_response = if stream_generation {
        match final_stream_suffix(&generated_text, &streamed_text)? {
            Some(suffix) => {
                if !suffix.is_empty() {
                    emitter.push_text(suffix)?;
                }
                true
            }
            None => false,
        }
    } else {
        false
    };
    if streamed_response {
        emitter.finish()?;
    }
    Ok(MlxGeneration {
        generated_ids,
        generated_text,
        draft_stats: None,
        time_to_first_token_ms,
        streamed_response,
    })
}
fn final_stream_suffix<'a>(
    generated_text: &'a str,
    streamed_text: &str,
) -> Result<Option<&'a str>, ProviderError> {
    if streamed_text.is_empty() {
        return Ok(None);
    }

    generated_text
        .strip_prefix(streamed_text)
        .map(Some)
        .ok_or_else(|| mlx_error("streamed MLX decode did not match final tokenizer decode"))
}
pub(crate) fn mlx_max_tokens(
    settings: &ModelSettings,
    request_max_tokens: Option<i32>,
    context_limit: usize,
    prompt_tokens: usize,
) -> usize {
    let configured_max = settings
        .max_output_tokens
        .or_else(|| request_max_tokens.and_then(|tokens| usize::try_from(tokens).ok()));
    if context_limit == 0 {
        return configured_max.unwrap_or(4096);
    }

    let context_headroom = context_limit.saturating_sub(prompt_tokens);
    configured_max
        .map(|max| max.min(context_headroom))
        .unwrap_or(context_headroom)
}

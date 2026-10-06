use super::generation::{generate_single_model, mlx_max_tokens};
use super::model_validation::{mlx_stop_token_ids, model_dir_from_path};
use super::sampling::{prng_key, sampling};
use super::tool_mode::ToolMode;
use super::{
    mlx_error::mlx_error, mlx_generation::MlxGeneration, mlx_loaded_model::MlxLoadedModel,
    mlx_stream_emitter::MlxStreamEmitter,
};
use super::{output::emit_generated_response, prompt::build_prompt};
use crate::ResolvedModelPaths;
use crate::backend::{BackendLoadedModel, LocalGenerationRequest, LocalInferenceBackend};
use crate::model::{ModelSettings, ToolCallingMode};
use crate::tool_emulation::CODE_EXECUTION_TOOL;
use goose_provider_types::conversations::{DraftStats, ProviderStats, ProviderUsage, Usage};
use goose_provider_types::errors::ProviderError;
use safemlx::{Device, DeviceType, Stream};
use safemlx_lm::models::{LoadedModel, Model};
use safemlx_lm::{
    gemma4_mtp::generate_gemma4_mtp, models::gemma4_assistant::load_gemma4_assistant_model,
};
use safemlx_lm_utils::tokenizer::Tokenizer;
use serde_json::json;
pub(crate) const MLX_BACKEND_ID: &str = "mlx";
pub(crate) struct MlxBackend;
impl MlxBackend {
    pub(crate) fn new() -> Self {
        Self
    }
}
impl LocalInferenceBackend for MlxBackend {
    fn id(&self) -> &'static str {
        MLX_BACKEND_ID
    }

    fn load_model(
        &self,
        model_id: &str,
        resolved: &ResolvedModelPaths,
        _settings: &ModelSettings,
    ) -> Result<Box<dyn BackendLoadedModel>, ProviderError> {
        if !resolved.model_path.exists() {
            return Err(ProviderError::ExecutionError(format!(
                "Model not downloaded: {}. Please download it from Settings > Local Inference.",
                model_id
            )));
        }

        let model_dir = model_dir_from_path(&resolved.model_path)?;
        let stream = Stream::new_with_device(&Device::new(DeviceType::Gpu, 0));
        let weights_stream = Stream::new_with_device(&Device::new(DeviceType::Cpu, 0));
        let model = LoadedModel::load(&model_dir, &stream, &weights_stream).map_err(mlx_error)?;
        let tokenizer =
            Tokenizer::from_file(model_dir.join("tokenizer.json")).map_err(mlx_error)?;
        tracing::info!(
            backend = self.id(),
            model_id,
            model_type = model.model_type(),
            "MLX model loaded successfully"
        );
        let stop_token_ids = mlx_stop_token_ids(&model, &model_dir);
        Ok(Box::new(MlxLoadedModel {
            model,
            tokenizer,
            model_dir,
            stop_token_ids,
        }))
    }

    fn generate(
        &self,
        loaded: &mut dyn BackendLoadedModel,
        request: LocalGenerationRequest<'_>,
    ) -> Result<(), ProviderError> {
        let loaded = loaded
            .as_any_mut()
            .downcast_mut::<MlxLoadedModel>()
            .ok_or_else(|| {
                ProviderError::ExecutionError("Loaded model backend mismatch".to_string())
            })?;

        let stream = Stream::new_with_device(&Device::new(DeviceType::Gpu, 0));
        let tool_mode = if request.tools.is_empty() {
            ToolMode::None
        } else {
            match request.settings.tool_calling {
                ToolCallingMode::ForceNative => ToolMode::Native,
                ToolCallingMode::Auto | ToolCallingMode::ForceEmulated => ToolMode::Emulated {
                    code_mode_enabled: request.tools.iter().any(|t| t.name == CODE_EXECUTION_TOOL),
                },
            }
        };
        let prompt = build_prompt(
            &mut loaded.model,
            &request.model_name,
            request.system,
            request.messages,
            request.tools,
            tool_mode,
        )?;
        let prompt_tokens = loaded.model.encode(&prompt, false).map_err(mlx_error)?;
        if prompt_tokens.len() >= request.context_limit && request.context_limit > 0 {
            return Err(ProviderError::ContextLengthExceeded(format!(
                "Prompt ({} tokens) exceeds context limit ({} tokens). Try reducing conversation length.",
                prompt_tokens.len(),
                request.context_limit
            )));
        }

        let prompt_array = loaded
            .model
            .encode_to_array(&prompt, false, &stream)
            .map_err(mlx_error)?;
        let max_tokens = mlx_max_tokens(
            request.settings,
            request.max_tokens,
            request.context_limit,
            prompt_tokens.len(),
        );
        let (settings_temp, seed) = sampling(request.settings);
        let temp = request.temperature.unwrap_or(settings_temp);
        let prng_key = prng_key(temp, seed)?;
        let eos_token_ids = loaded.stop_token_ids.clone();
        let generation_started = std::time::Instant::now();
        let MlxGeneration {
            generated_ids,
            generated_text,
            draft_stats,
            time_to_first_token_ms,
            streamed_response,
        } = if let Some(draft_model_path) = &request.draft_model_path {
            if matches!(loaded.model.model_mut(), Model::Gemma4(_)) {
                let weights_stream = Stream::new_with_device(&Device::new(DeviceType::Cpu, 0));
                let mut assistant =
                    load_gemma4_assistant_model(draft_model_path, &stream, &weights_stream)
                        .map_err(|error| {
                            mlx_error(format!("failed to load MLX draft model: {error}"))
                        })?;
                let target = match loaded.model.model_mut() {
                    Model::Gemma4(target) => target,
                    _ => unreachable!(),
                };
                let (ids, stats) = generate_gemma4_mtp(
                    target,
                    &mut assistant,
                    &prompt_array,
                    &eos_token_ids,
                    max_tokens,
                    temp,
                    prng_key,
                    &stream,
                )
                .map_err(mlx_error)?;
                let generated_text = loaded.tokenizer.decode(&ids, true).map_err(mlx_error)?;
                MlxGeneration {
                    generated_ids: ids,
                    generated_text,
                    draft_stats: Some(DraftStats {
                        model: Some(draft_model_path.display().to_string()),
                        draft_tokens: stats.draft_tokens,
                        accepted_tokens: stats.accepted_tokens,
                        target_tokens: stats.target_tokens,
                        rounds: stats.rounds,
                        accept_rate: stats.accept_rate(),
                    }),
                    time_to_first_token_ms: None,
                    streamed_response: false,
                }
            } else {
                generate_single_model(
                    &mut loaded.model,
                    &loaded.tokenizer,
                    &prompt_array,
                    &eos_token_ids,
                    max_tokens,
                    temp,
                    prng_key,
                    &stream,
                    generation_started,
                    MlxStreamEmitter::new(
                        request.message_id,
                        tool_mode,
                        request.settings.enable_thinking,
                        &prompt,
                        request.tx,
                    ),
                )?
            }
        } else {
            generate_single_model(
                &mut loaded.model,
                &loaded.tokenizer,
                &prompt_array,
                &eos_token_ids,
                max_tokens,
                temp,
                prng_key,
                &stream,
                generation_started,
                MlxStreamEmitter::new(
                    request.message_id,
                    tool_mode,
                    request.settings.enable_thinking,
                    &prompt,
                    request.tx,
                ),
            )?
        };

        if !streamed_response {
            emit_generated_response(
                &generated_text,
                &prompt,
                request.settings.enable_thinking,
                request.message_id,
                tool_mode,
                request.tx,
            )?;
        }

        let output_tokens = generated_ids.len() as i32;
        let input_tokens = prompt_tokens.len() as i32;
        let usage = Usage::new(
            Some(input_tokens),
            Some(output_tokens),
            Some(input_tokens + output_tokens),
        );
        let log_json = serde_json::json!({
            "path": "mlx",
            "model_dir": loaded.model_dir,
            "prompt_tokens": input_tokens,
            "output_tokens": output_tokens,
            "model_load_ms": request.model_load_ms,
            "time_to_first_token_ms": time_to_first_token_ms,
            "elapsed_ms": generation_started.elapsed().as_millis() as u64,
            "generated_text": generated_text,
            "draft": draft_stats,
        });
        let _ = request.log.write(&log_json, Some(&usage));
        let stats = ProviderStats {
            time_to_first_token_ms,
            model_load_ms: request.model_load_ms,
            elapsed_ms: Some(generation_started.elapsed().as_millis() as u64),
            output_tokens: Some(generated_ids.len()),
            draft: draft_stats,
        };
        let provider_usage = ProviderUsage::new(request.model_name, usage).with_stats(stats);
        let _ = request.tx.blocking_send(Ok((None, Some(provider_usage))));
        Ok(())
    }

    fn available_memory_bytes(&self) -> u64 {
        0
    }
}

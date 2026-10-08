mod chat_template;
mod chat_template_params;
mod chat_template_result;
mod inference_emulated_tools;
mod inference_engine;
mod inference_native_tools;
mod llama_cpp_backend;
mod loaded_model;
mod python_separators;
mod stop_suffix_trimmer;

pub use llama_cpp_backend::LlamaCppBackend;
pub(crate) use loaded_model::LoadedModel;
pub(crate) use stop_suffix_trimmer::StopSuffixTrimmer;

use anyhow::Result;
use llama_cpp_2::model::{LlamaChatTemplate, LlamaModel};
use llama_cpp_2::{LlamaBackendDeviceType, list_llama_ggml_backend_devices};
use std::ffi::CStr;

use self::inference_engine::LoadedChatTemplates;
use self::{chat_template::apply_chat_template, chat_template_params::ChatTemplateParams};
use crate::model::{ChatTemplate, ModelSettings, ToolCallingMode};
use bcaip_provider_types::errors::ProviderError;
pub(crate) const LLAMACPP_BACKEND_ID: &str = "llamacpp";

const CODE_EXECUTION_TOOL: &str = "code_execution__execute_typescript";

pub(crate) fn builtin_chat_template_names() -> Vec<String> {
    let count = unsafe { llama_cpp_sys_2::llama_chat_builtin_templates(std::ptr::null_mut(), 0) };
    if count <= 0 {
        return Vec::new();
    }

    let mut templates = vec![std::ptr::null(); count as usize];
    let written = unsafe {
        llama_cpp_sys_2::llama_chat_builtin_templates(templates.as_mut_ptr(), templates.len())
    };
    templates.truncate(written.max(0) as usize);

    templates
        .into_iter()
        .filter(|ptr| !ptr.is_null())
        .filter_map(|ptr| {
            unsafe { CStr::from_ptr(ptr) }
                .to_str()
                .ok()
                .map(str::to_string)
        })
        .collect()
}

fn supports_native_tool_calling(
    loaded: &LoadedModel,
    settings: &ModelSettings,
    template: &LlamaChatTemplate,
    oai_messages_json: &str,
    tools_json: Option<&str>,
) -> bool {
    let Some(tools_json) = tools_json.filter(|tools| !tools.trim().is_empty()) else {
        return false;
    };

    let params = ChatTemplateParams {
        messages_json: oai_messages_json,
        tools_json: Some(tools_json),
        enable_thinking: settings.enable_thinking,
    };

    match apply_chat_template(&loaded.model, template, &params) {
        Ok(result) => result.supports_native_tool_calling(),
        Err(e) => {
            tracing::debug!(
                error = %e,
                "chat template dry-run did not support native tool calling"
            );
            false
        }
    }
}

fn should_use_native_tool_calling(
    mode: ToolCallingMode,
    has_tools: bool,
    template_supports_native: bool,
) -> bool {
    has_tools
        && match mode {
            ToolCallingMode::Auto => template_supports_native,
            ToolCallingMode::ForceNative => true,
            ToolCallingMode::ForceEmulated => false,
        }
}

fn is_legacy_builtin_template_name(template: &str) -> bool {
    matches!(
        template.trim(),
        "bailing"
            | "bailing-think"
            | "bailing2"
            | "chatglm3"
            | "chatglm4"
            | "command-r"
            | "deepseek"
            | "deepseek-ocr"
            | "deepseek2"
            | "deepseek3"
            | "exaone-moe"
            | "exaone3"
            | "exaone4"
            | "falcon3"
            | "gemma"
            | "gigachat"
            | "glmedge"
            | "gpt-oss"
            | "granite"
            | "granite-4.0"
            | "grok-2"
            | "hunyuan-dense"
            | "hunyuan-moe"
            | "hunyuan-ocr"
            | "kimi-k2"
            | "llama2"
            | "llama2-sys"
            | "llama2-sys-bos"
            | "llama2-sys-strip"
            | "llama3"
            | "llama4"
            | "megrez"
            | "minicpm"
            | "mistral-v1"
            | "mistral-v3"
            | "mistral-v3-tekken"
            | "mistral-v7"
            | "mistral-v7-tekken"
            | "monarch"
            | "openchat"
            | "orion"
            | "pangu-embedded"
            | "phi3"
            | "phi4"
            | "rwkv-world"
            | "seed_oss"
            | "smolvlm"
            | "solar-open"
            | "vicuna"
            | "vicuna-orca"
            | "yandex"
            | "zephyr"
    )
}

fn missing_chat_template_error(
    model_id: &str,
    architecture: Option<&str>,
    context: &str,
    has_tool_use_template: bool,
) -> ProviderError {
    let architecture = architecture
        .map(str::trim)
        .filter(|arch| !arch.is_empty())
        .map(|arch| format!(" Detected GGUF general.architecture={arch}."))
        .unwrap_or_default();
    let tool_use_note = if has_tool_use_template {
        " A named tool_use chat template is present, but that template is only used for native tool calls with tools present."
    } else {
        ""
    };

    ProviderError::ExecutionError(format!(
        "Model {model_id} does not contain GGUF tokenizer.chat_template metadata required for {context}.{architecture}{tool_use_note} \
         BCAIP cannot safely infer the correct prompt format from architecture alone. Select a \
         llama.cpp built-in chat template name, configure a custom inline chat template containing \
         the full Jinja template source, or use a GGUF that includes tokenizer.chat_template metadata."
    ))
}

fn load_chat_templates(
    model: &LlamaModel,
    settings: &ModelSettings,
) -> Result<LoadedChatTemplates, ProviderError> {
    match &settings.chat_template {
        ChatTemplate::Embedded => Ok(LoadedChatTemplates {
            default: model.chat_template(None).ok(),
            tool_use: model.chat_template(Some("tool_use")).ok(),
            force_default: false,
        }),
        ChatTemplate::Builtin { name } => {
            let trimmed = name.trim();
            if trimmed.is_empty() {
                return Err(ProviderError::ExecutionError(
                    "Built-in chat template name is empty. Enter a llama.cpp built-in template name such as 'chatml', or use embedded chat template metadata.".to_string(),
                ));
            }
            LlamaChatTemplate::new(trimmed)
                .map_err(|e| {
                    ProviderError::ExecutionError(format!(
                        "Built-in chat template name contains an invalid NUL byte: {e}"
                    ))
                })
                .map(|template| LoadedChatTemplates {
                    default: Some(template),
                    tool_use: None,
                    force_default: true,
                })
        }
        ChatTemplate::CustomInline { template } => {
            let trimmed = template.trim();
            if trimmed.is_empty() {
                return Err(ProviderError::ExecutionError(
                    "Custom inline chat template is empty. Paste the full Jinja chat template source, use a llama.cpp built-in template name, or use embedded chat template metadata.".to_string(),
                ));
            }
            if trimmed == "chatml" || is_legacy_builtin_template_name(trimmed) {
                return Err(ProviderError::ExecutionError(format!(
                    "Custom inline chat template is set to '{trimmed}', which is a llama.cpp template name rather than Jinja template source. Paste the full Jinja chat template source instead, or select Built-in and enter '{trimmed}' if that built-in template is intended."
                )));
            }
            LlamaChatTemplate::new(template)
                .map_err(|e| {
                    ProviderError::ExecutionError(format!(
                        "Custom inline chat template contains an invalid NUL byte: {e}"
                    ))
                })
                .map(|template| LoadedChatTemplates {
                    default: Some(template),
                    tool_use: None,
                    force_default: true,
                })
        }
    }
}

fn select_generation_template<'a>(
    model_id: &str,
    model: &LlamaModel,
    templates: &'a LoadedChatTemplates,
    native_tool_calling: bool,
    has_tools: bool,
) -> Result<&'a LlamaChatTemplate, ProviderError> {
    if templates.force_default {
        return templates.default.as_ref().ok_or_else(|| {
            ProviderError::ExecutionError(
                "Configured chat template was not loaded correctly".to_string(),
            )
        });
    }

    if native_tool_calling
        && has_tools
        && let Some(template) = templates.tool_use.as_ref()
    {
        return Ok(template);
    }

    templates.default.as_ref().ok_or_else(|| {
        let architecture = model.meta_val_str("general.architecture").ok();
        let context = if has_tools && native_tool_calling {
            "native tool calling because no tool_use template is available"
        } else if has_tools {
            "emulated tool calling"
        } else {
            "chat without tools"
        };
        missing_chat_template_error(
            model_id,
            architecture.as_deref(),
            context,
            templates.tool_use.is_some(),
        )
    })
}

#[cfg(target_arch = "x86_64")]
fn unsupported_cpu_features_error_message(missing_features: &[&str]) -> String {
    format!(
        "Local inference with the bundled llama.cpp backend requires CPU support for {}. \
         This CPU is missing {}. Use a CPU with those instruction sets or switch to a non-local provider.",
        missing_features.join(" and "),
        missing_features.join(", ")
    )
}

#[cfg(target_arch = "x86_64")]
fn check_cpu_supports_local_inference() -> Result<()> {
    let missing_features = [
        (!std::arch::is_x86_feature_detected!("fma")).then_some("FMA"),
        (!std::arch::is_x86_feature_detected!("avx2")).then_some("AVX2"),
        (!std::arch::is_x86_feature_detected!("f16c")).then_some("F16C"),
        (!std::arch::is_x86_feature_detected!("bmi2")).then_some("BMI2"),
        (!std::arch::is_x86_feature_detected!("sse4.2")).then_some("SSE4.2"),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();

    if missing_features.is_empty() {
        Ok(())
    } else {
        Err(anyhow::anyhow!(unsupported_cpu_features_error_message(
            &missing_features
        )))
    }
}

#[cfg(not(target_arch = "x86_64"))]
fn check_cpu_supports_local_inference() -> Result<()> {
    Ok(())
}

fn is_accelerator_device(device_type: LlamaBackendDeviceType) -> bool {
    matches!(
        device_type,
        LlamaBackendDeviceType::Gpu
            | LlamaBackendDeviceType::IntegratedGpu
            | LlamaBackendDeviceType::Accelerator
    )
}

fn is_non_cpu_device(device_type: LlamaBackendDeviceType) -> bool {
    !matches!(device_type, LlamaBackendDeviceType::Cpu)
}

fn log_inference_backend_devices() {
    let devices = list_llama_ggml_backend_devices();
    let non_cpu_devices: Vec<_> = devices
        .iter()
        .filter(|device| is_non_cpu_device(device.device_type))
        .collect();

    if non_cpu_devices.is_empty() {
        tracing::info!(
            device_count = devices.len(),
            "No non-CPU llama.cpp backend devices detected for local inference"
        );
        return;
    }

    for device in non_cpu_devices {
        tracing::info!(
            index = device.index,
            backend = %device.backend,
            name = %device.name,
            description = %device.description,
            device_type = ?device.device_type,
            memory_total_bytes = device.memory_total as u64,
            memory_free_bytes = device.memory_free as u64,
            "Non-CPU llama.cpp backend device detected for local inference"
        );
    }
}

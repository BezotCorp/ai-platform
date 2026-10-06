use llama_cpp_2::model::{LlamaChatMessage, LlamaChatTemplate, LlamaModel};
use llama_cpp_2::{token::LlamaToken, vocab::LlamaVocab};
use minijinja::value::Kwargs;
use minijinja::{Environment, Error, ErrorKind, Value};
use serde::Serialize;
use serde_json::{Value as Json, json};

use crate::{
    llamacpp::{
        chat_template_params::ChatTemplateParams, chat_template_result::ChatTemplateResult,
        python_separators::PythonSeparators,
    },
    native_tool_format::NativeToolFormat,
};
pub(crate) fn apply_chat_template(
    model: &LlamaModel,
    template: &LlamaChatTemplate,
    params: &ChatTemplateParams<'_>,
) -> Result<ChatTemplateResult, String> {
    let source = template.to_str().map_err(|e| e.to_string())?;
    let messages: Vec<Json> = serde_json::from_str(params.messages_json)
        .map_err(|e| format!("Invalid chat messages JSON: {e}"))?;
    let tools = parse_tools(params.tools_json)?;

    if !is_jinja_source(source) {
        return apply_builtin_template(model, template, &messages);
    }

    let vocab = model.vocab();
    let bos_token = special_piece(&vocab, vocab.bos());
    let eos_token = special_piece(&vocab, vocab.eos());
    let rendered = render_jinja_template(
        source,
        &messages,
        tools.as_deref(),
        params.enable_thinking,
        &bos_token,
        &eos_token,
    );
    let (prompt, generation_prompt) = match rendered {
        Ok(rendered) => rendered,
        Err(render_error) if tools.is_none() => {
            tracing::warn!(
                error = %render_error,
                "Jinja chat template could not be rendered, falling back to llama.cpp's built-in template matching"
            );
            return apply_builtin_template(model, template, &messages).map_err(|fallback_error| {
                format!("{render_error}; llama.cpp template fallback also failed: {fallback_error}")
            });
        }
        Err(render_error) => return Err(render_error),
    };

    let tool_format = tools.as_ref().map(|_| {
        source
            .contains("tools")
            .then(|| NativeToolFormat::detect(source))
            .flatten()
            .unwrap_or(NativeToolFormat::Generic)
    });

    Ok(ChatTemplateResult {
        prompt,
        generation_prompt,
        additional_stops: Vec::new(),
        tool_format,
    })
}

fn parse_tools(tools_json: Option<&str>) -> Result<Option<Vec<Json>>, String> {
    let Some(tools_json) = tools_json.filter(|tools| !tools.trim().is_empty()) else {
        return Ok(None);
    };
    let tools: Vec<Json> =
        serde_json::from_str(tools_json).map_err(|e| format!("Invalid tools JSON: {e}"))?;
    Ok((!tools.is_empty()).then_some(tools))
}

fn is_jinja_source(source: &str) -> bool {
    source.contains("{%") || source.contains("{{") || source.contains("{#")
}

fn special_piece(vocab: &LlamaVocab<'_>, token: LlamaToken) -> String {
    if token.0 < 0 {
        return String::new();
    }
    String::from_utf8_lossy(&vocab.token_to_piece(token, true, None)).into_owned()
}

fn apply_builtin_template(
    model: &LlamaModel,
    template: &LlamaChatTemplate,
    messages: &[Json],
) -> Result<ChatTemplateResult, String> {
    let chat = messages
        .iter()
        .map(|message| {
            let role = message
                .get("role")
                .and_then(Json::as_str)
                .unwrap_or("user")
                .to_string();
            LlamaChatMessage::new(role, message_text(message)).map_err(|e| e.to_string())
        })
        .collect::<Result<Vec<_>, _>>()?;

    let prompt = model
        .apply_chat_template(template, &chat, true)
        .map_err(|e| e.to_string())?;
    let base = model
        .apply_chat_template(template, &chat, false)
        .map_err(|e| e.to_string())?;

    Ok(ChatTemplateResult {
        generation_prompt: generation_suffix(&base, &prompt),
        prompt,
        additional_stops: Vec::new(),
        tool_format: None,
    })
}

fn message_text(message: &Json) -> String {
    match message.get("content") {
        Some(Json::String(text)) => text.clone(),
        Some(Json::Array(parts)) => parts
            .iter()
            .filter_map(|part| part.get("text").and_then(Json::as_str))
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn generation_suffix(base: &str, with_generation_prompt: &str) -> String {
    with_generation_prompt
        .strip_prefix(base)
        .unwrap_or_default()
        .to_string()
}

/// Renders a Hugging Face style chat template. Returns the full prompt and the part of it that
/// was added by `add_generation_prompt`.
fn render_jinja_template(
    source: &str,
    messages: &[Json],
    tools: Option<&[Json]>,
    enable_thinking: bool,
    bos_token: &str,
    eos_token: &str,
) -> Result<(String, String), String> {
    let mut messages = messages.to_vec();
    if template_requires_object_arguments(source) {
        parse_tool_call_arguments(&mut messages);
    }

    let mut env = jinja_environment();
    env.add_template_owned("chat", source.to_string())
        .map_err(|e| format!("Invalid chat template: {e:#}"))?;
    let template = env.get_template("chat").map_err(|e| e.to_string())?;

    let render = |add_generation_prompt: bool| -> Result<String, String> {
        let mut context = json!({
            "messages": messages,
            "add_generation_prompt": add_generation_prompt,
            "bos_token": bos_token,
            "eos_token": eos_token,
            "enable_thinking": enable_thinking,
            "date_string": chrono::Utc::now().format("%d %b %Y").to_string(),
        });
        if let Some(tools) = tools {
            context["tools"] = Json::Array(tools.to_vec());
        }
        template
            .render(&context)
            .map_err(|e| format!("Failed to render chat template: {e:#}"))
    };

    let prompt = render(true)?;
    let base = render(false)?;
    let generation_prompt = generation_suffix(&base, &prompt);
    Ok((prompt, generation_prompt))
}

fn jinja_environment() -> Environment<'static> {
    let mut env = Environment::new();
    env.set_trim_blocks(true);
    env.set_lstrip_blocks(true);
    env.set_unknown_method_callback(minijinja_contrib::pycompat::unknown_method_callback);
    env.add_function(
        "raise_exception",
        |message: String| -> Result<Value, Error> {
            Err(Error::new(ErrorKind::InvalidOperation, message))
        },
    );
    env.add_function("strftime_now", |format: String| {
        chrono::Utc::now().format(&format).to_string()
    });
    env.add_filter("tojson", tojson);
    env
}

/// Mirrors Hugging Face's `tojson`: Python-style separators and unescaped non-ASCII.
fn tojson(value: Value, kwargs: Kwargs) -> Result<String, Error> {
    let indent: Option<usize> = kwargs.get("indent")?;
    let json = serde_json::to_value(&value)
        .map_err(|e| Error::new(ErrorKind::BadSerialization, e.to_string()))?;
    let mut out = Vec::new();
    let result = match indent {
        Some(width) => {
            let indent = vec![b' '; width];
            let formatter = serde_json::ser::PrettyFormatter::with_indent(&indent);
            json.serialize(&mut serde_json::Serializer::with_formatter(
                &mut out, formatter,
            ))
        }
        None => json.serialize(&mut serde_json::Serializer::with_formatter(
            &mut out,
            PythonSeparators,
        )),
    };
    result.map_err(|e| Error::new(ErrorKind::BadSerialization, e.to_string()))?;
    String::from_utf8(out).map_err(|e| Error::new(ErrorKind::BadSerialization, e.to_string()))
}

/// Templates that re-serialise or iterate `arguments` expect an object, whereas OpenAI-format
/// messages carry it as a JSON string.
fn template_requires_object_arguments(source: &str) -> bool {
    [
        "arguments | tojson",
        "arguments|tojson",
        "arguments.items()",
        "arguments | items",
    ]
    .iter()
    .any(|pattern| source.contains(pattern))
}

fn parse_tool_call_arguments(messages: &mut [Json]) {
    let tool_calls = messages
        .iter_mut()
        .filter_map(|message| message.get_mut("tool_calls"))
        .filter_map(Json::as_array_mut)
        .flatten();

    for tool_call in tool_calls {
        let Some(arguments) = tool_call
            .get_mut("function")
            .and_then(|function| function.get_mut("arguments"))
        else {
            continue;
        };
        if let Some(parsed) = arguments
            .as_str()
            .and_then(|text| serde_json::from_str::<Json>(text).ok())
            .filter(Json::is_object)
        {
            *arguments = parsed;
        }
    }
}

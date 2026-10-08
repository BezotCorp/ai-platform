use crate::api_client::ApiClient;
use anyhow::Result;
use async_trait::async_trait;
use bcaip_provider_types::images::ImageFormat;
use serde_json::{Value, json};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
pub type OpenRouterSessionIdProvider = Box<dyn Fn() -> Option<String> + Send + Sync>;
use crate::decision::{DecisionProvider, DecisionRequest, DecisionResponse};
use crate::http_status::read_json_response;
use crate::openai_compatible::{handle_status, stream_openai_compat};
use crate::openrouter_format;
use bcaip_provider_types::base::{
    ConfigKey, MessageStream, Provider, ProviderDescriptor, ProviderMetadata,
};
use bcaip_provider_types::cache_semantics::{CacheSemantics, apply_chat_payload_breakpoints};
use bcaip_provider_types::conversations::Message;
use bcaip_provider_types::errors::ProviderError;
use bcaip_provider_types::formats::create_request_openai;
use bcaip_provider_types::model::ModelConfig;
use bcaip_provider_types::request_log::{LoggerHandleExt, start_log};
use bcaip_provider_types::retry::ProviderRetry;
use rmcp::model::Tool;
pub const OPENROUTER_PROVIDER_NAME: &str = "openrouter";
const OPENROUTER_PARAMETERS_CONFIG_KEY: &str = "OPENROUTER_PARAMETERS";
pub const OPENROUTER_DEFAULT_MODEL: &str = "anthropic/claude-sonnet-4";
pub const OPENROUTER_DECISION_DEFAULT_MODEL: &str = "typesafe/jev-1.13";

// OpenRouter can run many models, we suggest the default
pub const OPENROUTER_KNOWN_MODELS: &[&str] = &[
    "x-ai/grok-code-fast-1",
    "anthropic/claude-sonnet-4.5",
    "anthropic/claude-sonnet-4",
    "anthropic/claude-opus-4.1",
    "anthropic/claude-opus-4",
    "google/gemini-2.5-pro",
    "google/gemini-2.5-flash",
    "deepseek/deepseek-r1-0528",
    "qwen/qwen3-coder",
    "moonshotai/kimi-k2",
];
pub const OPENROUTER_DOC_URL: &str = "https://openrouter.ai/models";

const GEMINI_SCHEMA_REF_KEY: &str = "$ref";
const GEMINI_SAFE_SCHEMA_REF_KEY_BASE: &str = "dollar_ref";

#[derive(serde::Serialize)]
pub struct OpenRouterProvider {
    #[serde(skip)]
    api_client: ApiClient,
    supports_streaming: bool,
    #[serde(skip)]
    name: String,
    #[serde(skip)]
    configured_parameters: Option<HashMap<String, Value>>,
    #[serde(skip)]
    session_id_provider: Option<OpenRouterSessionIdProvider>,
}

impl OpenRouterProvider {
    pub fn new(
        api_client: ApiClient,
        configured_parameters: Option<HashMap<String, Value>>,
        session_id_provider: Option<OpenRouterSessionIdProvider>,
    ) -> Self {
        Self {
            api_client,
            supports_streaming: true,
            name: OPENROUTER_PROVIDER_NAME.to_string(),
            configured_parameters,
            session_id_provider,
        }
    }

    async fn post_chat_completions(
        &self,
        model_config: &ModelConfig,
        payload: &Value,
    ) -> Result<reqwest::Response, ProviderError> {
        self.with_retry(|| async {
            let resp = self
                .api_client
                .request("api/v1/chat/completions")
                .model_headers(model_config)?
                .streaming(true)
                .response_post(payload)
                .await?;
            handle_status(resp).await
        })
        .await
    }
}

fn is_mandatory_reasoning_error(error: &ProviderError) -> bool {
    matches!(error, ProviderError::RequestFailed(message) if message.contains("Reasoning is mandatory"))
}

fn is_gemini_model(model_name: &str) -> bool {
    model_name.starts_with("google/gemini")
}

/// Spans of the literal `$ref` token inside opaque tool text.
///
/// Tool results are not required to parse as JSON. Google rejects the token in
/// single-quoted Python `repr` output, YAML, and unquoted text just as it does
/// in strict JSON, so matching only well-formed JSON key positions would miss
/// the reproduction in #11260. A trailing identifier character means the token
/// is part of a longer name such as `$refs` or `$refresh_token`, which must be
/// left alone.
fn scan_schema_ref_tokens(content: &str) -> Vec<Range<usize>> {
    let mut spans = Vec::new();
    let mut cursor = 0;

    while cursor < content.len() {
        if !content.is_char_boundary(cursor) {
            cursor += 1;
            continue;
        }

        match match_schema_ref_at(content, cursor) {
            Some(end)
                if !starts_with_escaped_backslash(content, cursor)
                    && !continues_identifier(content, end) =>
            {
                spans.push(cursor..end);
                cursor = end;
            }
            _ => {
                cursor += content
                    .get(cursor..)
                    .and_then(|rest| rest.chars().next())
                    .map_or(1, char::len_utf8);
            }
        }
    }

    spans
}

/// Decodes one character: either a literal one or a `\uXXXX` escape. JSON lets
/// any character of a key be escaped, so `$\u0072ef` and `\u0024\u0072\u0065\u0066`
/// both decode to `$ref` and would otherwise reach Gemini unrewritten.
fn decode_unit(content: &str, index: usize) -> Option<(char, usize)> {
    let rest = content.get(index..)?;

    if let Some(after_prefix) = rest.strip_prefix("\\u") {
        let character = after_prefix
            .get(..4)
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .and_then(char::from_u32)?;
        return Some((character, index + "\\u".len() + 4));
    }

    let character = rest.chars().next()?;
    Some((character, index + character.len_utf8()))
}

/// Returns the end offset when the text at `start` decodes to `$ref`.
fn match_schema_ref_at(content: &str, start: usize) -> Option<usize> {
    let mut cursor = start;

    for expected in GEMINI_SCHEMA_REF_KEY.chars() {
        let (character, next) = decode_unit(content, cursor)?;
        if character != expected {
            return None;
        }
        cursor = next;
    }

    Some(cursor)
}

/// An odd number of backslashes before `\u0024ref` means the leading backslash
/// is itself escaped, so the text is the literal characters `\u0024ref` rather
/// than an encoded `$ref`. Rewriting it would emit invalid JSON.
fn starts_with_escaped_backslash(content: &str, start: usize) -> bool {
    if !content
        .get(start..)
        .is_some_and(|token| token.starts_with('\\'))
    {
        return false;
    }

    let preceding_backslashes = content
        .get(..start)
        .map(|prefix| prefix.chars().rev().take_while(|c| *c == '\\').count())
        .unwrap_or(0);

    preceding_backslashes % 2 == 1
}

/// A trailing identifier character means the token is part of a longer name
/// such as `$refs` or `$refresh_token`, which must be left alone.
fn continues_identifier(content: &str, end: usize) -> bool {
    decode_unit(content, end)
        .is_some_and(|(character, _)| character.is_ascii_alphanumeric() || character == '_')
}

/// Decodes `\uXXXX` escapes so a candidate key spelled as an escape sequence
/// still counts as occupied.
fn decoded_view(content: &str) -> String {
    let mut decoded = String::with_capacity(content.len());
    let mut rest = content;

    while let Some(offset) = rest.find("\\u") {
        let (before, from_escape) = rest.split_at(offset);
        decoded.push_str(before);

        let after_prefix = from_escape.get("\\u".len()..).unwrap_or_default();
        match after_prefix
            .get(..4)
            .and_then(|hex| u32::from_str_radix(hex, 16).ok())
            .and_then(char::from_u32)
        {
            Some(character) => {
                decoded.push(character);
                rest = after_prefix.get(4..).unwrap_or_default();
            }
            None => {
                decoded.push_str("\\u");
                rest = after_prefix;
            }
        }
    }
    decoded.push_str(rest);

    decoded
}

fn replace_spans(content: &str, spans: &[Range<usize>], replacement: &str) -> String {
    let mut rewritten = String::with_capacity(content.len());
    let mut copied_through = 0;

    for span in spans {
        if let Some(preceding) = content.get(copied_through..span.start) {
            rewritten.push_str(preceding);
        }
        rewritten.push_str(replacement);
        copied_through = span.end;
    }
    if let Some(trailing) = content.get(copied_through..) {
        rewritten.push_str(trailing);
    }

    rewritten
}

/// The replacement must not already occur anywhere in the tool text, otherwise
/// the rewrite would be ambiguous to the model reading the compatibility note.
///
/// Tool output is externally controlled, so occupied candidates are collected in
/// a single pass rather than rescanning every result for each suffix.
fn collision_free_gemini_schema_ref_key(contents: &[&str]) -> String {
    let mut occupied = HashSet::new();
    for content in contents {
        collect_safe_key_candidates(content, &mut occupied);
        collect_safe_key_candidates(&decoded_view(content), &mut occupied);
    }

    (1..)
        .map(|suffix| {
            if suffix == 1 {
                GEMINI_SAFE_SCHEMA_REF_KEY_BASE.to_string()
            } else {
                format!("{GEMINI_SAFE_SCHEMA_REF_KEY_BASE}_{suffix}")
            }
        })
        .find(|candidate| !occupied.contains(candidate))
        .expect("an unbounded suffix sequence always yields an unused candidate")
}

/// Records every `dollar_ref`/`dollar_ref_N` occurrence in one scan.
fn collect_safe_key_candidates(content: &str, occupied: &mut HashSet<String>) {
    let mut cursor = 0;

    while let Some(offset) = content
        .get(cursor..)
        .and_then(|tail| tail.find(GEMINI_SAFE_SCHEMA_REF_KEY_BASE))
    {
        let start = cursor + offset;
        let end = start + GEMINI_SAFE_SCHEMA_REF_KEY_BASE.len();
        let suffix_len = content
            .get(end..)
            .map(|rest| {
                let digits = rest
                    .strip_prefix('_')
                    .map(|after| after.chars().take_while(char::is_ascii_digit).count())
                    .unwrap_or(0);
                if digits == 0 { 0 } else { 1 + digits }
            })
            .unwrap_or(0);

        if let Some(token) = content.get(start..end + suffix_len) {
            occupied.insert(token.to_string());
        }
        cursor = end;
    }
}

fn gemini_schema_ref_note(safe_key: &str) -> String {
    format!(
        "[OpenRouter/Gemini compatibility: interpret `{safe_key}` as the JSON Schema key formed by `$` followed by `ref`.]\n"
    )
}

fn apply_gemini_compatibility(model_name: &str, payload: &mut Value, messages: &[Message]) {
    if is_gemini_model(model_name) {
        escape_gemini_schema_ref_keys_in_tool_responses(payload);
        openrouter_format::add_reasoning_details_to_request(payload, messages);
    }
}

/// OpenRouter translates OpenAI `role: tool` messages into Gemini
/// `function_response` parts. Gemini rejects a response containing a literal
/// JSON Schema `$ref` key, treating its value as a function-response part name
/// instead of arbitrary tool text, and BCAIP replays persisted history, so one
/// such tool result breaks every later turn in the session.
///
/// Rewrite the token and prepend a note so the model can reconstruct the
/// original text. All bytes outside matching token spans remain unchanged.
fn escape_gemini_schema_ref_keys_in_tool_responses(payload: &mut Value) -> usize {
    let Some(messages) = payload.get_mut("messages").and_then(Value::as_array_mut) else {
        return 0;
    };

    let mut tool_contents = Vec::new();
    for (message_index, message) in messages.iter().enumerate() {
        if message.get("role").and_then(Value::as_str) != Some("tool") {
            continue;
        }
        let Some(content_text) = message.get("content").and_then(Value::as_str) else {
            continue;
        };
        tool_contents.push((message_index, content_text.to_string()));
    }

    let scanned: Vec<&str> = tool_contents
        .iter()
        .map(|(_, content)| content.as_str())
        .collect();
    let safe_key = collision_free_gemini_schema_ref_key(&scanned);
    let note = gemini_schema_ref_note(&safe_key);

    let mut escaped = 0;
    for (message_index, content_text) in &tool_contents {
        let spans = scan_schema_ref_tokens(content_text);
        if spans.is_empty() {
            continue;
        }

        let sanitized = replace_spans(content_text, &spans, &safe_key);
        messages[*message_index]["content"] = Value::String(format!("{note}{sanitized}"));
        escaped += spans.len();
    }

    escaped
}

fn merge_request_params(
    request_params: &mut Option<HashMap<String, Value>>,
    params: HashMap<String, Value>,
) {
    request_params
        .get_or_insert_with(HashMap::new)
        .extend(params);
}

fn merge_openrouter_parameters(model: &mut ModelConfig, params: HashMap<String, Value>) {
    merge_request_params(&mut model.request_params, params);
}

impl ProviderDescriptor for OpenRouterProvider {
    fn metadata() -> ProviderMetadata {
        ProviderMetadata::new(
            OPENROUTER_PROVIDER_NAME,
            "OpenRouter",
            "Router for many model providers",
            OPENROUTER_DEFAULT_MODEL,
            OPENROUTER_KNOWN_MODELS.to_vec(),
            OPENROUTER_DOC_URL,
            vec![
                ConfigKey::new("OPENROUTER_API_KEY", true, true, None, true),
                ConfigKey::new(
                    "OPENROUTER_HOST",
                    false,
                    false,
                    Some("https://openrouter.ai"),
                    false,
                ),
                ConfigKey::new(OPENROUTER_PARAMETERS_CONFIG_KEY, false, false, None, false),
            ],
        )
    }
}

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize, PartialEq)]
pub struct OpenRouterDecisionOptions {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
}

impl OpenRouterProvider {
    pub async fn create_decision_with_options(
        &self,
        request: &DecisionRequest,
        options: &OpenRouterDecisionOptions,
    ) -> Result<DecisionResponse, ProviderError> {
        let mut payload = serde_json::to_value(request).map_err(|error| {
            ProviderError::RequestFailed(format!("Failed to serialize Decisions request: {error}"))
        })?;
        let option_values = serde_json::to_value(options).map_err(|error| {
            ProviderError::RequestFailed(format!(
                "Failed to serialize OpenRouter Decisions options: {error}"
            ))
        })?;
        if let (Some(payload), Some(options)) = (payload.as_object_mut(), option_values.as_object())
        {
            payload.extend(options.clone());
        }
        let response = self
            .api_client
            .request("api/alpha/decisions")
            .response_post(&payload)
            .await?;
        let response = handle_status(response).await?;
        read_json_response(response).await
    }
}

#[async_trait]
impl DecisionProvider for OpenRouterProvider {
    async fn create_decision(
        &self,
        request: &DecisionRequest,
    ) -> Result<DecisionResponse, ProviderError> {
        self.create_decision_with_options(request, &OpenRouterDecisionOptions::default())
            .await
    }
}

#[async_trait]
impl Provider for OpenRouterProvider {
    fn get_name(&self) -> &str {
        &self.name
    }

    fn skip_canonical_filtering(&self) -> bool {
        true
    }

    async fn fetch_recommended_models(&self, toolshim: bool) -> Result<Vec<String>, ProviderError> {
        let response = self
            .api_client
            .request("api/v1/models")
            .response_get()
            .await
            .map_err(|e| {
                ProviderError::RequestFailed(format!(
                    "Failed to fetch models from OpenRouter API: {}",
                    e
                ))
            })?;

        let json: serde_json::Value = response.json().await.map_err(|e| {
            ProviderError::RequestFailed(format!(
                "Failed to parse OpenRouter API response as JSON: {}",
                e
            ))
        })?;

        if let Some(err_obj) = json.get("error") {
            let msg = err_obj
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown error");
            return Err(ProviderError::RequestFailed(format!(
                "OpenRouter API returned an error: {}",
                msg
            )));
        }

        let data = json.get("data").and_then(|v| v.as_array()).ok_or_else(|| {
            ProviderError::UsageError("Missing data field in JSON response".into())
        })?;

        let mut models: Vec<String> = data
            .iter()
            .filter_map(|model| {
                let id = model.get("id").and_then(|v| v.as_str())?;
                if toolshim {
                    return Some(id.to_string());
                }
                let supports_tools = model
                    .get("supported_parameters")
                    .and_then(|v| v.as_array())
                    .is_some_and(|params| params.iter().any(|p| p.as_str() == Some("tools")));
                if supports_tools {
                    Some(id.to_string())
                } else {
                    None
                }
            })
            .collect();
        models.sort();
        Ok(models)
    }

    async fn stream(
        &self,
        model_config: &ModelConfig,
        system: &str,
        messages: &[Message],
        tools: &[Tool],
    ) -> Result<MessageStream, ProviderError> {
        let session_id = self
            .session_id_provider
            .as_ref()
            .and_then(|provider| provider())
            .unwrap_or_default();

        let mut merged_model;
        let model_config = if let Some(params) = &self.configured_parameters {
            merged_model = model_config.clone();
            merge_openrouter_parameters(&mut merged_model, params.clone());
            &merged_model
        } else {
            model_config
        };

        let mut payload = create_request_openai(
            model_config,
            system,
            messages,
            tools,
            &ImageFormat::OpenAi,
            true,
        )?;

        if !session_id.is_empty()
            && let Some(obj) = payload.as_object_mut()
        {
            obj.insert("user".to_string(), Value::String(session_id.to_string()));
            obj.insert(
                "session_id".to_string(),
                Value::String(session_id.to_string()),
            );
        }

        if CacheSemantics::for_model(OPENROUTER_PROVIDER_NAME, &model_config.model_name)
            .uses_explicit_breakpoints()
            && !model_config.prompt_cache_disabled()
        {
            apply_chat_payload_breakpoints(&mut payload);
        }

        apply_gemini_compatibility(&model_config.model_name, &mut payload, messages);
        let sent_reasoning_disable =
            openrouter_format::apply_reasoning_config(&mut payload, model_config);

        if let Some(obj) = payload.as_object_mut() {
            obj.insert("transforms".to_string(), json!(["middle-out"]));
            obj.insert("usage".to_string(), json!({ "include": true }));
        }

        let mut log = start_log(model_config, &payload)?;

        let response = match self.post_chat_completions(model_config, &payload).await {
            // Mandatory-reasoning endpoints reject the disable request, so
            // downgrade to the lowest effort they all accept and retry once.
            Err(error) if sent_reasoning_disable && is_mandatory_reasoning_error(&error) => {
                let _ = log.error(&error);
                payload["reasoning"] = json!({ "effort": "low" });
                log = start_log(model_config, &payload)?;
                self.post_chat_completions(model_config, &payload).await
            }
            result => result,
        }
        .inspect_err(|e| {
            let _ = log.error(e);
        })?;

        stream_openai_compat(response, log)
    }
}

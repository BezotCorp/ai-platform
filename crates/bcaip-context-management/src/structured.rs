use crate::templates::{self, COMPACTION_SUMMARY_TEMPLATE};
use bcaip_provider_types::json::safely_parse_json;
use serde::{Deserialize, Serialize};

/// Structured output of the compaction LLM call.
///
/// Every list is ordered most-important-first so consumers (the render
/// template, experiments that truncate sections) can cut from the tail.
/// Fields deserialize leniently - omitted fields default to empty, and an
/// object or number where a string was asked for is stringified rather than
/// failing - because models routinely enrich the schema (e.g. `{"error": ..,
/// "fix": ..}` entries) and one such field must not discard a good summary.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StructuredSummary {
    #[serde(default, deserialize_with = "lenient_string_list")]
    pub user_intent: Vec<String>,
    #[serde(default, deserialize_with = "lenient_string_list")]
    pub technical_concepts: Vec<String>,
    #[serde(default, deserialize_with = "lenient_file_list")]
    pub files: Vec<FileActivity>,
    #[serde(default, deserialize_with = "lenient_string_list")]
    pub errors_and_fixes: Vec<String>,
    #[serde(default, deserialize_with = "lenient_string_list")]
    pub problem_solving: Vec<String>,
    #[serde(default, deserialize_with = "lenient_string_list")]
    pub user_messages: Vec<String>,
    #[serde(default, deserialize_with = "lenient_string_list")]
    pub pending_tasks: Vec<String>,
    #[serde(default, deserialize_with = "lenient_string_opt")]
    pub current_work: Option<String>,
    #[serde(default, deserialize_with = "lenient_string_opt")]
    pub next_step: Option<String>,
    /// Unknown top-level fields, kept so a user-customized compaction prompt
    /// that adds fields can still reach them from a customized render
    /// template. Not counted when deciding whether a summary is empty.
    #[serde(flatten)]
    pub extra: serde_json::Map<String, serde_json::Value>,
}

impl StructuredSummary {
    /// Returns `None` when no usable JSON document is found so the caller can
    /// keep the raw response text - the lossless fallback.
    pub fn parse(response_text: &str) -> Option<Self> {
        json_candidates(response_text).into_iter().find_map(|c| {
            let value = safely_parse_json(c).ok()?;
            let mut summary: Self = serde_json::from_value(value).ok()?;
            summary.normalize();
            (!summary.is_empty()).then_some(summary)
        })
    }

    pub fn render(&self) -> Result<String, minijinja::Error> {
        self.render_with(
            &templates::builtin_template(COMPACTION_SUMMARY_TEMPLATE)
                .expect("builtin compaction summary template"),
        )
    }

    pub fn render_with(&self, template: &str) -> Result<String, minijinja::Error> {
        templates::render(template, self)
    }

    /// Drops blank entries so a response of blank strings counts as empty
    /// (raw-text fallback) rather than rendering a summary of nothing.
    fn normalize(&mut self) {
        fn blank(s: &str) -> bool {
            s.trim().is_empty()
        }
        for list in [
            &mut self.user_intent,
            &mut self.technical_concepts,
            &mut self.errors_and_fixes,
            &mut self.problem_solving,
            &mut self.user_messages,
            &mut self.pending_tasks,
        ] {
            list.retain(|s| !blank(s));
        }
        for file in &mut self.files {
            if file.key_code.as_deref().is_some_and(blank) {
                file.key_code = None;
            }
        }
        self.files
            .retain(|f| !blank(&f.path) || !blank(&f.summary) || f.key_code.is_some());
        if self.current_work.as_deref().is_some_and(blank) {
            self.current_work = None;
        }
        if self.next_step.as_deref().is_some_and(blank) {
            self.next_step = None;
        }
    }

    fn is_empty(&self) -> bool {
        self.user_intent.is_empty()
            && self.technical_concepts.is_empty()
            && self.files.is_empty()
            && self.errors_and_fixes.is_empty()
            && self.problem_solving.is_empty()
            && self.user_messages.is_empty()
            && self.pending_tasks.is_empty()
            && self.current_work.is_none()
            && self.next_step.is_none()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileActivity {
    #[serde(default, deserialize_with = "lenient_string")]
    pub path: String,
    #[serde(default, deserialize_with = "lenient_string")]
    pub summary: String,
    #[serde(default, deserialize_with = "lenient_string_opt")]
    pub key_code: Option<String>,
}

fn stringify_lenient(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(k, v)| format!("{k}: {}", stringify_lenient(v)))
            .collect::<Vec<_>>()
            .join("; "),
        serde_json::Value::Array(items) => items
            .iter()
            .map(stringify_lenient)
            .collect::<Vec<_>>()
            .join("; "),
        other => other.to_string(),
    }
}

fn lenient_string_list<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Array(items) => items.iter().map(stringify_lenient).collect(),
        serde_json::Value::Null => Vec::new(),
        other => vec![stringify_lenient(&other)],
    })
}

fn lenient_string<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(stringify_lenient(&value))
}

fn lenient_string_opt<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(match value {
        serde_json::Value::Null => None,
        other => Some(stringify_lenient(&other)),
    })
}

/// `files` entries should be objects, but a model that over-applies the
/// "plain strings" rule may emit them as strings; render those as path-only
/// activities rather than discarding the whole summary.
fn lenient_file_list<'de, D>(deserializer: D) -> Result<Vec<FileActivity>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    let items = match value {
        serde_json::Value::Array(items) => items,
        serde_json::Value::Null => return Ok(Vec::new()),
        other => vec![other],
    };
    Ok(items
        .into_iter()
        .filter_map(|item| match item {
            serde_json::Value::Object(_) => serde_json::from_value(item).ok(),
            other => {
                let path = stringify_lenient(&other);
                (!path.trim().is_empty()).then_some(FileActivity {
                    path,
                    summary: String::new(),
                    key_code: None,
                })
            }
        })
        .collect())
}

/// Candidate JSON documents in the model's response, tried in order until one
/// parses: after each `</analysis>` terminator (last first) the
/// post-terminator ```json fences (last first) then a leading object, and
/// finally a leading object of the whole text.
///
/// Terminators before the last are retried because the summary JSON may
/// itself quote `</analysis>` (e.g. a session editing compaction prompts),
/// hiding the real terminator from a plain rfind. Such a candidate is
/// accepted only if it contains every later terminator occurrence - proof
/// they were quoted inside it - so a fenced example inside the scratchpad,
/// which precedes the real terminator, can never leak through.
///
/// Every candidate must sit directly at its marker and brace-balance to a
/// close. Anything looser erodes the lossless fallback: JSON merely quoted in
/// a prose response, or a fenced example inside the discarded scratchpad,
/// would silently replace the raw text. Extraction is brace-balanced rather
/// than fence-delimited because string values may legally contain ```, and an
/// unterminated object (output cut off mid-JSON) is not repaired - repair
/// would drop the late, continuation-critical sections that the raw-text
/// fallback preserves.
#[allow(clippy::string_slice)] // All markers are ASCII; indices are byte offsets of ASCII matches.
fn json_candidates(text: &str) -> Vec<&str> {
    const TERMINATOR: &str = "</analysis>";

    let mut cuts: Vec<usize> = text
        .match_indices(TERMINATOR)
        .map(|(idx, _)| idx + TERMINATOR.len())
        .collect();
    if cuts.is_empty() {
        cuts.push(0);
    }

    let mut candidates: Vec<&str> = Vec::new();
    for &cut in cuts.iter().rev() {
        let tail = &text[cut..];
        let later_terminators = tail.matches(TERMINATOR).count();
        candidates.extend(
            fenced_json_blocks(tail)
                .chain(leading_object(tail))
                .filter(|candidate| candidate.matches(TERMINATOR).count() == later_terminators),
        );
    }
    candidates.extend(leading_object(text));
    candidates.dedup();
    candidates
}

/// Every fence is tried, last first, because a string value may itself quote
/// a fenced JSON snippet, and such an embedded fence must not shadow the real
/// one.
#[allow(clippy::string_slice)] // The marker is ASCII; indices are byte offsets of ASCII matches.
fn fenced_json_blocks(text: &str) -> impl Iterator<Item = &str> {
    text.match_indices("```json")
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .filter_map(|(idx, marker)| leading_object(&text[idx + marker.len()..]))
}

#[allow(clippy::string_slice)] // Indices come from char_indices(); slicing is safe.
fn leading_object(text: &str) -> Option<&str> {
    let text = text.trim_start();
    if !text.starts_with('{') {
        return None;
    }

    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in text.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[..=idx]);
                }
            }
            _ => {}
        }
    }

    None
}

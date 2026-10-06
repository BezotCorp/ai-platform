/// Safely parse a JSON string that may contain doubly-encoded or malformed JSON.
/// This function first attempts to parse the input string as-is. If that fails,
/// it applies control character escaping and truncated JSON repair and tries again.
///
/// This approach preserves valid JSON like `{"key1": "value1",\n"key2": "value"}`
/// (which contains a literal \n but is perfectly valid JSON) while still fixing
/// broken JSON like `{"key1": "value1\n","key2": "value"}` (which contains an
/// unescaped newline character).
pub fn safely_parse_json(s: &str) -> Result<serde_json::Value, serde_json::Error> {
    // First, try parsing the string as-is
    match serde_json::from_str(s) {
        Ok(value) => Ok(value),
        Err(_) => {
            for candidate in [
                repair_truncated_json(s),
                json_escape_control_chars_in_string(s),
            ] {
                if let Ok(value) = serde_json::from_str(&candidate) {
                    return Ok(value);
                }
            }
            let repaired = repair_truncated_json(&json_escape_control_chars_in_string(s));
            serde_json::from_str(&repaired)
        }
    }
}

fn repair_truncated_json(s: &str) -> String {
    let mut repaired = String::with_capacity(s.len() + 8);
    let mut in_string = false;
    let mut escape_next = false;
    let mut closers = Vec::new();
    for c in s.chars() {
        repaired.push(c);
        if in_string {
            if escape_next {
                escape_next = false;
                continue;
            }
            match c {
                '\\' => escape_next = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => closers.push('}'),
            '[' => closers.push(']'),
            '}' | ']' if closers.last() == Some(&c) => {
                closers.pop();
            }
            _ => {}
        }
    }
    if in_string {
        if escape_next {
            repaired.push('\\');
        }
        repaired.push('"');
    }
    while let Some(closer) = closers.pop() {
        repaired.push(closer);
    }
    repaired
}

/// Helper to escape control characters in a string that is supposed to be a JSON document.
/// This function iterates through the input string `s` and replaces any literal
/// control characters (U+0000 to U+001F) with their JSON-escaped equivalents
/// (e.g., '\n' becomes "\\n", '\u0001' becomes "\\u0001").
///
/// It does NOT escape quotes (") or backslashes (\) because it assumes `s` is a
/// full JSON document, and these characters might be structural (e.g., object delimiters,
/// existing valid escape sequences). The goal is to fix common LLM errors where
/// control characters are emitted raw into what should be JSON string values,
/// making the overall JSON structure unparsable.
///
/// If the input string `s` has other JSON syntax errors (e.g., an unescaped quote
/// *within* a string value like `{"key": "string with " quote"}`), this function
/// will not fix them. It specifically targets unescaped control characters.
pub fn json_escape_control_chars_in_string(s: &str) -> String {
    let mut r = String::with_capacity(s.len()); // Pre-allocate for efficiency
    for c in s.chars() {
        match c {
            // ASCII Control characters (U+0000 to U+001F)
            '\u{0000}'..='\u{001F}' => {
                match c {
                    '\u{0008}' => r.push_str("\\b"), // Backspace
                    '\u{000C}' => r.push_str("\\f"), // Form feed
                    '\n' => r.push_str("\\n"),       // Line feed
                    '\r' => r.push_str("\\r"),       // Carriage return
                    '\t' => r.push_str("\\t"),       // Tab
                    // Other control characters (e.g., NUL, SOH, VT, etc.)
                    // that don't have a specific short escape sequence.
                    _ => {
                        r.push_str(&format!("\\u{:04x}", c as u32));
                    }
                }
            }
            // Other characters are passed through.
            // This includes quotes (") and backslashes (\). If these are part of the
            // JSON structure (e.g. {"key": "value"}) or part of an already correctly
            // escaped sequence within a string value (e.g. "string with \\\" quote"),
            // they are preserved as is. This function does not attempt to fix
            // malformed quote or backslash usage *within* string values if the LLM
            // generates them incorrectly (e.g. {"key": "unescaped " quote in string"}).
            _ => r.push(c),
        }
    }
    r
}

/// Detect whether a raw tool-arguments string looks truncated (the model hit
/// its output-token limit mid-JSON). Returns true when the string has
/// unbalanced or unclosed structural delimiters — whether the cut-off happened
/// mid-value (e.g. `{"path":"/a` with no closing quote) or after a nested
/// closer but before the outer object closed (e.g. `{"items":[1,2]` where the
/// outer `{` is still open).
pub fn looks_truncated(args: &str) -> bool {
    let trimmed = args.trim_end();
    if trimmed.is_empty() {
        return false;
    }
    let mut in_string = false;
    let mut escape_next = false;
    let mut depth = Vec::new();
    for c in trimmed.chars() {
        if in_string {
            if escape_next {
                escape_next = false;
            } else if c == '\\' {
                escape_next = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => depth.push('}'),
            '[' => depth.push(']'),
            '}' | ']' => {
                if depth.last() == Some(&c) {
                    depth.pop();
                } else {
                    return true;
                }
            }
            _ => {}
        }
    }
    in_string || escape_next || !depth.is_empty()
}

/// Build an actionable error message for tool arguments that could not be
/// parsed. `args` is the raw, accumulated arguments string from the provider.
///
/// The message distinguishes truncation (likely from the output token limit)
/// from other malformation, and includes a snippet of where parsing broke.
pub fn truncation_error_message(args: &str) -> Option<String> {
    if args.is_empty() {
        return None;
    }
    if serde_json::from_str::<serde_json::Value>(args).is_ok() {
        return None;
    }
    let trimmed = args.trim_end();
    let is_truncated = looks_truncated(trimmed);
    let snippet = {
        let len = trimmed.chars().count();
        if len > 80 {
            let s: String = trimmed
                .chars()
                .rev()
                .take(80)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            format!("…{s}")
        } else {
            trimmed.to_string()
        }
    };
    let guidance = if is_truncated {
        "The model's response was truncated — it hit the output token limit while generating this tool call. \
         Try increasing max_tokens for this provider or breaking the task into smaller steps."
    } else {
        "The model produced malformed tool arguments. Try resending your message or breaking the task into smaller steps."
    };
    Some(format!(
        "{guidance}\nReceived {} characters; cut off at: {snippet}",
        trimmed.chars().count()
    ))
}

fn strip_json_wrapper(args: &str) -> Option<&str> {
    let trimmed = args.trim();
    if let Some(rest) = trimmed.strip_prefix("```") {
        let body = rest.strip_suffix("```")?.trim_start();
        let body = body
            .strip_prefix("json")
            .or_else(|| body.strip_prefix("JSON"))
            .unwrap_or(body);
        return Some(body.trim());
    }
    if trimmed.starts_with('<') && !trimmed.starts_with("</") {
        let open_end = trimmed.find('>')?;
        let tag_name = trimmed.get(1..open_end)?.split_whitespace().next()?;
        if tag_name.is_empty()
            || !tag_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return None;
        }
        let closing = format!("</{tag_name}>");
        let body = trimmed
            .get(open_end + 1..)?
            .trim_end()
            .strip_suffix(&closing)?;
        return Some(body.trim());
    }
    None
}

fn unwrap_double_encoded_object(value: serde_json::Value) -> serde_json::Value {
    let serde_json::Value::String(s) = &value else {
        return value;
    };
    let mut current = s.trim().to_string();
    for _ in 0..3 {
        match serde_json::from_str::<serde_json::Value>(&current) {
            Ok(inner @ serde_json::Value::Object(_)) => return inner,
            Ok(serde_json::Value::String(inner)) => current = inner.trim().to_string(),
            _ => break,
        }
    }
    value
}

/// Parse tool-call arguments, returning `None` when the input looks truncated
/// so callers can surface an actionable error rather than invoking a tool with
/// incomplete arguments. Non-truncated malformation (e.g. unescaped control
/// characters some models emit) is still repaired via [`safely_parse_json`].
pub fn parse_tool_arguments(args: &str) -> Option<serde_json::Value> {
    if args.is_empty() {
        return Some(serde_json::Value::Object(serde_json::Map::new()));
    }
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(args) {
        return Some(unwrap_double_encoded_object(value));
    }
    if let Some(inner) = strip_json_wrapper(args) {
        if let Ok(value) = serde_json::from_str::<serde_json::Value>(inner) {
            return Some(unwrap_double_encoded_object(value));
        }
        if !looks_truncated(inner)
            && let Ok(value) = safely_parse_json(inner)
        {
            return Some(unwrap_double_encoded_object(value));
        }
    }
    if !looks_truncated(args)
        && let Ok(value) = safely_parse_json(args)
    {
        return Some(unwrap_double_encoded_object(value));
    }
    None
}

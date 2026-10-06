use tokio_util::sync::CancellationToken;
use unicode_normalization::UnicodeNormalization;
/// Encode bytes as a lowercase hexadecimal string.
///
/// This avoids relying on digest output types implementing `LowerHex`, which
/// changed in sha2 0.11.
pub fn bytes_to_hex(bytes: impl AsRef<[u8]>) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let bytes = bytes.as_ref();
    let mut output = String::with_capacity(bytes.len() * 2);

    for &byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }

    output
}

/// Check if a character is in the Unicode Tags Block range (U+E0000-U+E007F)
/// These characters are invisible and can be used for steganographic attacks
fn is_in_unicode_tag_range(c: char) -> bool {
    matches!(c, '\u{E0000}'..='\u{E007F}')
}

pub fn contains_unicode_tags(text: &str) -> bool {
    text.chars().any(is_in_unicode_tag_range)
}

pub fn strip_unicode_tags(text: &str) -> String {
    text.chars()
        .filter(|&c| !is_in_unicode_tag_range(c))
        .collect()
}

/// Sanitize Unicode Tags Block characters from text
pub fn sanitize_unicode_tags(text: &str) -> String {
    let normalized: String = text.nfc().collect();
    strip_unicode_tags(&normalized)
}

/// Safely truncate a string at character boundaries, not byte boundaries
///
/// This function ensures that multi-byte UTF-8 characters (like Japanese, emoji, etc.)
/// are not split in the middle, which would cause a panic.
///
/// # Arguments
/// * `s` - The string to truncate
/// * `max_chars` - Maximum number of characters to keep
///
/// # Returns
/// A truncated string with "..." appended if truncation occurred
pub fn safe_truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_chars.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}

pub fn is_token_cancelled(cancellation_token: &Option<CancellationToken>) -> bool {
    cancellation_token
        .as_ref()
        .is_some_and(|t| t.is_cancelled())
}

pub fn split_command_args(input: &str) -> anyhow::Result<Vec<String>> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_double_quote = false;
    let mut in_single_quote = false;

    for c in input.chars() {
        match c {
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            '\'' if !in_double_quote && (in_single_quote || current.is_empty()) => {
                in_single_quote = !in_single_quote
            }
            c if c.is_whitespace() && !in_double_quote && !in_single_quote => {
                if !current.is_empty() {
                    parts.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(c),
        }
    }

    if in_double_quote || in_single_quote {
        return Err(anyhow::anyhow!("Unmatched quote in command"));
    }

    if !current.is_empty() {
        parts.push(current);
    }

    Ok(parts)
}

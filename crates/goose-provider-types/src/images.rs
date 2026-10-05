use std::{borrow::Cow, fs, io::Read as _, path::Path};

use base64::Engine as _;
use rmcp::model::ImageContent;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::errors::ProviderError;

const MAX_IMAGE_BYTES: u64 = 20 * 1024 * 1024;

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub enum ImageFormat {
    OpenAi,
    Anthropic,
}

/// Convert an image content into an image json based on format
pub fn convert_image(image: &ImageContent, image_format: &ImageFormat) -> Value {
    match image_format {
        ImageFormat::OpenAi => json!({
            "type": "image_url",
            "image_url": {
                "url": format!("data:{};base64,{}", image.mime_type, image.data)
            }
        }),
        ImageFormat::Anthropic => json!({
            "type": "image",
            "source": {
                "type": "base64",
                "media_type": image.mime_type,
                "data": image.data,
            }
        }),
    }
}

pub fn detect_image_path(text: &str) -> Option<Cow<'_, str>> {
    const EXTENSIONS: [&str; 3] = [".png", ".jpg", ".jpeg"];
    const MAX_PATH_LEN: usize = 4096;

    let mut best: Option<(usize, Cow<'_, str>)> = None;
    let mut from = 0;
    while from < text.len() {
        let Some(end) = EXTENSIONS
            .iter()
            .filter_map(|ext| find_ascii_ci(text, ext, from).map(|i| i + ext.len()))
            .min()
        else {
            break;
        };

        let terminator = text.get(end..).and_then(|rest| rest.chars().next());
        let terminated = terminator.is_none_or(is_path_terminator);

        if terminated {
            let mut floor = end.saturating_sub(MAX_PATH_LEN);
            while floor < end && !text.is_char_boundary(floor) {
                floor += 1;
            }
            if let Some(window) = text.get(floor..end) {
                for (rel, _) in window.match_indices('/') {
                    let start = floor + rel;
                    let preceded_by_boundary = text
                        .get(..start)
                        .and_then(|prefix| prefix.chars().next_back())
                        .is_none_or(is_path_leading_boundary);
                    if !preceded_by_boundary {
                        continue;
                    }
                    let Some(candidate) = text.get(start..end) else {
                        continue;
                    };
                    if let Some(candidate_path) = image_path_candidate(candidate) {
                        // Keep the first referenced path, but allow a longer
                        // match anchored at the same start to extend it (a
                        // whitespace-terminated extension may be a prefix of a
                        // spaced filename ending in a later extension).
                        match best {
                            Some((best_start, _)) if start == best_start => {
                                best = Some((start, candidate_path));
                            }
                            None => best = Some((start, candidate_path)),
                            Some(_) => {}
                        }
                        break;
                    }
                }
            }
        }
        from = end;
    }
    best.map(|(_, candidate)| candidate)
}

fn clean_path(path: &str) -> Cow<'_, str> {
    if !path.contains('\\') {
        return Cow::Borrowed(path);
    }

    let mut cleaned = String::with_capacity(path.len());
    let mut chars = path.chars().peekable();
    let mut changed = false;

    while let Some(c) = chars.next() {
        if c == '\\'
            && let Some(&next) = chars.peek()
            && !next.is_alphanumeric()
        {
            cleaned.push(next);
            chars.next();
            changed = true;
            continue;
        }
        cleaned.push(c);
    }

    if changed {
        Cow::Owned(cleaned)
    } else {
        Cow::Borrowed(path)
    }
}

fn image_path_candidate(candidate: &str) -> Option<Cow<'_, str>> {
    if is_existing_image_path(candidate) {
        return Some(Cow::Borrowed(candidate));
    }

    let cleaned = clean_path(candidate);
    if cleaned.as_ref() != candidate && is_existing_image_path(cleaned.as_ref()) {
        return Some(cleaned);
    }

    None
}

fn is_existing_image_path(candidate: &str) -> bool {
    let path = Path::new(candidate);
    path.is_absolute() && path.is_file() && is_image_file(path)
}

fn is_path_leading_boundary(c: char) -> bool {
    c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '\u{00AB}' | '\u{00BB}' | '\u{2018}'
                ..='\u{201F}' | '\u{2039}' | '\u{203A}'
        )
}

fn is_path_terminator(c: char) -> bool {
    c == '/'
        || c.is_whitespace()
        || matches!(
            c,
            '"' | '\'' | '\u{00AB}' | '\u{00BB}' | '\u{2013}'
                ..='\u{201F}' | '\u{2026}' | '\u{2039}' | '\u{203A}'
        )
        || ('\u{2300}'..='\u{23FF}').contains(&c)
        || ('\u{2600}'..='\u{27BF}').contains(&c)
        || ('\u{2B00}'..='\u{2BFF}').contains(&c)
        || ('\u{1F1E6}'..='\u{1F1FF}').contains(&c)
        || ('\u{1F300}'..='\u{1FAFF}').contains(&c)
}

/// Case-insensitive ASCII substring search returning a byte index into
/// `haystack` (no allocation, so the index stays valid for slicing).
fn find_ascii_ci(haystack: &str, needle: &str, from: usize) -> Option<usize> {
    let (hb, nb) = (haystack.as_bytes(), needle.as_bytes());
    if nb.is_empty() || hb.len() < nb.len() || from > hb.len() - nb.len() {
        return None;
    }
    (from..=hb.len() - nb.len()).find(|&i| {
        hb[i..i + nb.len()]
            .iter()
            .zip(nb)
            .all(|(a, b)| a.eq_ignore_ascii_case(b))
    })
}

/// Check if a file is actually an image by examining its magic bytes
fn is_image_file(path: &Path) -> bool {
    if let Ok(mut file) = fs::File::open(path) {
        let mut buffer = [0u8; 8]; // Large enough for most image magic numbers
        if file.read(&mut buffer).is_ok() {
            return has_image_magic(&buffer);
        }
    }
    false
}

fn has_image_magic(bytes: &[u8]) -> bool {
    matches!(
        bytes.get(..4),
        Some([0x89, 0x50, 0x4E, 0x47])
            | Some([0xFF, 0xD8, 0xFF, _])
            | Some([0x47, 0x49, 0x46, 0x38])
    )
}

fn read_bounded(reader: impl std::io::Read, max_bytes: u64) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(max_bytes + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// Convert a local image file to base64 encoded ImageContent
pub fn load_image_file(path: &str) -> Result<ImageContent, ProviderError> {
    let path = Path::new(path);

    let mime_type = match path.extension().and_then(|e| e.to_str()) {
        Some(ext) => match ext.to_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            _ => {
                return Err(ProviderError::RequestFailed(
                    "Unsupported image format".to_string(),
                ));
            }
        },
        None => {
            return Err(ProviderError::RequestFailed(
                "Unknown image format".to_string(),
            ));
        }
    };

    let file = fs::File::open(path)
        .map_err(|e| ProviderError::RequestFailed(format!("Failed to read image file: {e}")))?;
    let file_size = file
        .metadata()
        .map_err(|e| ProviderError::RequestFailed(format!("Failed to read image file: {e}")))?
        .len();
    if file_size > MAX_IMAGE_BYTES {
        return Err(ProviderError::RequestFailed(format!(
            "Image file exceeds the {} MiB limit",
            MAX_IMAGE_BYTES / (1024 * 1024)
        )));
    }

    let bytes = read_bounded(file, MAX_IMAGE_BYTES)
        .map_err(|e| ProviderError::RequestFailed(format!("Failed to read image file: {e}")))?;
    if bytes.len() as u64 > MAX_IMAGE_BYTES {
        return Err(ProviderError::RequestFailed(format!(
            "Image file exceeds the {} MiB limit",
            MAX_IMAGE_BYTES / (1024 * 1024)
        )));
    }
    if !has_image_magic(&bytes) {
        return Err(ProviderError::RequestFailed(
            "File is not a valid image".to_string(),
        ));
    }

    let data = base64::prelude::BASE64_STANDARD.encode(&bytes);

    Ok(ImageContent::new(data, mime_type))
}

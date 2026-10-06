use crate::utils::sanitize_unicode_tags;
use base64::Engine;
use rmcp::model::ResourceContents;
pub fn extract_text_from_resource(resource: &ResourceContents) -> String {
    match resource {
        ResourceContents::TextResourceContents { text, .. } => sanitize_unicode_tags(text),
        ResourceContents::BlobResourceContents {
            blob, mime_type, ..
        } => match base64::engine::general_purpose::STANDARD.decode(blob) {
            Ok(bytes) => {
                let byte_len = bytes.len();
                match String::from_utf8(bytes) {
                    Ok(text) => sanitize_unicode_tags(&text),
                    Err(_) => {
                        let mime = mime_type
                            .as_ref()
                            .map(|m| m.as_str())
                            .unwrap_or("application/octet-stream");
                        format!("[Binary content ({}) - {} bytes]", mime, byte_len)
                    }
                }
            }
            Err(_) => sanitize_unicode_tags(blob),
        },
        _ => String::new(),
    }
}

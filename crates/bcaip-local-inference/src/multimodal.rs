use base64::prelude::*;
use bcaip_provider_types::conversations::{Message, MessageContent};
use bcaip_provider_types::errors::ProviderError;
use serde_json::Value;
#[derive(Debug)]
pub struct ExtractedImage {
    pub bytes: Vec<u8>,
}

#[derive(Debug)]
pub struct MultimodalMessages {
    pub messages_json: String,
    pub images: Vec<ExtractedImage>,
}

/// Walk the OpenAI-format messages JSON array. For each content part with
/// `type: "image_url"`, decode the base64 data URL, store the raw bytes,
/// and replace the part with `{"type": "text", "text": "<marker>"}`.
///
/// Returns the modified JSON string and the extracted images in order.
pub fn extract_images_from_messages_json(
    messages_json: &str,
    marker: &str,
) -> Result<MultimodalMessages, ProviderError> {
    let mut messages: Vec<Value> = serde_json::from_str(messages_json).map_err(|e| {
        ProviderError::ExecutionError(format!("Failed to parse messages JSON: {e}"))
    })?;

    let mut images = Vec::new();

    for msg in messages.iter_mut() {
        let Some(content) = msg.get_mut("content").and_then(|c| c.as_array_mut()) else {
            continue;
        };

        for part in content.iter_mut() {
            if part.get("type").and_then(|t| t.as_str()) != Some("image_url") {
                continue;
            }

            let url = part
                .get("image_url")
                .and_then(|obj| obj.get("url"))
                .and_then(|u| u.as_str())
                .unwrap_or_default();

            if url.starts_with("http://") || url.starts_with("https://") {
                return Err(ProviderError::ExecutionError(
                    "Remote image URLs are not supported with local inference. \
                     Please attach the image directly."
                        .to_string(),
                ));
            }

            let base64_data = url.split_once(',').map_or(url, |(_, data)| data);

            let bytes = BASE64_STANDARD.decode(base64_data).map_err(|e| {
                ProviderError::ExecutionError(format!("Failed to decode base64 image: {e}"))
            })?;

            images.push(ExtractedImage { bytes });

            *part = serde_json::json!({
                "type": "text",
                "text": marker,
            });
        }
    }

    let messages_json = serde_json::to_string(&messages)
        .map_err(|e| ProviderError::ExecutionError(format!("Failed to serialize messages: {e}")))?;

    Ok(MultimodalMessages {
        messages_json,
        images,
    })
}

/// Scan messages for `MessageContent::Image` entries. Return the extracted image
/// bytes and a new message list with images replaced by text marker placeholders.
pub fn extract_images_from_messages(
    messages: &[Message],
    marker: &str,
) -> (Vec<ExtractedImage>, Vec<Message>) {
    let mut images = Vec::new();
    let mut new_messages = Vec::with_capacity(messages.len());

    for msg in messages {
        let mut new_content = Vec::with_capacity(msg.content.len());
        for content in &msg.content {
            match content {
                MessageContent::Image(img) => {
                    if let Ok(bytes) = BASE64_STANDARD.decode(&img.data) {
                        images.push(ExtractedImage { bytes });
                        new_content.push(MessageContent::text(marker));
                    } else {
                        new_content.push(MessageContent::text(
                            "[Image attached — failed to decode image data]",
                        ));
                    }
                }
                other => new_content.push(other.clone()),
            }
        }
        new_messages.push(Message {
            role: msg.role.clone(),
            content: new_content,
            ..msg.clone()
        });
    }

    (images, new_messages)
}

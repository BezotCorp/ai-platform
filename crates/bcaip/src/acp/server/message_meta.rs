use agent_client_protocol::schema::v1::{ContentBlock, ContentChunk, MessageId, Meta};
use bcaip_provider_types::conversations::{Message, MessageContent};
use serde::Serialize;
const OUTPUT_TOKEN_LIMIT_TEXT: &str =
    "Response stopped because the model reached its output-token limit.";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct BcaipMessageMeta<'a> {
    created: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    message_id: Option<&'a str>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    steer: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    output_token_limit_reached: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    fallback_content: bool,
}

fn bcaip_message_meta(
    message: &Message,
    steer: bool,
) -> serde_json::Map<String, serde_json::Value> {
    let message_meta = BcaipMessageMeta {
        created: message.created,
        message_id: message.id.as_deref(),
        steer,
        output_token_limit_reached: message.metadata.output_token_limit_reached,
        fallback_content: has_output_token_limit_fallback_content(message),
    };

    match serde_json::to_value(message_meta) {
        Ok(serde_json::Value::Object(meta)) => meta,
        _ => serde_json::Map::new(),
    }
}

fn extend_message_meta(meta: &mut Meta, message: &Message, steer: bool) {
    let message_bcaip = bcaip_message_meta(message, steer);
    let bcaip_value = meta
        .entry("bcaip".to_string())
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));

    if let serde_json::Value::Object(bcaip) = bcaip_value {
        bcaip.extend(message_bcaip);
    } else {
        *bcaip_value = serde_json::Value::Object(message_bcaip);
    }
}

fn message_meta_with_steer(message: &Message, steer: bool) -> Meta {
    let mut meta = Meta::new();
    extend_message_meta(&mut meta, message, steer);
    meta
}

fn message_meta(message: &Message) -> Meta {
    message_meta_with_steer(message, message.metadata.steer)
}

pub(crate) fn message_meta_without_steer(message: &Message) -> Meta {
    message_meta_with_steer(message, false)
}

pub(crate) fn merge_message_meta(mut meta: Meta, message: &Message) -> Meta {
    extend_message_meta(&mut meta, message, message.metadata.steer);
    meta
}

pub(crate) fn content_chunk_for_message(message: &Message, content: ContentBlock) -> ContentChunk {
    let mut chunk = ContentChunk::new(content).meta(message_meta(message));
    if let Some(message_id) = message.id.as_deref() {
        chunk = chunk.message_id(MessageId::new(message_id));
    }
    chunk
}

pub(crate) fn populate_output_token_limit_content(message: &mut Message) {
    if message.role != rmcp::model::Role::Assistant
        || !message.content.is_empty()
        || !message.metadata.output_token_limit_reached
    {
        return;
    }

    message
        .content
        .push(MessageContent::text(OUTPUT_TOKEN_LIMIT_TEXT));
}

fn has_output_token_limit_fallback_content(message: &Message) -> bool {
    message.role == rmcp::model::Role::Assistant
        && message.metadata.output_token_limit_reached
        && matches!(
            message.content.as_slice(),
            [MessageContent::Text(text)] if text.text == OUTPUT_TOKEN_LIMIT_TEXT
        )
}

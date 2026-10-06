use crate::session::session_manager::Session;
use anyhow::Result;
use bcaip_provider_types::conversations::Message;
use rmcp::model::Role;
use sqlx::{AssertSqlSafe, Pool, Sqlite};
use std::collections::HashMap;
const LAST_MESSAGE_SNIPPET_MAX_CHARS: usize = 128;
const RECENT_MESSAGE_SNIPPET_SCAN_LIMIT: usize = 8;

#[derive(Debug, sqlx::FromRow)]
struct RecentMessageRow {
    row_id: i64,
    session_id: String,
    role: String,
    content_json: String,
    created_timestamp: i64,
    metadata_json: Option<String>,
    message_id: Option<String>,
}

pub(crate) async fn hydrate_last_message_snippets(
    pool: &Pool<Sqlite>,
    sessions: &mut [Session],
) -> Result<()> {
    if sessions.is_empty() {
        return Ok(());
    }

    let session_ids = sessions
        .iter()
        .map(|session| session.id.clone())
        .collect::<Vec<_>>();
    let mut snippets = HashMap::with_capacity(session_ids.len());

    let rows = recent_message_rows(pool, &session_ids).await?;

    for row in rows {
        if snippets.contains_key(&row.session_id) {
            continue;
        }

        let session_id = row.session_id.clone();
        let Some(message) = message_from_recent_row(row)? else {
            continue;
        };
        if let Some(snippet) = message_snippet(&message, LAST_MESSAGE_SNIPPET_MAX_CHARS) {
            snippets.insert(session_id, snippet);
        }
    }

    for session in sessions {
        session.last_message_snippet = snippets.remove(&session.id);
    }

    Ok(())
}

async fn recent_message_rows(
    pool: &Pool<Sqlite>,
    session_ids: &[String],
) -> Result<Vec<RecentMessageRow>> {
    if session_ids.is_empty() {
        return Ok(Vec::new());
    }

    let branch = r#"
        SELECT row_id, session_id, role, content_json, created_timestamp, metadata_json, message_id
        FROM (
            SELECT id AS row_id, session_id, role, content_json, created_timestamp, metadata_json, message_id
            FROM messages
            WHERE session_id = ?
            ORDER BY created_timestamp DESC, id DESC
            LIMIT ?
        )
    "#;
    let sql = std::iter::repeat_n(branch, session_ids.len())
        .collect::<Vec<_>>()
        .join(" UNION ALL ");

    let mut query = sqlx::query_as::<_, RecentMessageRow>(AssertSqlSafe(sql));
    for session_id in session_ids {
        query = query
            .bind(session_id)
            .bind(RECENT_MESSAGE_SNIPPET_SCAN_LIMIT as i64);
    }

    let mut rows = query.fetch_all(pool).await?;
    rows.sort_by(|left, right| {
        left.session_id
            .cmp(&right.session_id)
            .then_with(|| right.created_timestamp.cmp(&left.created_timestamp))
            .then_with(|| right.row_id.cmp(&left.row_id))
    });
    Ok(rows)
}

fn message_from_recent_row(row: RecentMessageRow) -> Result<Option<Message>> {
    let role = match row.role.as_str() {
        "user" => Role::User,
        "assistant" => Role::Assistant,
        _ => return Ok(None),
    };

    let content = match serde_json::from_str(&row.content_json) {
        Ok(content) => content,
        Err(_) => return Ok(None),
    };
    let metadata = row
        .metadata_json
        .and_then(|json| serde_json::from_str(&json).ok())
        .unwrap_or_default();

    let mut message = Message::new(role, row.created_timestamp, content);
    message.metadata = metadata;
    if let Some(id) = row.message_id {
        message = message.with_id(id);
    }
    Ok(Some(message))
}

/// Build a bounded, single-line snippet from user-visible message text.
///
/// Tool-request, tool-response, thinking, image-only, and assistant-audience
/// blocks collapse to an empty string and return `None`. Internal whitespace
/// and newlines are collapsed to single spaces, and the result includes at most
/// `max_chars` characters of content; if truncated, a trailing `…` is appended
/// so it can be rendered verbatim by clients.
fn message_snippet(message: &Message, max_chars: usize) -> Option<String> {
    if !message.metadata.user_visible {
        return None;
    }

    let text = message
        .content
        .iter()
        .filter_map(|content| content.filter_for_audience(Role::User))
        .filter_map(|content| content.as_text().map(|text| text.to_string()))
        .collect::<Vec<_>>()
        .join("\n");
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if normalized.is_empty() {
        return None;
    }

    let mut chars = normalized.chars();
    let mut result: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        let end = result.trim_end().len();
        result.truncate(end);
        result.push('…');
    }
    Some(result)
}

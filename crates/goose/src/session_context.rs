use futures::{StreamExt, stream::BoxStream};
use reqwest::header::{HeaderName, HeaderValue};

pub const SESSION_ID_HEADER: &str = "agent-session-id";

pub const TOOL_CALL_REQUEST_ID_HEADER: &str = "agent-tool-call-request-id";
pub const WORKING_DIR_HEADER: &str = "agent-working-dir";

tokio::task_local! {
    pub static SESSION_ID: Option<String>;
}

pub async fn with_session_id<F>(session_id: Option<String>, f: F) -> F::Output
where
    F: std::future::Future,
{
    SESSION_ID.scope(session_id, f).await
}

pub fn with_session_id_stream<'a, T: Send + 'a>(
    session_id: Option<String>,
    stream: BoxStream<'a, T>,
) -> BoxStream<'a, T> {
    Box::pin(futures::stream::unfold(
        (stream, session_id),
        |(mut stream, session_id)| async move {
            with_session_id(session_id.clone(), stream.next())
                .await
                .map(|item| (item, (stream, session_id)))
        },
    ))
}

pub fn current_session_id() -> Option<String> {
    SESSION_ID.try_with(|id| id.clone()).ok().flatten()
}

pub fn session_id_request_builder() -> goose_providers::api_client::RequestBuilderDecorator {
    session_id_request_builder_with_header_name(HeaderName::from_static(SESSION_ID_HEADER))
}

pub(crate) fn session_id_request_builder_with_header_override(
    header_name_override: Option<&str>,
) -> Result<goose_providers::api_client::RequestBuilderDecorator, reqwest::header::InvalidHeaderName>
{
    let header_name = match header_name_override {
        Some(header_name) => HeaderName::from_bytes(header_name.as_bytes())?,
        None => HeaderName::from_static(SESSION_ID_HEADER),
    };

    Ok(session_id_request_builder_with_header_name(header_name))
}

fn session_id_request_builder_with_header_name(
    header_name: HeaderName,
) -> goose_providers::api_client::RequestBuilderDecorator {
    std::sync::Arc::new(move |request| {
        let (client, request) = request.build_split();
        let mut request = request?;
        let session_header = header_name.clone();
        request.headers_mut().remove(&session_header);

        if let Some(session_id) = current_session_id() {
            let value = HeaderValue::from_str(&session_id)?;
            request.headers_mut().insert(session_header, value);
        }

        Ok(reqwest::RequestBuilder::from_parts(client, request))
    })
}

/// Local OS user running goose, shared by the OTLP `user.name` resource
/// attribute and the `session.user` span attribute so the two never drift.
pub fn session_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("LOGNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

/// Hostname of the machine running goose, shared by the OTLP `host.name`
/// resource attribute and the `session.host` span attribute.
pub fn session_host() -> String {
    gethostname::gethostname().to_string_lossy().to_string()
}

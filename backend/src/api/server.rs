use std::{
    env,
    net::Ipv4Addr,
    sync::Arc,
};

use anyhow::{Context, Result, bail};

use axum::{
    Router,
    extract::{
        State,
        ws::WebSocketUpgrade,
    },
    http::{
        HeaderMap,
        StatusCode,
        header,
    },
    response::{
        IntoResponse,
        Response,
    },
    routing::get,
};

use serde_json::json;
use tokio::sync::Semaphore;

use crate::providers::ollama::client::Client;

use super::{
    server_state::ServerState,
    socket,
};

async fn upgrade(
    State(state): State<ServerState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());

    if origin != Some(state.origin.as_ref()) {
        return (
            StatusCode::FORBIDDEN,
            "Origine non autorisée",
        )
            .into_response();
    }

    ws.max_message_size(256 * 1024)
        .on_upgrade(move |socket| {
            socket::serve(
                socket,
                state.client,
                state.token,
                state.gpu,
            )
        })
}

pub(crate) async fn run() -> Result<()> {
    let token = env::var("AI_PLATFORM_TOKEN")
        .context(
            "AI_PLATFORM_TOKEN doit être fourni par le frontend"
        )?;

    if token.len() < 32 || token.len() > 512 {
        bail!("Jeton d'authentification invalide");
    }

    let origin = env::var("AI_PLATFORM_ORIGIN")
        .context(
            "AI_PLATFORM_ORIGIN doit être fourni par le frontend"
        )?;

    if origin.trim().is_empty()
        || origin == "*"
    {
        bail!("Origine frontend invalide");
    }

    let ollama_host = env::var("OLLAMA_HOST")
        .unwrap_or_else(|_| {
            "http://127.0.0.1:11434".into()
        });

    let client = Client::new(&ollama_host)?;

    let state = ServerState {
        client,
        token: Arc::from(token),
        origin: Arc::from(origin),
        gpu: Arc::new(Semaphore::new(1)),
    };

    let listener = tokio::net::TcpListener::bind((
        Ipv4Addr::LOCALHOST,
        0,
    ))
    .await?;

    let url = format!(
        "ws://{}/ws",
        listener.local_addr()?
    );

    let router = Router::new()
        .route("/ws", get(upgrade))
        .with_state(state);

    // Le processus frontend parent lit cette ligne.
    // Le jeton secret n'est jamais affiché.
    println!(
        "{}",
        json!({
            "type": "ready",
            "url": url,
        })
    );

    use std::io::Write;
    std::io::stdout().flush()?;

    axum::serve(listener, router).await?;

    Ok(())
}

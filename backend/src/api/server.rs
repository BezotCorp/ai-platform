use anyhow::{Context, Result, bail};
use axum::{
    Router,
    extract::{State, ws::WebSocketUpgrade},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use serde_json::json;
use std::{
    env,
    io::{self, Write},
    net::Ipv4Addr,
    sync::Arc,
};
use tokio::{
    fs,
    sync::{Mutex, Semaphore},
};

use crate::{
    agents::MemoryStore,
    api::{ServerState, socket},
    providers::Client,
};

const DEFAULT_OLLAMA_HOST: &str = "http://127.0.0.1:11434";

async fn upgrade(
    State(state): State<ServerState>,
    headers: HeaderMap,
    ws: WebSocketUpgrade,
) -> Response {
    let origin = headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok());
    if origin != Some(state.origin.as_ref()) {
        return (StatusCode::FORBIDDEN, "Origine non autorisée").into_response();
    }
    ws.max_message_size(256 * 1024).on_upgrade(move |socket| {
        socket::serve(
            socket,
            state.client,
            state.token,
            state.gpu,
            state.project_root,
            state.writes,
            state.approve_reads,
            state.memory,
        )
    })
}

pub(crate) async fn run() -> Result<()> {
    let token = env::var("AI_PLATFORM_TOKEN")
        .context("AI_PLATFORM_TOKEN doit être fourni par le frontend")?;
    if token.len() < 32 || token.len() > 512 {
        bail!("Jeton d'authentification invalide");
    }
    let origin = env::var("AI_PLATFORM_ORIGIN")
        .context("AI_PLATFORM_ORIGIN doit être fourni par le frontend")?;
    if origin.trim().is_empty() || origin == "*" {
        bail!("Origine frontend invalide");
    }
    let ollama_host = env::var("OLLAMA_HOST").unwrap_or_else(|_| DEFAULT_OLLAMA_HOST.into());
    let client = Client::new(&ollama_host)?;
    let project_root = env::var("AI_PLATFORM_PROJECT_ROOT")
        .context("AI_PLATFORM_PROJECT_ROOT doit être fourni par le frontend")?;
    let project_root = fs::canonicalize(&project_root).await?;
    if !fs::metadata(&project_root).await?.is_dir() {
        bail!("Le projet autorisé n'est pas un répertoire");
    }
    let approve_reads = match env::var("AI_PLATFORM_APPROVE_READS").as_deref() {
        Ok("1") => true,
        Ok("0") | Err(_) => false,
        _ => {
            bail!("AI_PLATFORM_APPROVE_READS doit valoir 0 ou 1")
        }
    };
    // Persistance facultative, explicitement configurée par le frontend.
    // Un chemin absent laisse le backend sans mémoire persistante.
    let memory = match env::var("AI_PLATFORM_MEMORY_DB") {
        Ok(path) if !path.trim().is_empty() => {
            Some(MemoryStore::open(path.into(), &project_root).await?)
        }
        Ok(_) => bail!("AI_PLATFORM_MEMORY_DB ne doit pas être vide"),
        Err(env::VarError::NotPresent) => None,
        Err(error) => return Err(error.into()),
    };
    let state = ServerState {
        client,
        token: Arc::from(token),
        origin: Arc::from(origin),
        gpu: Arc::new(Semaphore::new(1)),
        project_root: Arc::new(project_root),
        writes: Arc::new(Mutex::new(())),
        approve_reads,
        memory,
    };
    let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).await?;
    let url = format!("ws://{}/ws", listener.local_addr()?);
    let router = Router::new().route("/ws", get(upgrade)).with_state(state);
    // Le processus frontend parent lit cette ligne.
    // Le jeton secret n'est jamais affiché.
    println!(
        "{}",
        json!({
            "type": "ready",
            "url": url,
        })
    );
    io::stdout().flush()?;
    axum::serve(listener, router).await?;
    Ok(())
}

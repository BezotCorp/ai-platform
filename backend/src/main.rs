mod agents;
mod configurations;
mod event;
mod file_manager;
mod providers;
mod sessions;
mod sqlite;
mod tools;
mod websocket;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    websocket::run().await
}

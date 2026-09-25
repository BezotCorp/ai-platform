mod agents;
mod api;
mod file_manager;
mod providers;
mod sessions;
mod tools;
mod sqlite;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    api::run().await
}

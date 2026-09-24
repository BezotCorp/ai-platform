mod providers;
mod agents;
mod context;
mod memory;
mod tools;
mod sessions;
mod api;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    api::server::run().await
}

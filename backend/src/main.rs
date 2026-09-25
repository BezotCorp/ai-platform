mod agents;
mod api;
mod context;
mod memory;
mod providers;
mod sessions;
mod tools;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    api::run().await
}

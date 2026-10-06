use anyhow::Result;
use bcaip_provider_types::base::Provider;
use bcaip_provider_types::conversations::Message;
use bcaip_provider_types::model::ModelConfig;
use futures::StreamExt;
use goose_providers::declarative::EnvKeyResolver;

async fn complete(provider: &dyn Provider, model: ModelConfig) -> Result<()> {
    let system = "You are a knowledgable geography expert";
    let messages = [Message::user().with_text("what is the capital of France?")];
    let mut stream = provider.stream(&model, system, &messages, &[]).await?;

    while let Some((Some(msg), _)) = stream.next().await.transpose()? {
        print!("{}", msg.as_concat_text());
    }
    println!();

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let model = ModelConfig::new("deepseek-v4-flash");
    let provider = goose_providers::deepseek::create(None, EnvKeyResolver {})?;
    complete(provider.as_ref(), model).await?;

    Ok(())
}

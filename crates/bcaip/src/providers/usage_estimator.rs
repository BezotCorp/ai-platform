use crate::token_counter::create_token_counter;
use anyhow::Result;
use bcaip_provider_types::conversations::Message;
use bcaip_provider_types::conversations::ProviderUsage;
use rmcp::model::Tool;
/// Ensures that ProviderUsage has token counts, estimating them if necessary.
/// This provides a single place to handle the fallback logic for providers that don't return usage data.
pub async fn ensure_usage_tokens(
    provider_usage: &mut ProviderUsage,
    system_prompt: &str,
    request_messages: &[Message],
    response: &Message,
    tools: &[Tool],
) -> Result<()> {
    if provider_usage.usage.input_tokens.is_some() && provider_usage.usage.output_tokens.is_some() {
        return Ok(());
    }

    let token_counter = create_token_counter()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create token counter: {}", e))?;

    if provider_usage.usage.input_tokens.is_none() {
        let input_count = token_counter.count_chat_tokens(system_prompt, request_messages, tools);
        provider_usage.usage.input_tokens = Some(input_count as i32);
    }

    if provider_usage.usage.output_tokens.is_none() {
        let response_text = response
            .content
            .iter()
            .map(|c| format!("{}", c))
            .collect::<Vec<_>>()
            .join(" ");
        let output_count = token_counter.count_tokens(&response_text);
        provider_usage.usage.output_tokens = Some(output_count as i32);
    }

    if let (Some(input), Some(output)) = (
        provider_usage.usage.input_tokens,
        provider_usage.usage.output_tokens,
    ) {
        provider_usage.usage.total_tokens = Some(input + output);
    }

    Ok(())
}

use std::env;

use anyhow::{Result, bail};

pub(crate) fn limits() -> Result<(usize, usize)> {
    let context_tokens = env::var(
        "AI_PLATFORM_NUM_CTX"
    )
    .ok()
    .map(|value| value.parse::<usize>())
    .transpose()?
    .unwrap_or(4096);

    let output_tokens = env::var(
        "AI_PLATFORM_NUM_PREDICT"
    )
    .ok()
    .map(|value| value.parse::<usize>())
    .transpose()?
    .unwrap_or(768);

    if !(1024..=131072).contains(&context_tokens) {
        bail!("Capacité de contexte invalide");
    }

    if !(128..=8192).contains(&output_tokens) {
        bail!("Réserve de génération invalide");
    }

    if output_tokens
        .checked_add(384)
        .is_none_or(|minimum| minimum >= context_tokens)
    {
        bail!(
            "La génération laisse trop peu de place au contexte"
        );
    }

    Ok((context_tokens, output_tokens))
}

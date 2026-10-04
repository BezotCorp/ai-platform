use std::collections::HashMap;
use std::future::Future;

use crate::canonical::maybe_get_canonical_model;
use crate::errors::ProviderError;
use crate::model::DEFAULT_CONTEXT_LIMIT;

#[derive(Debug, Clone, Default)]
pub struct ContextLimitResolver {
    provider_name: String,
    configured_limits: HashMap<String, usize>,
}

impl ContextLimitResolver {
    pub fn new(provider_name: impl Into<String>) -> Self {
        Self {
            provider_name: provider_name.into(),
            configured_limits: HashMap::new(),
        }
    }

    pub fn with_configured_limits(
        mut self,
        configured_limits: impl IntoIterator<Item = (String, usize)>,
    ) -> Self {
        self.configured_limits = configured_limits
            .into_iter()
            .filter(|(_, context_limit)| *context_limit > 0)
            .collect();
        self
    }

    fn configured_limit(&self, model: &str) -> Option<usize> {
        self.configured_limits.get(model).copied().or_else(|| {
            let mut matches = self
                .configured_limits
                .iter()
                .filter(|(configured_model, _)| configured_model.eq_ignore_ascii_case(model));
            let (_, limit) = matches.next()?;
            matches.next().is_none().then_some(*limit)
        })
    }

    pub fn resolve_local(&self, model: &str, override_limit: Option<usize>) -> usize {
        override_limit
            .or_else(|| self.configured_limit(model))
            .or_else(|| {
                maybe_get_canonical_model(&self.provider_name, model)
                    .map(|canonical| canonical.limit.context)
            })
            .unwrap_or(DEFAULT_CONTEXT_LIMIT)
    }

    pub async fn resolve<F, Fut>(
        &self,
        model: &str,
        override_limit: Option<usize>,
        discover: F,
    ) -> usize
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Option<usize>, ProviderError>>,
    {
        if let Some(limit) = override_limit {
            return limit;
        }

        if let Some(limit) = self.configured_limit(model) {
            return limit;
        }

        match discover().await {
            Ok(Some(limit)) if limit > 0 => return limit,
            Ok(Some(_) | None) => {}
            Err(error) => tracing::warn!(
                provider = self.provider_name,
                model,
                %error,
                "Context-limit discovery failed; falling back"
            ),
        }

        maybe_get_canonical_model(&self.provider_name, model)
            .map(|canonical| canonical.limit.context)
            .unwrap_or(DEFAULT_CONTEXT_LIMIT)
    }
}

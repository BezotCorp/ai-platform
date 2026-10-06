use crate::config::Config;
use opentelemetry_otlp::WithExportConfig;
use std::{env, time::Duration};
/// OTLP settings read from the goose config file. They only apply where the standard
/// `OTEL_EXPORTER_OTLP_*` environment variables are unset, so the environment always wins.
#[derive(Debug, Default)]
pub(crate) struct OtlpConfigOverrides {
    endpoint: Option<String>,
    timeout: Option<Duration>,
}

impl OtlpConfigOverrides {
    pub(crate) fn from_config(config: &Config) -> Self {
        Self {
            endpoint: config
                .get_param::<String>("otel_exporter_otlp_endpoint")
                .ok(),
            timeout: config
                .get_param::<u64>("otel_exporter_otlp_timeout")
                .ok()
                .map(Duration::from_millis),
        }
    }

    pub(crate) fn has_endpoint(&self) -> bool {
        self.endpoint.is_some() && env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_err()
    }

    pub(crate) fn apply<B: WithExportConfig>(&self, mut builder: B, signal: &str) -> B {
        let signal_upper = signal.to_uppercase();
        let endpoint_set_in_env = env::var("OTEL_EXPORTER_OTLP_ENDPOINT").is_ok()
            || env::var(format!("OTEL_EXPORTER_OTLP_{signal_upper}_ENDPOINT")).is_ok();
        if let Some(endpoint) = self.endpoint.as_deref().filter(|_| !endpoint_set_in_env) {
            // A programmatic endpoint is used verbatim, whereas the base endpoint variable has the
            // signal path appended by the SDK.
            builder =
                builder.with_endpoint(format!("{}/v1/{signal}", endpoint.trim_end_matches('/')));
        }

        let timeout_set_in_env = env::var("OTEL_EXPORTER_OTLP_TIMEOUT").is_ok()
            || env::var(format!("OTEL_EXPORTER_OTLP_{signal_upper}_TIMEOUT")).is_ok();
        if let Some(timeout) = self.timeout.filter(|_| !timeout_set_in_env) {
            builder = builder.with_timeout(timeout);
        }
        builder
    }
}

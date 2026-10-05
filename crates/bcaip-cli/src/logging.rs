use anyhow::Result;
use goose::providers::utils::init_goose_request_log;
use std::sync::OnceLock;
// Used to ensure we only set up tracing once
static INIT: OnceLock<Result<()>> = OnceLock::new();

/// Sets up the logging infrastructure for the CLI.
/// Logs go to a JSON file only (no console output).
pub fn setup_logging(name: Option<&str>) -> &'static Result<()> {
    INIT.get_or_init(|| {
        use tracing_subscriber::util::SubscriberInitExt;
        init_goose_request_log()?;
        let config = goose::logging::LoggingConfig {
            component: "cli",
            name,
            extra_directives: &["bcaip_cli=info"],
            console: false,
            json: true,
        };
        let subscriber = goose::logging::build_logging_subscriber(&config)?;

        subscriber
            .try_init()
            .map_err(|e| anyhow::anyhow!("Failed to set global subscriber: {}", e))?;
        Ok(())
    })
}

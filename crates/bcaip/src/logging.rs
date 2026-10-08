use crate::config::paths::Paths;
use anyhow::{Context, Result};
use std::time::{Duration, SystemTime};
use std::{fs, path::PathBuf};
use tracing_appender::rolling::Rotation;
use tracing_subscriber::{
    EnvFilter, Layer, Registry, filter::LevelFilter, fmt, layer::SubscriberExt,
    util::SubscriberInitExt,
};

/// Configuration for the shared logging setup.
pub struct LoggingConfig<'a> {
    /// Component name used for the log directory (e.g. "cli").
    pub component: &'a str,
    /// Optional session/run name appended to the log filename.
    pub name: Option<&'a str>,
    /// Additional `EnvFilter` directives beyond the defaults (e.g. "bcaip_cli=info").
    /// Only applied when `RUST_LOG` is **not** set.
    pub extra_directives: &'a [&'a str],
    /// Whether to emit a pretty console layer to stderr in addition to the file layer.
    pub console: bool,
    /// Whether the file layer should use JSON formatting. When false, uses plain text
    /// with source file path included.
    pub json: bool,
}

/// Build the `EnvFilter`. If `RUST_LOG` is set, use it as-is. Otherwise build a
/// default filter and append the caller's extra directives.
fn build_env_filter(extra_directives: &[&str]) -> EnvFilter {
    if let Ok(filter) = EnvFilter::try_from_default_env() {
        return filter;
    }

    let mut filter = EnvFilter::new("")
        .add_directive("mcp_client=info".parse().unwrap())
        .add_directive("bcaip=info".parse().unwrap())
        .add_directive(LevelFilter::WARN.into());

    for directive in extra_directives {
        if let Ok(d) = directive.parse() {
            filter = filter.add_directive(d);
        }
    }
    filter
}

/// Set up file-based (and optionally console) tracing for a BCAIP component.
///
/// Call `try_init` on the returned subscriber; callers are responsible for the
/// `Once` guard or direct init as appropriate for their use case.
pub fn build_logging_subscriber(
    config: &LoggingConfig<'_>,
) -> Result<impl SubscriberInitExt + Send + Sync + 'static> {
    let log_dir = prepare_log_directory(config.component, true)?;
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();
    let log_filename = match config.name {
        Some(n) => format!("{}-{}.log", timestamp, n),
        None => format!("{}.log", timestamp),
    };

    let file_appender =
        tracing_appender::rolling::RollingFileAppender::new(Rotation::NEVER, log_dir, log_filename);

    let env_filter = build_env_filter(config.extra_directives);

    let mut layers: Vec<Box<dyn Layer<Registry> + Send + Sync>> = if config.json {
        let file_layer = fmt::layer()
            .with_target(true)
            .with_level(true)
            .with_writer(file_appender)
            .with_ansi(false)
            .json();
        vec![file_layer.with_filter(env_filter.clone()).boxed()]
    } else {
        let file_layer = fmt::layer()
            .with_target(true)
            .with_level(true)
            .with_writer(file_appender)
            .with_ansi(false)
            .with_file(true);
        vec![file_layer.with_filter(env_filter.clone()).boxed()]
    };

    if config.console {
        let console_layer = fmt::layer()
            .with_writer(std::io::stderr)
            .with_target(true)
            .with_level(true)
            .with_file(true)
            .with_ansi(false)
            .with_line_number(true)
            .pretty();
        layers.push(console_layer.with_filter(env_filter).boxed());
    }

    #[cfg(feature = "otel")]
    layers.extend(crate::otel::otlp::init_otlp_layers(
        crate::config::Config::global(),
    ));

    if let Some(langfuse) = crate::tracing::langfuse_layer::create_langfuse_observer() {
        layers.push(langfuse.with_filter(LevelFilter::DEBUG).boxed());
    }

    Ok(Registry::default().with(layers))
}

/// Returns the directory where log files should be stored for a specific component.
/// Creates the directory structure if it doesn't exist.
///
/// # Arguments
///
/// * `component` - The component name (e.g., "cli", "server", "debug", "llm")
/// * `use_date_subdir` - Whether to create a date-based subdirectory
pub fn prepare_log_directory(component: &str, use_date_subdir: bool) -> Result<PathBuf> {
    let base_log_dir = Paths::in_state_dir("logs");

    let _ = cleanup_old_logs(component);

    let component_dir = base_log_dir.join(component);

    let log_dir = if use_date_subdir {
        component_dir.join(chrono::Local::now().format("%Y-%m-%d").to_string())
    } else {
        component_dir
    };

    fs::create_dir_all(&log_dir)
        .with_context(|| format!("Failed to create log directory: {:?}", log_dir))?;

    Ok(log_dir)
}

pub fn cleanup_old_logs(component: &str) -> Result<()> {
    let base_log_dir = Paths::in_state_dir("logs");
    let component_dir = base_log_dir.join(component);

    if !component_dir.exists() {
        return Ok(());
    }

    let two_weeks = SystemTime::now() - Duration::from_secs(14 * 24 * 60 * 60);
    let entries = fs::read_dir(&component_dir)?;

    for entry in entries.flatten() {
        let path = entry.path();

        if let Ok(metadata) = entry.metadata() {
            if let Ok(modified) = metadata.modified() {
                if modified < two_weeks && path.is_dir() {
                    let _ = fs::remove_dir_all(&path);
                }
            }
        }
    }

    Ok(())
}

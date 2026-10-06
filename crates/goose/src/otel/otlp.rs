use opentelemetry::trace::TracerProvider;
use opentelemetry::{KeyValue, global};
use opentelemetry_appender_tracing::layer::{OpenTelemetryTracingBridge, TracingSpanAttributes};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::{SdkLogger, SdkLoggerProvider};
use opentelemetry_sdk::metrics::{SdkMeterProvider, Temporality};
use opentelemetry_sdk::propagation::TraceContextPropagator;
use opentelemetry_sdk::resource::{EnvResourceDetector, TelemetryResourceDetector};
use opentelemetry_sdk::trace::SdkTracerProvider;
use std::env;
use std::sync::{Arc, Mutex};
use tracing::{Level, Metadata};
use tracing_opentelemetry::{MetricsLayer, OpenTelemetryLayer};
use tracing_subscriber::Layer as _;
use tracing_subscriber::filter::{EnvFilter, FilterExt, FilterFn};

use crate::otel::otlp_config_overrides::OtlpConfigOverrides;
pub type OtlpTracingLayer =
    OpenTelemetryLayer<tracing_subscriber::Registry, opentelemetry_sdk::trace::Tracer>;
pub type OtlpMetricsLayer = MetricsLayer<tracing_subscriber::Registry, SdkMeterProvider>;
pub type OtlpLogsLayer = OpenTelemetryTracingBridge<SdkLoggerProvider, SdkLogger>;
pub type OtlpResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

static TRACER_PROVIDER: Mutex<Option<SdkTracerProvider>> = Mutex::new(None);
static METER_PROVIDER: Mutex<Option<SdkMeterProvider>> = Mutex::new(None);
static LOGGER_PROVIDER: Mutex<Option<SdkLoggerProvider>> = Mutex::new(None);

static GRPC_PROTOCOL_WARNING_EMITTED: std::sync::Once = std::sync::Once::new();

/// One-shot stderr warning when `OTEL_EXPORTER_OTLP_PROTOCOL=grpc` is set
/// in an environment where goose was built without the `grpc-tonic`
/// transport feature. Using `tracing::warn!` here would race the OTel
/// subscriber that is being initialized; eprintln keeps it visible
/// regardless of subscriber state.
fn warn_grpc_protocol_skipped_once() {
    GRPC_PROTOCOL_WARNING_EMITTED.call_once(|| {
        eprintln!(
            "goose otel: OTEL_EXPORTER_OTLP_PROTOCOL is set to a gRPC \
             variant, but this goose build only includes the HTTP \
             transport (http-proto). OTLP signals are disabled to \
             avoid background-thread panics. Set \
             OTEL_EXPORTER_OTLP_PROTOCOL=http/protobuf and point \
             OTEL_EXPORTER_OTLP_ENDPOINT at an http://…:4318 collector \
             to re-enable export."
        );
    });
}

/// Dedicated single-thread Tokio runtime for OTLP HTTP export.
///
/// `BatchSpanProcessor`, `BatchLogProcessor`, and `PeriodicReader` all
/// spawn raw `std::thread`s with no Tokio reactor. The async reqwest HTTP
/// client calls `tokio::time::sleep` during export, which panics without a
/// reactor. We drive every OTLP export call through this runtime so the
/// reqwest futures always have a live Tokio handle.
static OTEL_RT: Mutex<Option<Arc<tokio::runtime::Runtime>>> = Mutex::new(None);

fn get_or_create_otel_rt() -> OtlpResult<Arc<tokio::runtime::Runtime>> {
    let mut guard = OTEL_RT.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(rt) = guard.as_ref() {
        return Ok(Arc::clone(rt));
    }
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let rt = Arc::new(rt);
    *guard = Some(Arc::clone(&rt));
    Ok(rt)
}

/// Wraps an OTLP `SpanExporter` so that each `export` call is driven inside
/// a dedicated Tokio runtime. Required because `BatchSpanProcessor` runs in
/// a raw `std::thread` with no Tokio reactor.
#[derive(Debug)]
struct TokioSpanExporter {
    inner: opentelemetry_otlp::SpanExporter,
    rt: Arc<tokio::runtime::Runtime>,
}

impl opentelemetry_sdk::trace::SpanExporter for TokioSpanExporter {
    async fn export(
        &self,
        batch: Vec<opentelemetry_sdk::trace::SpanData>,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        if tokio::runtime::Handle::try_current().is_ok() {
            self.inner.export(batch).await
        } else {
            self.rt.block_on(self.inner.export(batch))
        }
    }

    fn shutdown_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        self.inner.force_flush()
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

/// Wraps an OTLP `LogExporter` so that each export runs inside a dedicated
/// Tokio runtime for the same reason as `TokioSpanExporter`.
#[derive(Debug)]
struct TokioLogExporter {
    inner: opentelemetry_otlp::LogExporter,
    rt: Arc<tokio::runtime::Runtime>,
}

impl opentelemetry_sdk::logs::LogExporter for TokioLogExporter {
    async fn export(
        &self,
        batch: opentelemetry_sdk::logs::LogBatch<'_>,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        if tokio::runtime::Handle::try_current().is_ok() {
            self.inner.export(batch).await
        } else {
            self.rt.block_on(self.inner.export(batch))
        }
    }

    fn shutdown_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn set_resource(&mut self, resource: &Resource) {
        self.inner.set_resource(resource);
    }
}

/// Wraps an OTLP `MetricExporter` so that periodic export runs inside a
/// dedicated Tokio runtime for the same reason as `TokioSpanExporter`.
#[derive(Debug)]
struct TokioMetricExporter {
    inner: opentelemetry_otlp::MetricExporter,
    rt: Arc<tokio::runtime::Runtime>,
}

impl opentelemetry_sdk::metrics::exporter::PushMetricExporter for TokioMetricExporter {
    async fn export(
        &self,
        metrics: &opentelemetry_sdk::metrics::data::ResourceMetrics,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        if tokio::runtime::Handle::try_current().is_ok() {
            self.inner.export(metrics).await
        } else {
            self.rt.block_on(self.inner.export(metrics))
        }
    }

    fn force_flush(&self) -> opentelemetry_sdk::error::OTelSdkResult {
        self.inner.force_flush()
    }

    fn shutdown_with_timeout(
        &self,
        timeout: std::time::Duration,
    ) -> opentelemetry_sdk::error::OTelSdkResult {
        self.inner.shutdown_with_timeout(timeout)
    }

    fn temporality(&self) -> Temporality {
        self.inner.temporality()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExporterType {
    Otlp,
    Console,
    None,
}

impl ExporterType {
    pub fn from_env_value(value: &str) -> Self {
        match value.to_lowercase().as_str() {
            "" | "otlp" => ExporterType::Otlp,
            "console" | "stdout" => ExporterType::Console,
            _ => ExporterType::None,
        }
    }
}

/// Resolved transport protocol for an OTLP signal, per OTel spec:
/// signal-specific `OTEL_EXPORTER_OTLP_{SIGNAL}_PROTOCOL` overrides the
/// shared `OTEL_EXPORTER_OTLP_PROTOCOL`, and the default is `http/protobuf`
/// (matching what `.with_http()` produces in this build).
///
/// goose's `opentelemetry-otlp` build only enables the `http-proto` /
/// `reqwest-blocking-client` transport features — not `grpc-tonic`. If the caller's
/// environment sets `…_PROTOCOL=grpc`, the `.with_http()` exporter still
/// builds successfully but its background batch / metric reader threads
/// panic on the first export with
/// `internal error: entered unreachable code: HTTP client should not
/// receive Grpc protocol`. We honour the env var by skipping the signal
/// rather than crashing detached threads.
fn signal_protocol_is_http(signal: &str) -> bool {
    let signal_var = format!("OTEL_EXPORTER_OTLP_{}_PROTOCOL", signal.to_uppercase());
    let raw = env::var(&signal_var)
        .ok()
        .filter(|v| !v.is_empty())
        .or_else(|| env::var("OTEL_EXPORTER_OTLP_PROTOCOL").ok())
        .unwrap_or_default();
    match raw.trim().to_lowercase().as_str() {
        // Default per spec when unset — matches `.with_http()`.
        "" | "http/protobuf" | "http/json" => true,
        // gRPC variants require the `grpc-tonic` feature, which goose
        // does not enable.
        _ => false,
    }
}

/// Returns the exporter type for a signal, or None if disabled.
///
/// Checks in order:
/// 1. OTEL_SDK_DISABLED — disables everything
/// 2. OTEL_{SIGNAL}_EXPORTER — explicit exporter selection ("none" disables)
/// 3. OTEL_EXPORTER_OTLP_{SIGNAL}_ENDPOINT or OTEL_EXPORTER_OTLP_ENDPOINT — enables OTLP
pub(crate) fn signal_exporter(
    signal: &str,
    overrides: &OtlpConfigOverrides,
) -> Option<ExporterType> {
    if env::var("OTEL_SDK_DISABLED")
        .ok()
        .is_some_and(|v| v.eq_ignore_ascii_case("true"))
    {
        return None;
    }

    let exporter_var = format!("OTEL_{}_EXPORTER", signal.to_uppercase());
    if let Ok(val) = env::var(&exporter_var) {
        let typ = ExporterType::from_env_value(&val);
        return if matches!(typ, ExporterType::None) {
            None
        } else {
            Some(typ)
        };
    }

    let signal_endpoint = format!("OTEL_EXPORTER_OTLP_{}_ENDPOINT", signal.to_uppercase());
    let has_endpoint = env::var(&signal_endpoint)
        .ok()
        .is_some_and(|v| !v.is_empty())
        || env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
            .ok()
            .is_some_and(|v| !v.is_empty())
        || overrides.has_endpoint();

    if has_endpoint {
        Some(ExporterType::Otlp)
    } else {
        None
    }
}

fn create_resource() -> Resource {
    use crate::session_context::{session_host, session_user};

    let mut builder = Resource::builder_empty()
        .with_attributes([
            KeyValue::new("service.name", "goose"),
            KeyValue::new("service.version", env!("CARGO_PKG_VERSION")),
            KeyValue::new("service.namespace", "goose"),
            KeyValue::new("host.name", session_host()),
            KeyValue::new("user.name", session_user()),
        ])
        .with_detector(Box::new(EnvResourceDetector::new()))
        .with_detector(Box::new(TelemetryResourceDetector));

    // OTEL_SERVICE_NAME takes highest priority (skip SdkProvidedResourceDetector
    // which would fall back to "unknown_service" when unset)
    if let Ok(name) = std::env::var("OTEL_SERVICE_NAME") {
        if !name.is_empty() {
            builder = builder.with_service_name(name);
        }
    }
    builder.build()
}

/// Initializes all OTLP signal layers (traces, metrics, logs) and propagation.
/// Returns boxed layers ready to add to a subscriber.
pub fn init_otlp_layers(
    config: &crate::config::Config,
) -> Vec<Box<dyn tracing_subscriber::Layer<tracing_subscriber::Registry> + Send + Sync>> {
    let overrides = OtlpConfigOverrides::from_config(config);

    let mut layers: Vec<
        Box<dyn tracing_subscriber::Layer<tracing_subscriber::Registry> + Send + Sync>,
    > = Vec::new();

    if let Ok(layer) = create_otlp_tracing_layer(&overrides) {
        layers.push(layer.with_filter(create_otlp_tracing_filter()).boxed());
    }
    if let Ok(layer) = create_otlp_metrics_layer(&overrides) {
        layers.push(layer.with_filter(create_otlp_metrics_filter()).boxed());
    }
    if let Ok(bridge) = create_otlp_logs_layer(&overrides) {
        layers.push(bridge.with_filter(create_otlp_logs_filter()).boxed());
    }

    if !layers.is_empty() {
        global::set_text_map_propagator(TraceContextPropagator::new());
    }

    layers
}

fn create_otlp_tracing_layer(overrides: &OtlpConfigOverrides) -> OtlpResult<OtlpTracingLayer> {
    let exporter = signal_exporter("traces", overrides).ok_or("Traces not enabled")?;
    let resource = create_resource();

    let tracer_provider = match exporter {
        ExporterType::Otlp => {
            if !signal_protocol_is_http("traces") {
                warn_grpc_protocol_skipped_once();
                return Err("OTLP traces protocol is grpc but goose was built without grpc-tonic; skipping traces exporter".into());
            }
            let rt = get_or_create_otel_rt()?;
            let exporter = TokioSpanExporter {
                inner: overrides
                    .apply(
                        opentelemetry_otlp::SpanExporter::builder().with_http(),
                        "traces",
                    )
                    .build()?,
                rt,
            };
            SdkTracerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource)
                .build()
        }
        ExporterType::Console => {
            let exporter = opentelemetry_stdout::SpanExporter::default();
            SdkTracerProvider::builder()
                .with_simple_exporter(exporter)
                .with_resource(resource)
                .build()
        }
        ExporterType::None => return Err("Traces exporter set to none".into()),
    };

    global::set_tracer_provider(tracer_provider.clone());
    let tracer = tracer_provider.tracer("goose");
    *TRACER_PROVIDER.lock().unwrap_or_else(|e| e.into_inner()) = Some(tracer_provider);

    Ok(tracing_opentelemetry::layer().with_tracer(tracer))
}

// TODO: remove once https://github.com/open-telemetry/opentelemetry-rust/pull/3351 is released.
fn temporality_preference() -> Temporality {
    match env::var("OTEL_EXPORTER_OTLP_METRICS_TEMPORALITY_PREFERENCE")
        .unwrap_or_default()
        .to_lowercase()
        .as_str()
    {
        "delta" => Temporality::Delta,
        "lowmemory" => Temporality::LowMemory,
        _ => Temporality::Cumulative,
    }
}

fn create_otlp_metrics_layer(overrides: &OtlpConfigOverrides) -> OtlpResult<OtlpMetricsLayer> {
    let exporter = signal_exporter("metrics", overrides).ok_or("Metrics not enabled")?;
    let resource = create_resource();

    let meter_provider = match exporter {
        ExporterType::Otlp => {
            if !signal_protocol_is_http("metrics") {
                warn_grpc_protocol_skipped_once();
                return Err("OTLP metrics protocol is grpc but goose was built without grpc-tonic; skipping metrics exporter".into());
            }
            let rt = get_or_create_otel_rt()?;
            let exporter = TokioMetricExporter {
                inner: overrides
                    .apply(
                        opentelemetry_otlp::MetricExporter::builder().with_http(),
                        "metrics",
                    )
                    .with_temporality(temporality_preference())
                    .build()?,
                rt,
            };
            SdkMeterProvider::builder()
                .with_resource(resource)
                .with_periodic_exporter(exporter)
                .build()
        }
        ExporterType::Console => {
            let exporter = opentelemetry_stdout::MetricExporter::default();
            SdkMeterProvider::builder()
                .with_resource(resource)
                .with_periodic_exporter(exporter)
                .build()
        }
        ExporterType::None => return Err("Metrics exporter set to none".into()),
    };

    global::set_meter_provider(meter_provider.clone());
    *METER_PROVIDER.lock().unwrap_or_else(|e| e.into_inner()) = Some(meter_provider.clone());

    Ok(MetricsLayer::new(meter_provider))
}

fn create_otlp_logs_layer(overrides: &OtlpConfigOverrides) -> OtlpResult<OtlpLogsLayer> {
    let exporter = signal_exporter("logs", overrides).ok_or("Logs not enabled")?;
    let resource = create_resource();

    let logger_provider = match exporter {
        ExporterType::Otlp => {
            if !signal_protocol_is_http("logs") {
                warn_grpc_protocol_skipped_once();
                return Err("OTLP logs protocol is grpc but goose was built without grpc-tonic; skipping logs exporter".into());
            }
            let rt = get_or_create_otel_rt()?;
            let exporter = TokioLogExporter {
                inner: overrides
                    .apply(
                        opentelemetry_otlp::LogExporter::builder().with_http(),
                        "logs",
                    )
                    .build()?,
                rt,
            };
            SdkLoggerProvider::builder()
                .with_batch_exporter(exporter)
                .with_resource(resource)
                .build()
        }
        ExporterType::Console => {
            let exporter = opentelemetry_stdout::LogExporter::default();
            SdkLoggerProvider::builder()
                .with_simple_exporter(exporter)
                .with_resource(resource)
                .build()
        }
        ExporterType::None => return Err("Logs exporter set to none".into()),
    };

    let bridge = OpenTelemetryTracingBridge::builder(&logger_provider)
        .with_tracing_span_attributes(TracingSpanAttributes::allowlist([
            "session.id",
            "session.user",
            "session.host",
            "session.agent_type",
        ]))
        .build();
    *LOGGER_PROVIDER.lock().unwrap_or_else(|e| e.into_inner()) = Some(logger_provider);

    Ok(bridge)
}

pub fn is_otlp_initialized() -> bool {
    TRACER_PROVIDER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some()
        || METER_PROVIDER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
        || LOGGER_PROVIDER
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
}

/// Creates a custom filter for OTLP tracing that captures:
/// - All spans at INFO level and above
/// - Specific spans marked with "otel.trace" field
/// - Events from specific modules related to telemetry
fn create_otlp_tracing_filter() -> FilterFn<impl Fn(&Metadata<'_>) -> bool> {
    FilterFn::new(|metadata: &Metadata<'_>| {
        if is_otlp_suppressed_target(metadata.target()) {
            return false;
        }

        if metadata.level() <= &Level::INFO {
            return true;
        }

        if metadata.level() == &Level::DEBUG {
            let target = metadata.target();
            if target.starts_with("goose::")
                || target.starts_with("opentelemetry")
                || target.starts_with("tracing_opentelemetry")
            {
                return true;
            }
        }

        false
    })
}

/// Creates a custom filter for OTLP metrics that captures:
/// - All events at INFO level and above
/// - Specific events marked with "otel.metric" field
/// - Events that should be converted to metrics
fn create_otlp_metrics_filter() -> FilterFn<impl Fn(&Metadata<'_>) -> bool> {
    FilterFn::new(|metadata: &Metadata<'_>| {
        if metadata.level() <= &Level::INFO {
            return true;
        }

        if metadata.level() == &Level::DEBUG {
            let target = metadata.target();
            if target.starts_with("goose::telemetry")
                || target.starts_with("goose::metrics")
                || target.contains("metric")
            {
                return true;
            }
        }

        false
    })
}

fn parse_level(s: &str) -> Option<Level> {
    match s.to_lowercase().as_str() {
        "trace" => Some(Level::TRACE),
        "debug" => Some(Level::DEBUG),
        "info" => Some(Level::INFO),
        "warn" => Some(Level::WARN),
        "error" => Some(Level::ERROR),
        _ => None,
    }
}

fn otel_logs_level() -> Level {
    env::var("OTEL_LOG_LEVEL")
        .ok()
        .and_then(|s| parse_level(&s))
        .unwrap_or(Level::INFO)
}

/// Targets suppressed from OTLP trace and log export.
///
/// `rmcp::service` logs the full `InitializeResult` (including extension instructions
/// and user memory content) as a `peer_info` attribute on every MCP handshake.
/// This can be 400KB+ per session init and contains PII/sensitive data.
/// These events have no analytical value in OTLP — suppress them entirely.
const OTLP_SUPPRESSED_TARGETS: &[&str] = &["rmcp::service"];

fn is_otlp_suppressed_target(target: &str) -> bool {
    OTLP_SUPPRESSED_TARGETS.iter().any(|suppressed| {
        target == *suppressed
            || target
                .strip_prefix(suppressed)
                .is_some_and(|suffix| suffix.starts_with("::"))
    })
}

/// Creates a custom filter for OTLP logs.
/// Valid RUST_LOG directives take precedence over OTEL_LOG_LEVEL and default INFO.
/// Invalid RUST_LOG values fall back to OTEL_LOG_LEVEL and default INFO.
/// Suppresses targets listed in `OTLP_SUPPRESSED_TARGETS`.
fn create_otlp_logs_filter() -> impl tracing_subscriber::layer::Filter<tracing_subscriber::Registry>
{
    let filter = match env::var("RUST_LOG") {
        Ok(value) if !value.trim().is_empty() => EnvFilter::try_new(value)
            .unwrap_or_else(|_| EnvFilter::new(otel_logs_level().to_string())),
        _ => EnvFilter::new(otel_logs_level().to_string()),
    };

    filter.and(FilterFn::new(|metadata: &Metadata<'_>| {
        !is_otlp_suppressed_target(metadata.target())
    }))
}

pub fn shutdown_otlp() {
    let timeout = std::time::Duration::from_millis(
        crate::config::Config::global()
            .get_param::<u64>("otel_shutdown_timeout_ms")
            .unwrap_or(5000),
    );

    if let Some(provider) = TRACER_PROVIDER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        if let Err(e) = provider.shutdown_with_timeout(timeout) {
            tracing::warn!("OTLP tracer provider shutdown error: {e}");
        }
    }
    if let Some(provider) = METER_PROVIDER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        if let Err(e) = provider.shutdown_with_timeout(timeout) {
            tracing::warn!("OTLP meter provider shutdown error: {e}");
        }
    }
    if let Some(provider) = LOGGER_PROVIDER
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        if let Err(e) = provider.shutdown_with_timeout(timeout) {
            tracing::warn!("OTLP logger provider shutdown error: {e}");
        }
    }

    if let Some(rt) = OTEL_RT.lock().unwrap_or_else(|e| e.into_inner()).take() {
        // Dropping a `Runtime` inside an async context panics with "Cannot drop
        // a runtime in a context where blocking is not allowed". Move the drop
        // off-runtime via `shutdown_background` (which takes ownership of
        // `Runtime` and shuts down without blocking the caller).  If we still
        // have other `Arc` clones alive, fall back to dropping on a plain thread.
        match Arc::try_unwrap(rt) {
            Ok(runtime) => runtime.shutdown_background(),
            Err(arc) => {
                std::thread::spawn(move || drop(arc));
            }
        }
    }
}

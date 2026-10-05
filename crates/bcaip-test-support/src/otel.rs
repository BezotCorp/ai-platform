use opentelemetry::metrics::{Meter, MeterProvider};
use opentelemetry::{InstrumentationScope, global};
use std::sync::Arc;
struct SavedMeterProvider(Arc<dyn MeterProvider + Send + Sync>);

impl MeterProvider for SavedMeterProvider {
    fn meter_with_scope(&self, scope: InstrumentationScope) -> Meter {
        self.0.meter_with_scope(scope)
    }
}

pub struct OtelTestGuard {
    pub _env: env_lock::EnvGuard<'static>,
    prev_tracer: global::GlobalTracerProvider,
    prev_meter: Arc<dyn MeterProvider + Send + Sync>,
}

impl Drop for OtelTestGuard {
    fn drop(&mut self) {
        global::set_tracer_provider(self.prev_tracer.clone());
        global::set_meter_provider(SavedMeterProvider(self.prev_meter.clone()));
    }
}

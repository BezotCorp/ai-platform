use chrono::Utc;
use serde_json::{Value, json};
use std::{collections::HashMap, fmt, sync::Arc};
use tokio::sync::Mutex;
use tracing::field::{Field, Visit};
use tracing::{Event, Id, Level, Metadata, Subscriber, span};
use tracing_subscriber::{Layer, layer::Context, registry::LookupSpan};
use uuid::Uuid;
#[derive(Debug, Clone)]
pub struct SpanData {
    pub observation_id: String, // Langfuse requires ids to be UUID v4 strings
    pub name: String,
    pub start_time: String,
    pub level: String,
    pub metadata: serde_json::Map<String, Value>,
    pub parent_span_id: Option<u64>,
}

pub fn map_level(level: &Level) -> &'static str {
    match *level {
        Level::ERROR => "ERROR",
        Level::WARN => "WARNING",
        Level::INFO => "DEFAULT",
        Level::DEBUG => "DEBUG",
        Level::TRACE => "DEBUG",
    }
}

pub fn flatten_metadata(
    metadata: serde_json::Map<String, Value>,
) -> serde_json::Map<String, Value> {
    let mut flattened = serde_json::Map::new();
    for (key, value) in metadata {
        match value {
            Value::String(s) => {
                flattened.insert(key, json!(s));
            }
            Value::Object(mut obj) => {
                if let Some(text) = obj.remove("text") {
                    flattened.insert(key, text);
                } else {
                    flattened.insert(key, json!(obj));
                }
            }
            _ => {
                flattened.insert(key, value);
            }
        }
    }
    flattened
}

pub trait BatchManager: Send + Sync + 'static {
    fn add_event(&mut self, event_type: &str, body: Value);
    fn send(&mut self) -> Result<(), Box<dyn std::error::Error + Send + Sync>>;
    fn is_empty(&self) -> bool;
}

#[derive(Debug)]
pub struct SpanTracker {
    active_spans: HashMap<u64, String>, // span_id -> observation_id. span_id in Tracing is u64 whereas Langfuse requires UUID v4 strings
    current_trace_id: Option<String>,
}

impl Default for SpanTracker {
    fn default() -> Self {
        Self::new()
    }
}

impl SpanTracker {
    pub fn new() -> Self {
        Self {
            active_spans: HashMap::new(),
            current_trace_id: None,
        }
    }

    pub fn add_span(&mut self, span_id: u64, observation_id: String) {
        self.active_spans.insert(span_id, observation_id);
    }

    pub fn get_span(&self, span_id: u64) -> Option<&String> {
        self.active_spans.get(&span_id)
    }

    pub fn remove_span(&mut self, span_id: u64) -> Option<String> {
        self.active_spans.remove(&span_id)
    }
}

#[derive(Clone)]
pub struct ObservationLayer {
    pub batch_manager: Arc<Mutex<dyn BatchManager>>,
    pub span_tracker: Arc<Mutex<SpanTracker>>,
}

impl ObservationLayer {
    pub async fn handle_span(&self, span_id: u64, span_data: SpanData) {
        let observation_id = span_data.observation_id.clone();

        {
            let mut spans = self.span_tracker.lock().await;
            spans.add_span(span_id, observation_id.clone());
        }

        // Get parent ID if it exists
        let parent_id = if let Some(parent_span_id) = span_data.parent_span_id {
            let spans = self.span_tracker.lock().await;
            spans.get_span(parent_span_id).cloned()
        } else {
            None
        };

        let trace_id = self.ensure_trace_id().await;

        // Create the span observation
        let mut batch = self.batch_manager.lock().await;
        batch.add_event(
            "observation-create",
            json!({
                "id": observation_id,
                "traceId": trace_id,
                "type": "SPAN",
                "name": span_data.name,
                "startTime": span_data.start_time,
                "parentObservationId": parent_id,
                "metadata": span_data.metadata,
                "level": span_data.level
            }),
        );
    }

    pub async fn handle_span_close(&self, span_id: u64) {
        let observation_id = {
            let mut spans = self.span_tracker.lock().await;
            spans.remove_span(span_id)
        };

        if let Some(observation_id) = observation_id {
            let trace_id = self.ensure_trace_id().await;
            let mut batch = self.batch_manager.lock().await;
            batch.add_event(
                "observation-update",
                json!({
                    "id": observation_id,
                    "type": "SPAN",
                    "traceId": trace_id,
                    "endTime": Utc::now().to_rfc3339()
                }),
            );
        }
    }

    pub async fn ensure_trace_id(&self) -> String {
        let mut spans = self.span_tracker.lock().await;
        if let Some(id) = spans.current_trace_id.clone() {
            return id;
        }

        let trace_id = Uuid::new_v4().to_string();
        spans.current_trace_id = Some(trace_id.clone());

        let mut batch = self.batch_manager.lock().await;
        batch.add_event(
            "trace-create",
            json!({
                "id": trace_id,
                "name": Utc::now().timestamp().to_string(),
                "timestamp": Utc::now().to_rfc3339(),
                "input": {},
                "metadata": {},
                "tags": [],
                "public": false
            }),
        );

        trace_id
    }

    pub async fn update_trace(&self, updates: serde_json::Map<String, Value>) {
        let trace_id = self.ensure_trace_id().await;
        let mut body = json!({ "id": trace_id });
        for (k, v) in updates {
            body[k] = v;
        }
        let mut batch = self.batch_manager.lock().await;
        batch.add_event("trace-create", body);
    }

    pub async fn handle_record(&self, span_id: u64, metadata: serde_json::Map<String, Value>) {
        // Handle trace-level fields by updating the trace itself
        let trace_fields: Vec<&str> = vec!["trace_input", "trace_output"];
        let has_trace_fields = trace_fields.iter().any(|f| metadata.contains_key(*f));

        if has_trace_fields {
            let mut trace_updates = serde_json::Map::new();
            if let Some(val) = metadata.get("trace_input") {
                trace_updates.insert("input".to_string(), val.clone());
            }
            if let Some(val) = metadata.get("trace_output") {
                trace_updates.insert("output".to_string(), val.clone());
            }
            if !trace_updates.is_empty() {
                self.update_trace(trace_updates).await;
            }
        }

        // Filter out trace-level fields from span metadata
        let span_metadata: serde_json::Map<String, Value> = metadata
            .into_iter()
            .filter(|(k, _)| !trace_fields.contains(&k.as_str()))
            .collect();

        if span_metadata.is_empty() {
            return;
        }

        let observation_id = {
            let spans = self.span_tracker.lock().await;
            spans.get_span(span_id).cloned()
        };

        if let Some(observation_id) = observation_id {
            let trace_id = self.ensure_trace_id().await;

            let mut update = json!({
                "id": observation_id,
                "traceId": trace_id,
                "type": "SPAN"
            });

            // Handle special fields
            if let Some(val) = span_metadata.get("input") {
                update["input"] = val.clone();
            }

            if let Some(val) = span_metadata.get("output") {
                update["output"] = val.clone();
            }

            if let Some(val) = span_metadata.get("model_config") {
                update["metadata"] = json!({ "model_config": val });
            }

            // Handle any remaining metadata
            let remaining_metadata: serde_json::Map<String, Value> = span_metadata
                .iter()
                .filter(|(k, _)| !["input", "output", "model_config"].contains(&k.as_str()))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();

            if !remaining_metadata.is_empty() {
                let flattened = flatten_metadata(remaining_metadata);
                if update.get("metadata").is_some() {
                    if let Some(obj) = update["metadata"].as_object_mut() {
                        for (k, v) in flattened {
                            obj.insert(k, v);
                        }
                    }
                } else {
                    update["metadata"] = json!(flattened);
                }
            }

            let mut batch = self.batch_manager.lock().await;
            batch.add_event("span-update", update);
        }
    }
}

impl<S> Layer<S> for ObservationLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn enabled(&self, metadata: &Metadata<'_>, _ctx: Context<'_, S>) -> bool {
        metadata.target().starts_with("bcaip::")
    }

    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let span_id = id.into_u64();

        let parent_span_id = ctx
            .span_scope(id)
            .and_then(|mut scope| scope.nth(1))
            .map(|parent| parent.id().into_u64());

        let mut visitor = JsonVisitor::new();
        attrs.record(&mut visitor);

        let span_data = SpanData {
            observation_id: Uuid::new_v4().to_string(),
            name: attrs.metadata().name().to_string(),
            start_time: Utc::now().to_rfc3339(),
            level: map_level(attrs.metadata().level()).to_owned(),
            metadata: visitor.recorded_fields,
            parent_span_id,
        };

        let layer = self.clone();
        tokio::spawn(async move { layer.handle_span(span_id, span_data).await });
    }

    fn on_close(&self, id: Id, _ctx: Context<'_, S>) {
        let span_id = id.into_u64();
        let layer = self.clone();
        tokio::spawn(async move { layer.handle_span_close(span_id).await });
    }

    fn on_record(&self, span: &Id, values: &span::Record<'_>, _ctx: Context<'_, S>) {
        let span_id = span.into_u64();
        let mut visitor = JsonVisitor::new();
        values.record(&mut visitor);
        let metadata = visitor.recorded_fields;

        if !metadata.is_empty() {
            let layer = self.clone();
            tokio::spawn(async move { layer.handle_record(span_id, metadata).await });
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut visitor = JsonVisitor::new();
        event.record(&mut visitor);
        let metadata = visitor.recorded_fields;

        if let Some(span_id) = ctx.lookup_current().map(|span| span.id().into_u64()) {
            let layer = self.clone();
            tokio::spawn(async move { layer.handle_record(span_id, metadata).await });
        }
    }
}

#[derive(Debug)]
struct JsonVisitor {
    recorded_fields: serde_json::Map<String, Value>,
}

impl JsonVisitor {
    fn new() -> Self {
        Self {
            recorded_fields: serde_json::Map::new(),
        }
    }

    fn insert_value(&mut self, field: &Field, value: Value) {
        self.recorded_fields.insert(field.name().to_string(), value);
    }
}

macro_rules! record_field {
    ($fn_name:ident, $type:ty) => {
        fn $fn_name(&mut self, field: &Field, value: $type) {
            self.insert_value(field, Value::from(value));
        }
    };
}

impl Visit for JsonVisitor {
    record_field!(record_i64, i64);
    record_field!(record_u64, u64);
    record_field!(record_bool, bool);
    record_field!(record_str, &str);

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.insert_value(field, Value::String(format!("{:?}", value)));
    }
}

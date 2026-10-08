//! Structured provider request/response observability for GDK callers.
//!
//! Kotlin and Python consumers register an [`ObservabilityHook`] to receive
//! typed lifecycle events (start, response metadata, completion) for both
//! streaming and non-streaming provider calls instead of scraping logs.
//!
//! Hooks are opt-in: with no hook registered nothing is emitted and no work is
//! performed on the request path. The hook is invoked synchronously, wrapped in
//! `catch_unwind` so a throwing foreign callback cannot fail the request.

use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

use crate::bindings::{BcaipError, BcaipStreamError, Usage};
use bcaip_provider_types::conversations::Message;
use rmcp::model::Tool;

/// Receives structured provider request lifecycle events.
#[uniffi::export(callback_interface)]
pub trait ObservabilityHook: Send + Sync {
    fn on_request_start(&self, event: RequestStartEvent);
    fn on_response_start(&self, event: ResponseStartEvent);
    fn on_request_end(&self, event: RequestEndEvent);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RequestOperation {
    Complete,
    Stream,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RequestPayload {
    pub system: String,
    pub messages_json: String,
    pub tools_json: String,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RequestStartEvent {
    pub request_id: String,
    pub provider: String,
    pub model: String,
    pub operation: RequestOperation,
    pub payload: Option<RequestPayload>,
}

/// Emitted when the provider response becomes available: when the stream is
/// opened for streaming requests, or when the body is received otherwise.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ResponseStartEvent {
    pub request_id: String,
    pub provider: String,
    pub model: String,
    pub operation: RequestOperation,
    pub elapsed_ms: u64,
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct RequestEndEvent {
    pub request_id: String,
    pub provider: String,
    pub model: String,
    pub operation: RequestOperation,
    pub outcome: RequestOutcome,
    pub duration_ms: u64,
    pub usage: Option<Usage>,
    pub response_json: Option<String>,
}

#[derive(Debug, Clone, uniffi::Enum)]
pub enum RequestOutcome {
    Success,
    Failure { error: BcaipStreamError },
}

static HOOK: RwLock<Option<Arc<RegisteredHook>>> = RwLock::new(None);
static NEXT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Registers the process-wide observability hook, replacing any previous one.
///
/// Payloads are omitted unless `capture_payloads` is enabled because system
/// prompts, conversations and tool results routinely contain sensitive data.
#[uniffi::export(default(capture_payloads = false))]
pub fn set_observability_hook(hook: Box<dyn ObservabilityHook>, capture_payloads: bool) {
    let registered = Arc::new(RegisteredHook {
        hook,
        capture_payloads,
        revoked: AtomicBool::new(false),
    });
    let previous = HOOK
        .write()
        .expect("observability hook lock")
        .replace(registered);
    if let Some(previous) = previous {
        previous.revoke();
    }
}

/// Removes the observability hook, after which no further events are emitted,
/// including for requests that are still in flight.
#[uniffi::export]
pub fn clear_observability_hook() {
    if let Some(previous) = HOOK.write().expect("observability hook lock").take() {
        previous.revoke();
    }
}

struct RegisteredHook {
    hook: Box<dyn ObservabilityHook>,
    capture_payloads: bool,
    revoked: AtomicBool,
}

impl RegisteredHook {
    fn revoke(&self) {
        self.revoked.store(true, Ordering::Release);
    }

    fn emit(&self, deliver: impl FnOnce(&dyn ObservabilityHook)) {
        if self.revoked.load(Ordering::Acquire) {
            return;
        }
        let _ = catch_unwind(AssertUnwindSafe(|| deliver(self.hook.as_ref())));
    }
}

pub(crate) struct RequestDescriptor<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub operation: RequestOperation,
    pub system: &'a str,
    pub messages: &'a [Message],
    pub tools: &'a [Tool],
}

/// Tracks one provider request and emits its lifecycle events. Disabled (and
/// free) when no hook is registered.
pub(crate) struct RequestObserver {
    active: Option<ActiveRequest>,
}

struct ActiveRequest {
    hook: Arc<RegisteredHook>,
    request_id: String,
    provider: String,
    model: String,
    operation: RequestOperation,
    started: Instant,
    ended: AtomicBool,
}

impl RequestObserver {
    pub(crate) fn start(descriptor: RequestDescriptor<'_>) -> Self {
        let Some(hook) = HOOK.read().expect("observability hook lock").clone() else {
            return Self { active: None };
        };

        let request_id = format!("req-{}", NEXT_REQUEST_ID.fetch_add(1, Ordering::Relaxed));
        let payload = hook.capture_payloads.then(|| RequestPayload {
            system: descriptor.system.to_string(),
            messages_json: serde_json::to_string(descriptor.messages)
                .unwrap_or_else(|_| "null".to_string()),
            tools_json: serde_json::to_string(descriptor.tools)
                .unwrap_or_else(|_| "null".to_string()),
        });

        let event = RequestStartEvent {
            request_id: request_id.clone(),
            provider: descriptor.provider.to_string(),
            model: descriptor.model.to_string(),
            operation: descriptor.operation,
            payload,
        };
        hook.emit(|hook| hook.on_request_start(event));

        Self {
            active: Some(ActiveRequest {
                hook,
                request_id,
                provider: descriptor.provider.to_string(),
                model: descriptor.model.to_string(),
                operation: descriptor.operation,
                started: Instant::now(),
                ended: AtomicBool::new(false),
            }),
        }
    }

    pub(crate) fn captures_payloads(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.hook.capture_payloads)
    }

    pub(crate) fn response_started(&self) {
        let Some(active) = self.active.as_ref() else {
            return;
        };

        let event = ResponseStartEvent {
            request_id: active.request_id.clone(),
            provider: active.provider.clone(),
            model: active.model.clone(),
            operation: active.operation,
            elapsed_ms: active.started.elapsed().as_millis() as u64,
        };
        active.hook.emit(|hook| hook.on_response_start(event));
    }

    pub(crate) fn succeeded(&self, usage: Option<Usage>, response_json: Option<String>) {
        self.end(RequestOutcome::Success, usage, response_json);
    }

    pub(crate) fn fail(&self, error: BcaipError) -> BcaipError {
        self.end(
            RequestOutcome::Failure {
                error: BcaipStreamError::from(&error),
            },
            None,
            None,
        );
        error
    }

    pub(crate) fn fail_stream(&self, error: BcaipStreamError) {
        self.end(RequestOutcome::Failure { error }, None, None);
    }

    fn end(&self, outcome: RequestOutcome, usage: Option<Usage>, response_json: Option<String>) {
        let Some(active) = self.active.as_ref() else {
            return;
        };

        if active.ended.swap(true, Ordering::AcqRel) {
            return;
        }

        let event = RequestEndEvent {
            request_id: active.request_id.clone(),
            provider: active.provider.clone(),
            model: active.model.clone(),
            operation: active.operation,
            outcome,
            duration_ms: active.started.elapsed().as_millis() as u64,
            usage,
            response_json,
        };
        active.hook.emit(|hook| hook.on_request_end(event));
    }
}

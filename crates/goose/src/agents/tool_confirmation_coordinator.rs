use std::collections::HashMap;
use std::sync::{Arc, Mutex as StdMutex};

use anyhow::{Result, anyhow};
use bcaip_provider_types::permission::Permission;
use tokio::sync::{Mutex, Notify, OwnedMutexGuard};
use tokio_util::sync::CancellationToken;
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ConfirmationAnswer {
    LiveHandled,
    StateMachine(Permission),
}

pub(crate) struct SessionToolConfirmationState {
    // Held for the lifetime of one Agent::reply stream, including confirmation waits and resumes.
    turn_lock: Arc<Mutex<()>>,
    // Serializes submit_tool_confirmation so concurrent answers cannot both be accepted.
    pub(crate) confirmation_submission_lock: Mutex<()>,
    // Tracks requests from the current confirmation pause; None means still unanswered.
    confirmations: StdMutex<HashMap<String, Option<ConfirmationAnswer>>>,
    // Wakes wait_for_all_confirmation_answers; confirmations remains the source of truth.
    confirmation_answered: Notify,
}

impl SessionToolConfirmationState {
    fn new() -> Self {
        Self {
            turn_lock: Arc::new(Mutex::new(())),
            confirmation_submission_lock: Mutex::new(()),
            confirmations: StdMutex::new(HashMap::new()),
            confirmation_answered: Notify::new(),
        }
    }

    pub(crate) fn try_start_turn(self: &Arc<Self>) -> Result<ActiveTurnGuard> {
        let turn_lock_guard = self
            .turn_lock
            .clone()
            .try_lock_owned()
            .map_err(|_| anyhow!("session already has an active turn"))?;
        Ok(ActiveTurnGuard {
            state: self.clone(),
            _turn_lock_guard: turn_lock_guard,
        })
    }

    pub(crate) fn register_request(&self, request_id: String) {
        self.confirmations
            .lock()
            .expect("tool confirmation state unavailable")
            .entry(request_id)
            .or_insert(None);
    }

    pub(crate) fn answer(&self, request_id: &str) -> Option<ConfirmationAnswer> {
        self.confirmations
            .lock()
            .expect("tool confirmation state unavailable")
            .get(request_id)
            .and_then(Clone::clone)
    }

    pub(crate) fn contains_request(&self, request_id: &str) -> bool {
        self.confirmations
            .lock()
            .expect("tool confirmation state unavailable")
            .contains_key(request_id)
    }

    pub(crate) fn record_answer(&self, request_id: &str, answer: ConfirmationAnswer) -> Result<()> {
        let mut confirmations = self
            .confirmations
            .lock()
            .expect("tool confirmation state unavailable");
        match confirmations.get_mut(request_id) {
            None => return Err(anyhow!("tool confirmation request is no longer active")),
            Some(Some(_)) => return Err(anyhow!("tool confirmation request was already answered")),
            Some(slot @ None) => *slot = Some(answer),
        }
        drop(confirmations);
        self.confirmation_answered.notify_waiters();
        Ok(())
    }

    pub(crate) async fn wait_for_all_confirmation_answers(
        &self,
        cancel: &CancellationToken,
    ) -> Result<bool> {
        loop {
            let answer_received = self.confirmation_answered.notified();
            tokio::pin!(answer_received);
            answer_received.as_mut().enable();

            let completed = {
                let confirmations = self
                    .confirmations
                    .lock()
                    .expect("tool confirmation state unavailable");
                (!confirmations.is_empty() && confirmations.values().all(Option::is_some)).then(
                    || {
                        confirmations.values().any(|answer| {
                            matches!(answer, Some(ConfirmationAnswer::StateMachine(_)))
                        })
                    },
                )
            };
            if let Some(has_state_machine_answer) = completed {
                return Ok(has_state_machine_answer);
            }

            tokio::select! {
                _ = answer_received => {}
                _ = cancel.cancelled() => return Err(anyhow!("state-machine turn cancelled")),
            }
        }
    }

    pub(super) fn clear_confirmations(&self) {
        self.confirmations
            .lock()
            .expect("tool confirmation state unavailable")
            .clear();
    }
}

pub(super) struct ActiveTurnGuard {
    state: Arc<SessionToolConfirmationState>,
    _turn_lock_guard: OwnedMutexGuard<()>,
}

impl ActiveTurnGuard {
    pub(super) fn state(&self) -> &Arc<SessionToolConfirmationState> {
        &self.state
    }
}

impl Drop for ActiveTurnGuard {
    fn drop(&mut self) {
        self.state.clear_confirmations();
    }
}

pub(super) struct ToolConfirmationCoordinator {
    sessions: StdMutex<HashMap<String, Arc<SessionToolConfirmationState>>>,
}

impl ToolConfirmationCoordinator {
    pub(super) fn new() -> Self {
        Self {
            sessions: StdMutex::new(HashMap::new()),
        }
    }

    pub(super) fn session(&self, session_id: &str) -> Arc<SessionToolConfirmationState> {
        self.sessions
            .lock()
            .expect("tool confirmation coordinator unavailable")
            .entry(session_id.to_string())
            .or_insert_with(|| Arc::new(SessionToolConfirmationState::new()))
            .clone()
    }
}

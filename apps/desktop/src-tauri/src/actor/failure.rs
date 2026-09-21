use super::SessionActor;
use engine::{session::StepFatal, SessionError};
use serde::Serialize;
use tauri::{Emitter, Runtime};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct HostFailure {
    pub code: &'static str,
    pub message: String,
}

impl From<StepFatal> for HostFailure {
    fn from(error: StepFatal) -> Self {
        Self::step(error)
    }
}

impl HostFailure {
    pub(super) fn step(error: StepFatal) -> Self {
        Self {
            code: "STEP_FATAL",
            message: error.to_string(),
        }
    }

    pub(super) fn civil(error: SessionError) -> Self {
        match error {
            SessionError::Step(fatal) => Self::step(fatal),
            other => Self {
                code: "CIVIL_DAY_SETTLEMENT_FAILED",
                message: other.to_string(),
            },
        }
    }
}

#[derive(Clone, Serialize)]
struct EngineFailurePayload<'a> {
    session_id: &'a str,
    timeline_id: &'a str,
    code: &'static str,
    message: String,
    events: [engine::Event; 0],
}

impl<R: Runtime> SessionActor<R> {
    pub(super) fn stop_after_step_failure(&mut self, error: StepFatal) {
        self.stop_after_host_failure(HostFailure::step(error));
    }

    pub(super) fn stop_after_host_failure(&mut self, failure: HostFailure) {
        self.running = false;
        let payload = EngineFailurePayload {
            session_id: &self.session_id,
            timeline_id: &self.timeline_id,
            code: failure.code,
            message: failure.message.clone(),
            events: [],
        };
        if let Err(emit_error) = self.app.emit("engine-failure", payload) {
            eprintln!(
                "[session {}] {} notification failed: {emit_error}; {}",
                self.session_id, failure.code, failure.message
            );
        }
        self.cmd_rx.close();
        self.pending_fixed_events.clear();
    }
}

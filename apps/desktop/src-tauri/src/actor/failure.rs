use super::SessionActor;
use engine::session::StepFatal;
use serde::Serialize;
use tauri::{Emitter, Runtime};

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub(super) struct HostFailure {
    pub code: &'static str,
    pub message: String,
}

impl From<StepFatal> for HostFailure {
    fn from(error: StepFatal) -> Self {
        Self {
            code: "STEP_FATAL",
            message: error.to_string(),
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
        self.running = false;
        let failure = HostFailure::from(error);
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

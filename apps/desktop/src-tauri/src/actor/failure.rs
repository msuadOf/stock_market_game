use super::SessionActor;
use engine::session::StepFatal;
use serde::Serialize;
use tauri::{Emitter, Runtime};

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
        let payload = EngineFailurePayload {
            session_id: &self.session_id,
            timeline_id: &self.timeline_id,
            code: "STEP_FATAL",
            message: error.to_string(),
            events: [],
        };
        if let Err(emit_error) = self.app.emit("engine-failure", payload) {
            eprintln!(
                "[session {}] STEP_FATAL notification failed: {emit_error}; {error}",
                self.session_id
            );
        }
        self.cmd_rx.close();
        self.pending_fixed_events.clear();
    }
}

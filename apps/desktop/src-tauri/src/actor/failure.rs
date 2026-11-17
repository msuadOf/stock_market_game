use super::{delivery::Packet, SessionActor};
use engine::session::StepFatal;
use serde::Serialize;
use tauri::Runtime;

#[cfg(test)]
pub(super) use engine::session::authority::failure_description;
pub(super) use engine::session::authority::save_error_message;
pub(crate) use engine::session::authority::HostFailure;

#[derive(Clone, Serialize)]
pub(super) struct EngineFailurePayload {
    session_id: String,
    timeline_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) publication_ordinal: Option<String>,
    #[serde(flatten)]
    failure: HostFailure,
    events: [engine::Event; 0],
}

impl EngineFailurePayload {
    pub(super) fn note_missing_publications(&mut self, from: u64, through: u64) {
        use std::fmt::Write;
        write!(
            self.failure.message,
            "；观察者未收到已提交的发布序号 {from}–{through}"
        )
        .expect("写 String 不会失败");
    }

    #[cfg(test)]
    pub(super) fn terminal_fixture() -> Self {
        let mut failure = HostFailure::archive("terminal fixture");
        failure.code = "TEST_TERMINAL";
        failure.recoverable = false;
        Self {
            session_id: "test".into(),
            timeline_id: "test".into(),
            publication_ordinal: None,
            failure,
            events: [],
        }
    }
}

impl<R: Runtime> SessionActor<R> {
    pub(super) fn stop_after_step_failure(&mut self, error: StepFatal) {
        self.stop_after_host_failure(HostFailure::step(error));
    }

    pub(super) fn stop_after_host_failure(&mut self, failure: HostFailure) {
        let Some(failure) = self.pacing.fail(
            &self.game,
            self.generation,
            failure,
            libos::native_async::now(),
        ) else {
            return;
        };
        let recoverable = failure.recoverable;
        let payload = EngineFailurePayload {
            session_id: self.session_id.clone(),
            timeline_id: self.timeline_id.clone(),
            publication_ordinal: None,
            failure,
            events: [],
        };
        if recoverable {
            self.delivery.stage(Packet::Failure(payload));
        } else {
            self.delivery.stage_terminal(payload);
        }
    }
}

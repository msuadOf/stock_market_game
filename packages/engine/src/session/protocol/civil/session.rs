use super::{CivilUpdate, TickBatch, TickFrame};
use crate::experience::AppendOnlyHistory;
use crate::session::{GameSession, SessionError, SessionSetup, StepFatal};

pub struct ProtocolSession {
    game: GameSession,
    intraday: AppendOnlyHistory<TickFrame>,
    fact_tick: Option<u64>,
    facts_at_tick: Vec<crate::session::protocol::EventFact>,
    #[cfg(test)]
    malformed_frame: bool,
    #[cfg(test)]
    malformed_civil: bool,
}

pub struct ProtocolCheckpoint {
    game: GameSession,
    intraday: AppendOnlyHistory<TickFrame>,
    fact_tick: Option<u64>,
    facts_at_tick: Vec<crate::session::protocol::EventFact>,
}

impl ProtocolSession {
    /// Keep the live state and completed history for an unpublished host batch.
    /// A file-save projection would expand shared history and omit diagnostics.
    pub fn checkpoint(&self) -> Result<ProtocolCheckpoint, StepFatal> {
        self.game.require_healthy()?;
        Ok(ProtocolCheckpoint {
            game: self.game.clone_for_tick_shadow()?,
            intraday: self.intraday.clone(),
            fact_tick: self.fact_tick,
            facts_at_tick: self.facts_at_tick.clone(),
        })
    }

    pub fn rollback(&mut self, checkpoint: ProtocolCheckpoint) {
        self.game = checkpoint.game;
        self.intraday = checkpoint.intraday;
        self.fact_tick = checkpoint.fact_tick;
        self.facts_at_tick = checkpoint.facts_at_tick;
    }

    pub fn civil_day_ready(&self) -> Result<bool, SessionError> {
        Ok(self.game.day()
            == self
                .game
                .civil_clock()
                .completed_trading_sessions_expected()?)
    }

    pub fn enqueue_player_intent(
        &mut self,
        player: crate::AccountId,
        intent: crate::Intent,
    ) -> Result<(), SessionError> {
        self.game.enqueue_player_intent(player, intent)
    }

    pub fn restore(slot: &crate::SaveSlot) -> Result<Self, SessionError> {
        Ok(Self {
            game: GameSession::restore(slot)?,
            intraday: AppendOnlyHistory::default(),
            fact_tick: None,
            facts_at_tick: Vec::new(),
            #[cfg(test)]
            malformed_frame: false,
            #[cfg(test)]
            malformed_civil: false,
        })
    }

    pub fn new(setup: SessionSetup, seed: u64) -> Result<Self, SessionError> {
        Ok(Self {
            game: GameSession::new(setup, seed)?,
            intraday: AppendOnlyHistory::default(),
            fact_tick: None,
            facts_at_tick: Vec::new(),
            #[cfg(test)]
            malformed_frame: false,
            #[cfg(test)]
            malformed_civil: false,
        })
    }

    pub fn game(&self) -> &GameSession {
        &self.game
    }

    pub fn step_frame(&mut self) -> Result<TickFrame, StepFatal> {
        #[cfg(test)]
        let malformed = std::mem::take(&mut self.malformed_frame);
        let checkpoint = self.checkpoint()?;
        let result = self.game.step_frame().and_then(|frame| {
            #[cfg(test)]
            let frame = malformed_frame_if_requested(frame, malformed);
            self.prepare_frame(frame)
        });
        let frame = match result {
            Ok(frame) => frame,
            Err(error) => {
                self.rollback(checkpoint);
                return Err(error);
            }
        };
        drop(checkpoint);
        self.facts_at_tick.extend(frame.facts.iter().cloned());
        self.intraday.push(frame.clone());
        Ok(frame)
    }

    /// Publishes one frame from the normal public protocol path while also
    /// returning the immutable evidence captured by that frame's committed P9.
    pub fn step_frame_with_commit_evidence(
        &mut self,
    ) -> Result<(TickFrame, crate::session::pipeline::TickCommitEvidence), StepFatal> {
        #[cfg(test)]
        let malformed = std::mem::take(&mut self.malformed_frame);
        let checkpoint = self.checkpoint()?;
        let result = self
            .game
            .step_frame_with_commit_evidence()
            .and_then(|(frame, evidence)| {
                #[cfg(test)]
                let frame = malformed_frame_if_requested(frame, malformed);
                self.prepare_frame(frame).map(|frame| (frame, evidence))
            });
        let (frame, evidence) = match result {
            Ok(committed) => committed,
            Err(error) => {
                self.rollback(checkpoint);
                return Err(error);
            }
        };
        drop(checkpoint);
        self.facts_at_tick.extend(frame.facts.iter().cloned());
        self.intraday.push(frame.clone());
        Ok((frame, evidence))
    }

    fn prepare_frame(&mut self, frame: TickFrame) -> Result<TickFrame, StepFatal> {
        self.prepare_fact_tick(frame.tick);
        frame.validate().map_err(protocol_fatal)?;
        Ok(frame)
    }

    pub fn end_civil_day_update(&mut self) -> Result<CivilUpdate, SessionError> {
        #[cfg(test)]
        let malformed = std::mem::take(&mut self.malformed_civil);
        let checkpoint = self.checkpoint()?;
        // Only civil publication needs the full day's transport array. Tick and
        // outer host checkpoints retain shared immutable history chunks.
        let intraday = self.intraday.iter().cloned().collect::<Vec<_>>();
        let result = self
            .game
            .end_civil_day_update(&intraday)
            .and_then(|update| {
                #[cfg(test)]
                let update = {
                    let mut update = update;
                    if malformed {
                        update.seq_to += 1;
                    }
                    update
                };
                let mut update = update;
                self.prepare_fact_tick(update.tick);
                update.facts = crate::session::protocol::attach_facts_after(
                    &self.facts_at_tick,
                    &update.events,
                )
                .map_err(protocol_fatal)?;
                update.validate().map_err(protocol_fatal)?;
                Ok(update)
            });
        let update = match result {
            Ok(update) => update,
            Err(error) => {
                self.rollback(checkpoint);
                return Err(error);
            }
        };
        self.facts_at_tick.extend(update.facts.iter().cloned());
        self.intraday = AppendOnlyHistory::default();
        Ok(update)
    }

    pub fn tick_batch(&self, frames: Vec<TickFrame>) -> Result<TickBatch, StepFatal> {
        let batch = TickBatch {
            frames,
            runtime_snapshot: Some(self.game.runtime_snapshot()),
        };
        batch.validate().map_err(protocol_fatal)?;
        Ok(batch)
    }

    fn prepare_fact_tick(&mut self, tick: u64) {
        if self.fact_tick != Some(tick) {
            self.fact_tick = Some(tick);
            self.facts_at_tick.clear();
        }
    }
}

#[cfg(test)]
fn malformed_frame_if_requested(mut frame: TickFrame, malformed: bool) -> TickFrame {
    if malformed {
        frame.seq_to = frame.seq_to.saturating_add(1);
    }
    frame
}

impl std::ops::Deref for ProtocolSession {
    type Target = GameSession;

    fn deref(&self) -> &Self::Target {
        &self.game
    }
}

fn protocol_fatal(error: super::ProtocolError) -> StepFatal {
    StepFatal::InvariantViolation {
        location: "ProtocolSession publication".to_owned(),
        description: error.to_string(),
    }
}

#[cfg(test)]
mod rollback_tests {
    use super::*;

    #[test]
    fn checkpoint_shares_completed_intraday_frames_until_new_work() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        setup.ticks_per_day = 48;
        let mut session = ProtocolSession::new(setup, 50).unwrap();
        for _ in 0..33 {
            session.step_frame().unwrap();
        }
        let checkpoint = session.checkpoint().unwrap();
        let history = serde_json::to_value(&checkpoint.intraday).unwrap();

        assert!(
            std::ptr::eq(&session.intraday[0], &checkpoint.intraday[0]),
            "creating a rollback point must share completed frames, not copy the trading day"
        );
        session.step_frame().unwrap();
        assert!(
            std::ptr::eq(&session.intraday[0], &checkpoint.intraday[0]),
            "appending a frame must not copy older completed history chunks"
        );
        assert_eq!(serde_json::to_value(&checkpoint.intraday).unwrap(), history);
        assert_eq!(session.intraday.len(), checkpoint.intraday.len() + 1);
    }

    #[test]
    fn outer_rollback_restores_runtime_diagnostics_without_reloading_a_save() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 51).unwrap();
        session.game.last_retail_order_events.push(
            crate::session::RetailOrderDiagnosticEvent::Rejected {
                account: crate::AccountId(1),
                code: crate::StockCode("600101".into()),
                reason: crate::RejectionReason::InsufficientCash,
            },
        );
        let before = session.game.session_state_hash().unwrap();
        let diagnostics = serde_json::to_value(session.game.last_retail_order_events()).unwrap();
        let checkpoint = session.checkpoint().unwrap();
        session.game.last_retail_order_events.clear();

        session.rollback(checkpoint);

        assert_eq!(session.game.session_state_hash().unwrap(), before);
        assert_eq!(
            serde_json::to_value(session.game.last_retail_order_events()).unwrap(),
            diagnostics
        );
    }

    #[test]
    fn outer_rollback_across_close_and_weekend_restores_history_and_can_repeat() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-04").unwrap();
        let steps_before_close = setup.ticks_per_day - 1;
        let mut session = ProtocolSession::new(setup, 52).unwrap();
        for _ in 0..steps_before_close {
            session.step_frame().unwrap();
        }
        let before = session.game.session_state_hash().unwrap();
        let history = serde_json::to_value(&session.intraday).unwrap();
        let fact_tick = session.fact_tick;
        let facts = serde_json::to_value(&session.facts_at_tick).unwrap();
        let checkpoint = session.checkpoint().unwrap();
        let frame = session.step_frame().unwrap();
        let closing = session.end_civil_day_update().unwrap();
        let weekend = session.end_civil_day_update().unwrap();
        assert!(session.intraday.is_empty());
        assert_eq!(weekend.civil_date, "2030-01-06");

        session.rollback(checkpoint);

        assert_eq!(session.game.session_state_hash().unwrap(), before);
        assert_eq!(serde_json::to_value(&session.intraday).unwrap(), history);
        assert_eq!(session.fact_tick, fact_tick);
        assert_eq!(serde_json::to_value(&session.facts_at_tick).unwrap(), facts);
        assert_eq!(
            serde_json::to_value(session.step_frame().unwrap()).unwrap(),
            serde_json::to_value(frame).unwrap()
        );
        assert_eq!(
            serde_json::to_value(session.end_civil_day_update().unwrap()).unwrap(),
            serde_json::to_value(closing).unwrap()
        );
        assert_eq!(
            serde_json::to_value(session.end_civil_day_update().unwrap()).unwrap(),
            serde_json::to_value(weekend).unwrap()
        );
    }

    #[test]
    fn checkpoint_rejects_poison_and_failed_candidate_can_retry_from_healthy_state() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 53).unwrap();
        let fatal = StepFatal::InvariantViolation {
            location: "protocol checkpoint test".into(),
            description: "injected candidate failure".into(),
        };
        let before = session.game.business_state_hash().unwrap();
        session.game.inject_step_failure(fatal.clone());
        assert_eq!(session.step_frame().unwrap_err(), fatal);
        assert_eq!(session.game.business_state_hash().unwrap(), before);
        assert_eq!(session.game.poison_reason(), None);
        session.step_frame().unwrap().validate().unwrap();

        session.game.poison_failed_step(fatal.clone());
        assert!(matches!(session.checkpoint(), Err(error) if error == fatal));
    }

    #[test]
    fn committed_evidence_frame_is_the_same_public_runtime_step() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut ordinary = ProtocolSession::new(setup.clone(), 47).unwrap();
        let mut observed = ProtocolSession::new(setup, 47).unwrap();

        let expected = ordinary.step_frame().unwrap();
        let (actual, evidence) = observed.step_frame_with_commit_evidence().unwrap();

        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(
            evidence.next_receipt_index(),
            observed.game().save().unwrap().runtime_v2.next_receipt_base
        );
    }

    #[test]
    fn malformed_evidence_frame_rolls_back_game_history_and_facts_then_retries() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 48).unwrap();
        let preceding = crate::Event::CivilDateAdvanced {
            seq: session.game.seq(),
            settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        session.fact_tick = Some(session.game.tick());
        session.facts_at_tick = crate::session::protocol::attach_facts(&[preceding]).unwrap();
        let before = serde_json::to_value(session.game.save().unwrap()).unwrap();
        let history = serde_json::to_value(&session.intraday).unwrap();
        let fact_tick = session.fact_tick;
        let facts = serde_json::to_value(&session.facts_at_tick).unwrap();
        session.malformed_frame = true;

        let error = session.step_frame_with_commit_evidence().unwrap_err();

        assert!(matches!(error, StepFatal::InvariantViolation { .. }));
        assert_eq!(
            serde_json::to_value(session.game.save().unwrap()).unwrap(),
            before
        );
        assert_eq!(serde_json::to_value(&session.intraday).unwrap(), history);
        assert_eq!(session.fact_tick, fact_tick);
        assert_eq!(serde_json::to_value(&session.facts_at_tick).unwrap(), facts);
        let (frame, evidence) = session.step_frame_with_commit_evidence().unwrap();
        frame.validate().unwrap();
        assert_eq!(
            evidence.next_receipt_index(),
            session.game().save().unwrap().runtime_v2.next_receipt_base
        );
    }

    #[test]
    fn malformed_civil_after_mutation_rolls_back_and_can_retry() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = ProtocolSession::new(setup, 41).unwrap();
        while !session.civil_day_ready().unwrap() {
            session.step_frame().unwrap();
        }
        let before = serde_json::to_value(session.game.save().unwrap()).unwrap();
        let history = serde_json::to_value(&session.intraday).unwrap();
        session.malformed_civil = true;

        let error = session.end_civil_day_update().unwrap_err();

        assert!(matches!(error, SessionError::Step(_)));
        assert_eq!(
            serde_json::to_value(session.game.save().unwrap()).unwrap(),
            before
        );
        assert_eq!(serde_json::to_value(&session.intraday).unwrap(), history);
        let update = session.end_civil_day_update().unwrap();
        update.validate().unwrap();
        assert_eq!(
            serde_json::to_value(&update.refresh.intraday).unwrap(),
            history
        );
    }

    #[test]
    fn malformed_frame_after_mutation_rolls_back_game_history_and_fact_cursor() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 40).unwrap();
        let preceding = crate::Event::CivilDateAdvanced {
            seq: session.game.seq(),
            settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        session.fact_tick = Some(session.game.tick());
        session.facts_at_tick = crate::session::protocol::attach_facts(&[preceding]).unwrap();
        let before = serde_json::to_value(session.game.save().unwrap()).unwrap();
        let history = serde_json::to_value(&session.intraday).unwrap();
        let fact_tick = session.fact_tick;
        let facts = serde_json::to_value(&session.facts_at_tick).unwrap();
        session.malformed_frame = true;

        let error = session.step_frame().unwrap_err();

        assert!(matches!(error, StepFatal::InvariantViolation { .. }));
        assert_eq!(
            serde_json::to_value(session.game.save().unwrap()).unwrap(),
            before
        );
        assert_eq!(serde_json::to_value(&session.intraday).unwrap(), history);
        assert_eq!(session.fact_tick, fact_tick);
        assert_eq!(serde_json::to_value(&session.facts_at_tick).unwrap(), facts);
        session.step_frame().unwrap().validate().unwrap();
    }

    #[test]
    fn adjacent_civil_updates_continue_but_restore_starts_a_fresh_publication_stream() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-04").unwrap();
        let mut uninterrupted = ProtocolSession::new(setup, 43).unwrap();
        while !uninterrupted.civil_day_ready().unwrap() {
            uninterrupted.step_frame().unwrap();
        }
        let first = uninterrupted.end_civil_day_update().unwrap();
        assert_eq!(first.civil_date, "2030-01-05");
        let saved = uninterrupted.game.save().unwrap();
        let mut restored = ProtocolSession::restore(&saved).unwrap();

        let continued = uninterrupted.end_civil_day_update().unwrap();
        let fresh = restored.end_civil_day_update().unwrap();

        assert_eq!(continued.events, fresh.events);
        let continued_indices = continued
            .facts
            .iter()
            .filter(|fact| is_phase_six_session(fact))
            .map(|fact| fact.key.local_event_index())
            .collect::<Vec<_>>();
        let fresh_indices = fresh
            .facts
            .iter()
            .filter(|fact| is_phase_six_session(fact))
            .map(|fact| fact.key.local_event_index())
            .collect::<Vec<_>>();
        assert!(!continued_indices.is_empty());
        assert_eq!(fresh_indices.first(), Some(&0));
        let preceding_count = first
            .facts
            .iter()
            .filter(|fact| is_phase_six_session(fact))
            .count() as u64;
        assert_eq!(continued_indices.first(), Some(&preceding_count));
    }

    fn is_phase_six_session(fact: &crate::session::protocol::EventFact) -> bool {
        fact.key.phase_rank() == 6
            && fact.key.entity() == &crate::session::pipeline::EntityTag::Session
            && fact.key.source() == crate::session::pipeline::EventSourceIndex::Session
    }
}

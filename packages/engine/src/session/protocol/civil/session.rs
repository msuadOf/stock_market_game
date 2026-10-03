use super::{CivilUpdate, TickBatch, TickFrame};
use crate::experience::AppendOnlyHistory;
use crate::session::{GameSession, SessionError, SessionSetup, StepFatal};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

/// 一个未发布宿主批次的共同回滚集合；GameSession 保留自身 tick authority。
struct ProtocolState {
    game: GameSession,
    intraday: AppendOnlyHistory<TickFrame>,
    fact_cursor: PublicationFactCursor,
    day_end_save: Option<Arc<crate::SaveSlot>>,
    published_runtime: RefCell<Option<super::super::PublicRuntimeState>>,
    pending_save_candidates: RefCell<BTreeMap<u64, Arc<crate::SaveSlot>>>,
}

impl ProtocolState {
    fn try_clone_for_checkpoint(&self) -> Result<Self, StepFatal> {
        self.game.require_healthy()?;
        Ok(Self {
            game: self.game.clone_for_tick_shadow()?,
            intraday: self.intraday.clone(),
            fact_cursor: self.fact_cursor.clone(),
            day_end_save: self.day_end_save.clone(),
            published_runtime: RefCell::new(self.published_runtime.borrow().clone()),
            pending_save_candidates: RefCell::new(self.pending_save_candidates.borrow().clone()),
        })
    }
}

/// 发布流内同 tick 的 preceding facts；恢复新 Session 时从空流开始。
#[derive(Clone, Default)]
struct PublicationFactCursor {
    tick: Option<u64>,
    facts: Vec<crate::session::protocol::EventFact>,
}

impl PublicationFactCursor {
    fn begin_tick(&mut self, tick: u64) {
        if self.tick != Some(tick) {
            self.tick = Some(tick);
            self.facts.clear();
        }
    }

    fn append_committed_facts(&mut self, facts: &[crate::session::protocol::EventFact]) {
        self.facts.extend(facts.iter().cloned());
    }

    fn attach_after(
        &self,
        events: &[crate::Event],
    ) -> Result<Vec<crate::session::protocol::EventFact>, super::super::ProtocolError> {
        crate::session::protocol::attach_facts_after(&self.facts, events)
    }
}

pub struct ProtocolSession {
    state: ProtocolState,
    #[cfg(test)]
    malformed_frame: bool,
    #[cfg(test)]
    malformed_civil: bool,
}

pub struct ProtocolCheckpoint {
    state: ProtocolState,
}

impl ProtocolSession {
    /// 保存未发布宿主批次的 live state 与共享历史；避免文件投影丢失运行时诊断。
    pub fn checkpoint(&self) -> Result<ProtocolCheckpoint, StepFatal> {
        Ok(ProtocolCheckpoint {
            state: self.state.try_clone_for_checkpoint()?,
        })
    }

    pub fn rollback(&mut self, checkpoint: ProtocolCheckpoint) {
        self.state = checkpoint.state;
    }

    pub fn civil_day_ready(&self) -> Result<bool, SessionError> {
        Ok(self.state.game.day()
            == self
                .state
                .game
                .civil_clock()
                .completed_trading_sessions_expected()?)
    }

    pub fn enqueue_player_intent(
        &mut self,
        player: crate::AccountId,
        intent: crate::Intent,
    ) -> Result<(), SessionError> {
        self.state.game.enqueue_player_intent(player, intent)
    }

    pub fn restore(slot: &crate::SaveSlot) -> Result<Self, SessionError> {
        let game = GameSession::restore(slot)?;
        let sessions_through_current_date =
            game.civil_clock().completed_trading_sessions_expected()?;
        let current_date_trades =
            game.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading;
        let sessions_through_settled_date = sessions_through_current_date
            .checked_sub(u32::from(current_date_trades))
            .ok_or_else(|| {
                SessionError::InvalidSave("自然日时钟的已完成交易会话计数不合法".into())
            })?;
        let completed = slot.civil_clock.settled_through.is_some()
            && slot.snapshot.tick.is_multiple_of(slot.setup.ticks_per_day)
            && game.day() == sessions_through_settled_date;
        if !completed {
            return Err(SessionError::InvalidSave(
                "公共日级档必须来自完整的自然日日终结算".into(),
            ));
        }
        if slot
            .resting_orders
            .values()
            .any(|orders| !orders.is_empty())
            || slot
                .auction_orders
                .values()
                .any(|orders| !orders.is_empty())
            || !slot.runtime_v2.live_envelopes.is_empty()
            || !slot.npc_order_lifecycles.is_empty()
            || slot.parent_orders.values().any(|plans| !plans.is_empty())
            || !slot.pending_player.is_empty()
            || slot
                .pending_npc
                .as_ref()
                .is_some_and(|batch| !batch.intents.is_empty())
        {
            return Err(SessionError::InvalidSave(
                "公共日级档不能包含日内活动委托或待处理输入".into(),
            ));
        }
        Ok(Self::from_game(game, Some(Arc::new(slot.clone()))))
    }

    pub fn new(setup: SessionSetup, seed: u64) -> Result<Self, SessionError> {
        Ok(Self::from_game(GameSession::new(setup, seed)?, None))
    }

    /// Restore a validated in-memory verification checkpoint, without making
    /// it available as a public day-end save. Public loading still uses `restore`.
    #[cfg(feature = "verification-harness")]
    pub fn restore_verification_checkpoint(slot: &crate::SaveSlot) -> Result<Self, SessionError> {
        Ok(Self::from_game(GameSession::restore(slot)?, None))
    }

    fn from_game(game: GameSession, day_end_save: Option<Arc<crate::SaveSlot>>) -> Self {
        Self {
            state: ProtocolState {
                game,
                intraday: AppendOnlyHistory::default(),
                fact_cursor: PublicationFactCursor::default(),
                day_end_save,
                published_runtime: RefCell::new(None),
                pending_save_candidates: RefCell::new(BTreeMap::new()),
            },
            #[cfg(test)]
            malformed_frame: false,
            #[cfg(test)]
            malformed_civil: false,
        }
    }

    pub fn game(&self) -> &GameSession {
        &self.state.game
    }

    pub fn save(&self) -> Result<crate::SaveSlot, SessionError> {
        self.state.game.require_healthy()?;
        self.state
            .day_end_save
            .as_ref()
            .map(|slot| slot.as_ref().clone())
            .ok_or_else(|| {
                SessionError::InvalidSave("尚未完成首次自然日日结，没有可用日终存档".into())
            })
    }

    pub fn save_candidate(
        &self,
        key: &super::super::SaveCandidateKey,
    ) -> Result<crate::SaveSlot, SessionError> {
        self.state.game.require_healthy()?;
        let mut pending = self.state.pending_save_candidates.borrow_mut();
        let candidate = pending
            .get(&key.seq)
            .or_else(|| {
                self.state
                    .day_end_save
                    .as_ref()
                    .filter(|slot| slot.snapshot.seq == key.seq)
            })
            .ok_or_else(|| {
                SessionError::InvalidSave(format!(
                    "日终候选 {} / {} 已失效或不存在",
                    key.settled_date, key.seq
                ))
            })?;
        if candidate.civil_clock.settled_through != Some(key.settled_date) {
            return Err(SessionError::InvalidSave(
                "日终候选自然日与请求不一致".into(),
            ));
        }
        let captured = candidate.as_ref().clone();
        pending.remove(&key.seq);
        Ok(captured)
    }

    pub fn discard_pending_save_candidates(&self) {
        self.state.pending_save_candidates.borrow_mut().clear();
    }

    pub fn prepare_public_baseline(&self) {
        self.discard_pending_save_candidates();
        *self.state.published_runtime.borrow_mut() = None;
    }

    pub fn step_frame(&mut self) -> Result<TickFrame, StepFatal> {
        #[cfg(test)]
        let malformed = std::mem::take(&mut self.malformed_frame);
        let checkpoint = self.checkpoint()?;
        let result = self.state.game.step_frame().and_then(|frame| {
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
        self.state.fact_cursor.append_committed_facts(&frame.facts);
        self.state.intraday.push(frame.clone());
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
        let result =
            self.state
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
        self.state.fact_cursor.append_committed_facts(&frame.facts);
        self.state.intraday.push(frame.clone());
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
        let intraday = self.state.intraday.iter().cloned().collect::<Vec<_>>();
        let result = self
            .state
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
                update.facts = self
                    .state
                    .fact_cursor
                    .attach_after(&update.events)
                    .map_err(protocol_fatal)?;
                update.validate().map_err(protocol_fatal)?;
                let candidate = self.state.game.save()?;
                Ok((update, candidate))
            });
        let (update, candidate) = match result {
            Ok(committed) => committed,
            Err(error) => {
                self.rollback(checkpoint);
                return Err(error);
            }
        };
        self.state.fact_cursor.append_committed_facts(&update.facts);
        self.state.intraday = AppendOnlyHistory::default();
        let candidate = Arc::new(candidate);
        self.state
            .pending_save_candidates
            .borrow_mut()
            .insert(candidate.snapshot.seq, Arc::clone(&candidate));
        self.state.day_end_save = Some(candidate);
        *self.state.published_runtime.borrow_mut() = None;
        Ok(update)
    }

    pub fn tick_batch(&self, frames: Vec<TickFrame>) -> Result<TickBatch, StepFatal> {
        let mut previous = self.state.published_runtime.borrow_mut();
        let (batch, current) = self
            .state
            .game
            .tick_batch_delta(frames, previous.as_ref())?;
        *previous = Some(current);
        Ok(batch)
    }

    fn prepare_fact_tick(&mut self, tick: u64) {
        self.state.fact_cursor.begin_tick(tick);
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
        &self.state.game
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
    fn publication_cursor_appends_at_same_tick_and_resets_on_next_tick() {
        let event = |seq| crate::Event::CivilDateAdvanced {
            seq,
            settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        let mut cursor = PublicationFactCursor::default();
        cursor.begin_tick(12);
        let first = cursor.attach_after(&[event(1)]).unwrap();
        assert_eq!(first[0].key.local_event_index(), 0);
        cursor.append_committed_facts(&first);
        cursor.begin_tick(12);
        let second = cursor.attach_after(&[event(2)]).unwrap();
        assert_eq!(second[0].key.local_event_index(), 1);
        // attach_after 的结果只有发布成功后才进入 cursor。
        assert_eq!(cursor.facts.len(), 1);
        cursor.append_committed_facts(&second);
        let checkpoint = cursor.clone();
        cursor.begin_tick(13);
        assert!(cursor.facts.is_empty());
        assert_eq!(
            cursor.attach_after(&[event(3)]).unwrap()[0]
                .key
                .local_event_index(),
            0
        );
        cursor = checkpoint;
        assert_eq!(cursor.tick, Some(12));
        assert_eq!(
            cursor.attach_after(&[event(3)]).unwrap()[0]
                .key
                .local_event_index(),
            2
        );
    }

    #[test]
    fn checkpoint_restores_published_runtime_after_a_successful_batch() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = ProtocolSession::new(setup, 81).unwrap();
        let first = session.step_frame().unwrap();
        session.tick_batch(vec![first]).unwrap();
        let before = session
            .state
            .published_runtime
            .borrow()
            .as_ref()
            .unwrap()
            .test_projection();
        let checkpoint = session.checkpoint().unwrap();
        let frame = session.step_frame().unwrap();
        let first_batch =
            serde_json::to_value(session.tick_batch(vec![frame.clone()]).unwrap()).unwrap();
        assert_ne!(
            session
                .state
                .published_runtime
                .borrow()
                .as_ref()
                .unwrap()
                .test_projection(),
            before
        );
        session.rollback(checkpoint);
        assert_eq!(
            session
                .state
                .published_runtime
                .borrow()
                .as_ref()
                .unwrap()
                .test_projection(),
            before
        );
        let retry = session.step_frame().unwrap();
        assert_eq!(
            serde_json::to_value(session.tick_batch(vec![retry]).unwrap()).unwrap(),
            first_batch
        );
    }

    #[test]
    fn checkpoint_restores_prior_candidates_and_frozen_save_after_two_civil_updates() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-04").unwrap();
        let steps = setup.ticks_per_day;
        let mut session = ProtocolSession::new(setup, 82).unwrap();
        for _ in 0..steps {
            session.step_frame().unwrap();
        }
        let old = session.end_civil_day_update().unwrap();
        let before = session.state.pending_save_candidates.borrow().clone();
        let frozen = session.state.day_end_save.clone().unwrap();
        let checkpoint = session.checkpoint().unwrap();
        session.end_civil_day_update().unwrap();
        session.end_civil_day_update().unwrap();
        assert_eq!(
            checkpoint.state.pending_save_candidates.borrow().len(),
            before.len()
        );
        assert_eq!(
            session.state.pending_save_candidates.borrow().len(),
            before.len() + 2
        );
        session.rollback(checkpoint);
        assert_eq!(
            session
                .state
                .pending_save_candidates
                .borrow()
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            before.keys().copied().collect::<Vec<_>>()
        );
        assert!(Arc::ptr_eq(
            session.state.day_end_save.as_ref().unwrap(),
            &frozen
        ));
        let key = super::super::super::SaveCandidateKey {
            seq: old.seq_to,
            settled_date: old.boundary.settled_date,
        };
        let captured = session.save_candidate(&key).unwrap();
        assert_eq!(captured.snapshot.seq, frozen.snapshot.seq);
        assert_eq!(
            captured.civil_clock.settled_through,
            frozen.civil_clock.settled_through
        );
    }

    #[cfg(feature = "verification-harness")]
    #[test]
    fn verification_checkpoint_restores_authority_without_creating_a_public_save() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let session = ProtocolSession::new(setup, 77).unwrap();
        let checkpoint = session.game().save().unwrap();
        assert!(matches!(
            ProtocolSession::restore(&checkpoint),
            Err(SessionError::InvalidSave(_))
        ));
        let restored = ProtocolSession::restore_verification_checkpoint(&checkpoint).unwrap();
        assert_eq!(
            serde_json::to_vec(&restored.game().save().unwrap()).unwrap(),
            serde_json::to_vec(&checkpoint).unwrap()
        );
        assert!(matches!(restored.save(), Err(SessionError::InvalidSave(_))));
        assert!(matches!(
            restored.save_candidate(&crate::session::protocol::SaveCandidateKey {
                seq: checkpoint.snapshot.seq,
                settled_date: checkpoint.setup.start_date,
            }),
            Err(SessionError::InvalidSave(_))
        ));
    }

    #[test]
    fn day_end_restore_does_not_misclassify_the_next_unsettled_close_as_a_save() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let steps = setup.ticks_per_day;
        let mut session = ProtocolSession::new(setup, 78).unwrap();
        for _ in 0..steps {
            session.step_frame().unwrap();
        }
        session.end_civil_day_update().unwrap();
        for _ in 0..steps {
            session.step_frame().unwrap();
        }
        let raw_current_state = session.game().save().unwrap();
        assert!(matches!(
            ProtocolSession::restore(&raw_current_state),
            Err(SessionError::InvalidSave(_))
        ));
    }

    #[test]
    fn restore_rejects_public_saves_with_intraday_order_state() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 79).unwrap();
        session.end_civil_day_update().unwrap();
        let saved = session.save().unwrap();
        let mut candidate = saved.clone();
        candidate.resting_orders.insert(
            crate::StockCode("600000".into()),
            vec![crate::Order {
                id: crate::OrderId(1),
                side: crate::Side::Buy,
                price: crate::Money::from_cents(1000),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: crate::Money::ZERO,
                owner: crate::AccountId(0),
                seq: 1,
            }],
        );
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));

        let mut candidate = saved.clone();
        candidate.auction_orders.insert(
            crate::StockCode("600000".into()),
            vec![crate::AuctionOrderSnap {
                owner: crate::AccountId(0),
                side: crate::Side::Buy,
                limit: crate::Money::from_cents(1000),
                qty: 100,
                order_id: 1,
            }],
        );
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));

        let mut candidate = saved.clone();
        candidate
            .runtime_v2
            .live_envelopes
            .push(crate::LiveEnvelopeV2 {
                key: crate::EnvelopeKeyV2 {
                    account: crate::AccountId(0),
                    stock: crate::StockCode("600000".into()),
                    order: crate::OrderId(1),
                    side: crate::Side::Buy,
                },
                charged: crate::FeeComponentsV2 {
                    commission: crate::Money::ZERO,
                    stamp_tax: crate::Money::ZERO,
                    transfer_fee: crate::Money::ZERO,
                },
            });
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));

        let mut candidate = saved.clone();
        candidate
            .npc_order_lifecycles
            .push(crate::session::NpcOrderLifecycle {
                account: crate::AccountId(0),
                code: crate::StockCode("600000".into()),
                order_id: crate::OrderId(1),
                placed_market_minute: 0,
                expires_market_minute: 1,
            });
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));

        let intent = crate::Intent::PlaceLimit {
            code: crate::StockCode("600000".into()),
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(crate::Money::from_cents(1000)),
            qty: 100,
        };
        let mut candidate = saved.clone();
        candidate
            .pending_player
            .push((crate::AccountId(0), intent.clone()));
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));

        let mut candidate = saved.clone();
        candidate.pending_npc = Some(crate::session::PendingNpcBatch {
            observed_tick: candidate.snapshot.tick,
            observed_accounts: Vec::new(),
            intents: vec![(crate::AccountId(0), intent)],
            dependencies: Vec::new(),
        });
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));

        let mut empty_pending = saved.clone();
        empty_pending.pending_npc = Some(crate::session::PendingNpcBatch {
            observed_tick: empty_pending.snapshot.tick,
            observed_accounts: Vec::new(),
            intents: Vec::new(),
            dependencies: Vec::new(),
        });
        assert!(ProtocolSession::restore(&empty_pending).is_ok());
    }

    #[test]
    fn restore_rejects_completed_save_with_active_parent_order_plan() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 80).unwrap();
        session.end_civil_day_update().unwrap();
        let mut candidate = session.save().unwrap();
        let code = crate::StockCode("600101".into());
        assert!(candidate.resting_orders.values().all(Vec::is_empty));
        candidate
            .parent_orders
            .entry(crate::AccountId(2))
            .or_default()
            .insert(
                code.clone(),
                crate::session::SaveParentOrderPlan {
                    code,
                    side: crate::Side::Buy,
                    target_qty: 200,
                    filled_qty: 0,
                    child_qty: 100,
                    active_child_order_id: None,
                    linked_plan_id: None,
                    limit_price: crate::Money::from_cents(1000),
                    expires_market_minute: 1000,
                },
            );

        GameSession::restore(&candidate)
            .expect("the low-level snapshot API must retain restorable intraday plans");
        assert!(matches!(
            ProtocolSession::restore(&candidate),
            Err(SessionError::InvalidSave(_))
        ));
    }

    #[test]
    fn day_end_save_candidate_keeps_two_completed_days_until_capture() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-05").unwrap();
        let mut session = ProtocolSession::new(setup, 77).unwrap();
        let first = session.end_civil_day_update().unwrap();
        let second = session.end_civil_day_update().unwrap();
        let key = super::super::super::SaveCandidateKey {
            seq: first.seq_to,
            settled_date: first.boundary.settled_date,
        };
        let captured = session.save_candidate(&key).unwrap();
        assert_eq!(
            captured.snapshot.seq, first.seq_to,
            "an earlier day-end request must not capture a later day"
        );
        assert_ne!(first.seq_to, second.seq_to);
        let wrong_date = super::super::super::SaveCandidateKey {
            seq: second.seq_to,
            settled_date: first.boundary.settled_date,
        };
        assert!(session.save_candidate(&wrong_date).is_err());
        let second_key = super::super::super::SaveCandidateKey {
            seq: second.seq_to,
            settled_date: second.boundary.settled_date,
        };
        assert_eq!(
            session.save_candidate(&second_key).unwrap().snapshot.seq,
            second.seq_to
        );
        assert!(session.save_candidate(&key).is_err());
        session.discard_pending_save_candidates();
        assert_eq!(
            session.save_candidate(&second_key).unwrap().snapshot.seq,
            second.seq_to
        );
    }

    #[test]
    fn published_tick_batch_uses_player_delta_instead_of_full_snapshot() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let mut session = ProtocolSession::new(setup, 76).unwrap();
        let frame = session.step_frame().unwrap();
        let batch = session.tick_batch(vec![frame]).unwrap();
        assert!(batch.runtime_snapshot.is_none());
        let json = serde_json::to_value(batch).unwrap();
        assert!(json["runtime_delta"]["accounts"]
            .as_object()
            .unwrap()
            .contains_key("0"));
        let frame = session.step_frame().unwrap();
        let batch = serde_json::to_value(session.tick_batch(vec![frame]).unwrap()).unwrap();
        assert!(batch["runtime_delta"]["accounts"]
            .as_object()
            .unwrap()
            .is_empty());
    }

    #[test]
    fn player_working_orders_reads_live_auction_without_a_save() {
        use crate::{Money, Side, StockCode};
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 75).unwrap();
        session.state.game.state.auction_orders.insert(
            StockCode("600101".into()),
            vec![
                crate::AuctionOrderSnap {
                    owner: crate::AccountId(0),
                    side: Side::Buy,
                    limit: Money::from_cents(1000),
                    qty: 100,
                    order_id: 81,
                },
                crate::AuctionOrderSnap {
                    owner: crate::AccountId(1),
                    side: Side::Sell,
                    limit: Money::from_cents(1000),
                    qty: 100,
                    order_id: 82,
                },
            ],
        );
        assert!(session.save().is_err());
        let orders = session.player_working_orders();
        assert_eq!(orders.len(), 1);
        assert_eq!(orders[0].id, 81);
        assert_eq!(orders[0].remaining_qty, 100);
        assert_eq!(orders[0].venue, "auction");
    }

    #[test]
    fn day_end_save_is_unavailable_before_first_completed_day() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let session = ProtocolSession::new(setup, 72).unwrap();
        assert!(session.save().is_err());
    }

    #[test]
    fn day_end_save_is_frozen_during_intraday_and_after_failed_day_end() {
        let mut setup = crate::session::protocol::civil::publication_tests::setup();
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        let steps = setup.ticks_per_day;
        let mut session = ProtocolSession::new(setup, 73).unwrap();
        for _ in 0..steps {
            session.step_frame().unwrap();
        }
        session.end_civil_day_update().unwrap();
        let saved = serde_json::to_value(session.save().unwrap()).unwrap();
        for _ in 0..steps {
            session.step_frame().unwrap();
        }
        assert_eq!(
            serde_json::to_value(session.save().unwrap()).unwrap(),
            saved
        );
        session.malformed_civil = true;
        assert!(session.end_civil_day_update().is_err());
        assert_eq!(
            serde_json::to_value(session.save().unwrap()).unwrap(),
            saved
        );
        session.end_civil_day_update().unwrap();
        assert_ne!(
            serde_json::to_value(session.save().unwrap()).unwrap(),
            saved
        );
    }

    #[test]
    fn day_end_save_covers_closed_days_and_restores_candidate() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 74).unwrap();
        session.end_civil_day_update().unwrap();
        let saved = session.save().unwrap();
        let restored = ProtocolSession::restore(&saved).unwrap();
        assert_eq!(
            serde_json::to_value(restored.save().unwrap()).unwrap(),
            serde_json::to_value(saved).unwrap()
        );
    }

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
        let history = serde_json::to_value(&checkpoint.state.intraday).unwrap();

        assert!(
            std::ptr::eq(&session.state.intraday[0], &checkpoint.state.intraday[0]),
            "creating a rollback point must share completed frames, not copy the trading day"
        );
        session.step_frame().unwrap();
        assert!(
            std::ptr::eq(&session.state.intraday[0], &checkpoint.state.intraday[0]),
            "appending a frame must not copy older completed history chunks"
        );
        assert_eq!(
            serde_json::to_value(&checkpoint.state.intraday).unwrap(),
            history
        );
        assert_eq!(
            session.state.intraday.len(),
            checkpoint.state.intraday.len() + 1
        );
    }

    #[test]
    fn outer_rollback_restores_runtime_diagnostics_without_reloading_a_save() {
        let setup = crate::session::protocol::civil::publication_tests::setup();
        let mut session = ProtocolSession::new(setup, 51).unwrap();
        session.state.game.state.last_retail_order_events.push(
            crate::session::RetailOrderDiagnosticEvent::Rejected {
                account: crate::AccountId(1),
                code: crate::StockCode("600101".into()),
                reason: crate::RejectionReason::InsufficientCash,
            },
        );
        let before = session.state.game.session_state_hash().unwrap();
        let diagnostics =
            serde_json::to_value(session.state.game.last_retail_order_events()).unwrap();
        let checkpoint = session.checkpoint().unwrap();
        session.state.game.state.last_retail_order_events.clear();

        session.rollback(checkpoint);

        assert_eq!(session.state.game.session_state_hash().unwrap(), before);
        assert_eq!(
            serde_json::to_value(session.state.game.last_retail_order_events()).unwrap(),
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
        let before = session.state.game.session_state_hash().unwrap();
        let history = serde_json::to_value(&session.state.intraday).unwrap();
        let fact_tick = session.state.fact_cursor.tick;
        let facts = serde_json::to_value(&session.state.fact_cursor.facts).unwrap();
        let checkpoint = session.checkpoint().unwrap();
        let frame = session.step_frame().unwrap();
        let closing = session.end_civil_day_update().unwrap();
        let weekend = session.end_civil_day_update().unwrap();
        assert!(session.state.intraday.is_empty());
        assert_eq!(weekend.civil_date, "2030-01-06");

        session.rollback(checkpoint);

        assert_eq!(session.state.game.session_state_hash().unwrap(), before);
        assert_eq!(
            serde_json::to_value(&session.state.intraday).unwrap(),
            history
        );
        assert_eq!(session.state.fact_cursor.tick, fact_tick);
        assert_eq!(
            serde_json::to_value(&session.state.fact_cursor.facts).unwrap(),
            facts
        );
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
        let before = session.state.game.business_state_hash().unwrap();
        session.state.game.inject_step_failure(fatal.clone());
        assert_eq!(session.step_frame().unwrap_err(), fatal);
        assert_eq!(session.state.game.business_state_hash().unwrap(), before);
        assert_eq!(session.state.game.poison_reason(), None);
        session.step_frame().unwrap().validate().unwrap();

        session.state.game.poison_failed_step(fatal.clone());
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
            seq: session.state.game.seq(),
            settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        session.state.fact_cursor.tick = Some(session.state.game.tick());
        session.state.fact_cursor.facts =
            crate::session::protocol::attach_facts(&[preceding]).unwrap();
        let before = serde_json::to_value(session.state.game.save().unwrap()).unwrap();
        let history = serde_json::to_value(&session.state.intraday).unwrap();
        let fact_tick = session.state.fact_cursor.tick;
        let facts = serde_json::to_value(&session.state.fact_cursor.facts).unwrap();
        session.malformed_frame = true;

        let error = session.step_frame_with_commit_evidence().unwrap_err();

        assert!(matches!(error, StepFatal::InvariantViolation { .. }));
        assert_eq!(
            serde_json::to_value(session.state.game.save().unwrap()).unwrap(),
            before
        );
        assert_eq!(
            serde_json::to_value(&session.state.intraday).unwrap(),
            history
        );
        assert_eq!(session.state.fact_cursor.tick, fact_tick);
        assert_eq!(
            serde_json::to_value(&session.state.fact_cursor.facts).unwrap(),
            facts
        );
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
        let before = serde_json::to_value(session.state.game.save().unwrap()).unwrap();
        let history = serde_json::to_value(&session.state.intraday).unwrap();
        session.malformed_civil = true;

        let error = session.end_civil_day_update().unwrap_err();

        assert!(matches!(error, SessionError::Step(_)));
        assert_eq!(
            serde_json::to_value(session.state.game.save().unwrap()).unwrap(),
            before
        );
        assert_eq!(
            serde_json::to_value(&session.state.intraday).unwrap(),
            history
        );
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
            seq: session.state.game.seq(),
            settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        session.state.fact_cursor.tick = Some(session.state.game.tick());
        session.state.fact_cursor.facts =
            crate::session::protocol::attach_facts(&[preceding]).unwrap();
        let before = serde_json::to_value(session.state.game.save().unwrap()).unwrap();
        let history = serde_json::to_value(&session.state.intraday).unwrap();
        let fact_tick = session.state.fact_cursor.tick;
        let facts = serde_json::to_value(&session.state.fact_cursor.facts).unwrap();
        session.malformed_frame = true;

        let error = session.step_frame().unwrap_err();

        assert!(matches!(error, StepFatal::InvariantViolation { .. }));
        assert_eq!(
            serde_json::to_value(session.state.game.save().unwrap()).unwrap(),
            before
        );
        assert_eq!(
            serde_json::to_value(&session.state.intraday).unwrap(),
            history
        );
        assert_eq!(session.state.fact_cursor.tick, fact_tick);
        assert_eq!(
            serde_json::to_value(&session.state.fact_cursor.facts).unwrap(),
            facts
        );
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
        let saved = uninterrupted.state.game.save().unwrap();
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

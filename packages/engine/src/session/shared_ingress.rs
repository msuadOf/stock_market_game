use super::{GameSession, IngressReceiptCursors, ReceiptBearingIntent, SessionError, StepFatal};
use crate::calendar::{CalendarExchange, CivilDate, DayStatus, TradingCalendar};
use crate::{AccountId, AccountKind, Intent, StockCode};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, MutexGuard, Weak,
};

#[cfg(feature = "verification-harness")]
mod verification;
#[cfg(feature = "verification-harness")]
pub use verification::IngressVerificationSnapshot;

#[derive(Clone)]
pub struct SharedSessionIngress {
    inner: Arc<Mutex<IngressState>>,
    #[cfg(feature = "verification-harness")]
    verification: Arc<verification::IngressVerification>,
}

struct IngressState {
    closed: bool,
    accounts: BTreeMap<AccountId, AccountKind>,
    receipts: IngressReceiptCursors,
    next_input: u64,
    inputs: VecDeque<(u64, ReceiptBearingIntent)>,
    readers: Vec<Weak<AtomicU64>>,
    calendar: TradingCalendar,
    stock_exchanges: BTreeMap<StockCode, CalendarExchange>,
    exchanges: Vec<CalendarExchange>,
    publication: CalendarPublication,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::session) struct CalendarPublication {
    pub(in crate::session) date: CivilDate,
    pub(in crate::session) epoch: u64,
}

pub(super) struct IngressBinding {
    pub(super) source: SharedSessionIngress,
    consumed: Arc<AtomicU64>,
}

impl SharedSessionIngress {
    pub(super) fn register_player(&self, account: AccountId) -> Result<(), SessionError> {
        let mut state = self.lock()?;
        if state.closed || state.accounts.contains_key(&account) {
            return Err(SessionError::InvalidSave("共享收件入口已关闭或新玩家账户身份已存在".into()));
        }
        state.accounts.insert(account, AccountKind::Player);
        Ok(())
    }
    /// 只读内存收件游标，供宿主核对关闭入口后的零新增事实；不提供日内持久化。
    pub fn receipt_cursors(&self) -> Result<IngressReceiptCursors, SessionError> {
        Ok(self.lock()?.receipts.clone())
    }

    /// 当前仍保留在内存 journal 的收件事实；可能含 checkpoint 保留的已消费条目，不是 pending 队列。
    pub fn recorded_player_inputs(&self) -> Result<Vec<ReceiptBearingIntent>, SessionError> {
        Ok(self
            .lock()?
            .inputs
            .iter()
            .map(|(_, receipt)| receipt.clone())
            .collect())
    }
    pub(in crate::session) fn calendar_publication(
        &self,
    ) -> Result<CalendarPublication, SessionError> {
        Ok(self.lock()?.publication)
    }

    pub(in crate::session) fn same_source(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }

    pub(in crate::session) fn publish_calendar(
        &self,
        expected: CalendarPublication,
        date: CivilDate,
    ) -> Result<(), SessionError> {
        let mut state = self.lock()?;
        if state.closed {
            return Err(SessionError::ResourceLimit(
                "已关闭收件入口不能发布自然日".into(),
            ));
        }
        if state.publication != expected || date < expected.date {
            return Err(SessionError::InvalidSave(
                "自然日发布边界已变化，不能回拨或覆盖已发布日期".into(),
            ));
        }
        if date == expected.date {
            return Ok(());
        }
        for exchange in &state.exchanges {
            state.calendar.day_status(*exchange, date)?;
        }
        let epoch = expected
            .epoch
            .checked_add(1)
            .ok_or_else(|| SessionError::ResourceLimit("自然日发布序号耗尽".into()))?;
        state.publication = CalendarPublication { date, epoch };
        Ok(())
    }
    #[cfg(feature = "verification-harness")]
    pub fn verification_arm(
        &self,
        phase: &str,
        tick: u64,
        account: Option<AccountId>,
    ) -> Result<(), SessionError> {
        let state = self.lock()?;
        if state.closed {
            return Err(SessionError::ResourceLimit(
                "verification ingress generation 已关闭".to_owned(),
            ));
        }
        match (phase, account) {
            ("npc_decision", Some(account))
                if state
                    .accounts
                    .get(&account)
                    .is_some_and(|kind| *kind != AccountKind::Player) => {}
            ("cutoff", None) => {}
            _ => {
                return Err(SessionError::ResourceLimit(
                    "verification ingress phase/account 非法".to_owned(),
                ))
            }
        }
        let result = self.verification.arm(verification::VerificationPoint {
            phase: phase.to_owned(),
            tick,
            account,
        });
        drop(state);
        result
    }

    #[cfg(feature = "verification-harness")]
    pub fn verification_snapshot(&self) -> Result<IngressVerificationSnapshot, SessionError> {
        self.verification.snapshot()
    }

    #[cfg(feature = "verification-harness")]
    pub fn verification_release(&self) -> Result<(), SessionError> {
        self.verification.release()
    }

    #[cfg(feature = "verification-harness")]
    pub(in crate::session) fn verification_wait_npc(
        &self,
        tick: u64,
        account: AccountId,
    ) -> Result<(), SessionError> {
        self.verification.wait("npc_decision", tick, Some(account))
    }

    #[cfg(feature = "verification-harness")]
    pub(in crate::session) fn verification_record_npc(
        &self,
        observed_tick: u64,
        receipt: &ReceiptBearingIntent,
    ) -> Result<(), SessionError> {
        self.verification.npc(observed_tick, receipt)
    }
    fn lock(&self) -> Result<MutexGuard<'_, IngressState>, SessionError> {
        self.inner.lock().map_err(|error| {
            SessionError::ResourceLimit(format!("session ingress metadata lock poisoned: {error}"))
        })
    }

    pub fn enqueue_player_intent(
        &self,
        account: AccountId,
        intent: Intent,
    ) -> Result<(), SessionError> {
        let mut state = self.lock()?;
        if state.closed {
            return Err(SessionError::ResourceLimit(
                "session ingress generation is closed".to_owned(),
            ));
        }
        match state.accounts.get(&account) {
            None => return Err(SessionError::UnknownPlayer(account)),
            Some(AccountKind::Player) => {}
            Some(_) => return Err(SessionError::NotPlayer(account)),
        }
        let code = match &intent {
            Intent::PlaceLimit { code, .. }
            | Intent::PlaceMarket { code, .. }
            | Intent::Cancel { code, .. } => code,
        };
        if let Some(exchange) = state.stock_exchanges.get(code) {
            if let DayStatus::Closed(reason) = state
                .calendar
                .day_status(*exchange, state.publication.date)?
            {
                return Err(crate::calendar::CalendarError::NotATradingDay {
                    exchange: *exchange,
                    date: state.publication.date,
                    reason,
                }
                .into());
            }
        }
        let input = state.next_input;
        let next = input.checked_add(1).ok_or_else(|| {
            SessionError::ResourceLimit("session ingress input cursor overflow".to_owned())
        })?;
        let receipt = state.receipts.receive(account, intent)?;
        state.next_input = next;
        state.inputs.push_back((input, receipt));
        #[cfg(feature = "verification-harness")]
        self.verification
            .player(&state.inputs.back().expect("正式收到的输入已登记").1)?;
        Ok(())
    }

    pub fn close(&self) -> Result<(), SessionError> {
        let mut state = self.lock()?;
        state.closed = true;
        #[cfg(feature = "verification-harness")]
        self.verification.release()?;
        Ok(())
    }

    pub(super) fn receive_private(
        &self,
        account: AccountId,
        intent: Intent,
    ) -> Result<ReceiptBearingIntent, SessionError> {
        let mut state = self.lock()?;
        if state.closed {
            return Err(SessionError::ResourceLimit(
                "session ingress generation is closed".to_owned(),
            ));
        }
        state.receipts.receive(account, intent)
    }
}

impl IngressBinding {
    pub(super) fn fork(&self) -> Result<Self, SessionError> {
        let consumed = Arc::new(AtomicU64::new(self.consumed.load(Ordering::Relaxed)));
        self.source.lock()?.readers.push(Arc::downgrade(&consumed));
        Ok(Self {
            source: self.source.clone(),
            consumed,
        })
    }

    fn snapshot(
        &self,
    ) -> Result<(Vec<ReceiptBearingIntent>, IngressReceiptCursors, u64), SessionError> {
        let mut state = self.source.lock()?;
        let mut minimum = state.next_input;
        state.readers.retain(|reader| match reader.upgrade() {
            Some(reader) => {
                minimum = minimum.min(reader.load(Ordering::Relaxed));
                true
            }
            None => false,
        });
        while state
            .inputs
            .front()
            .is_some_and(|(input, _)| *input < minimum)
        {
            state.inputs.pop_front();
        }
        let consumed = self.consumed.load(Ordering::Relaxed);
        let inputs = state
            .inputs
            .iter()
            .filter(|(input, _)| *input >= consumed)
            .map(|(_, receipt)| receipt.clone())
            .collect();
        Ok((inputs, state.receipts.clone(), state.next_input))
    }
}

impl GameSession {
    pub fn shared_ingress(&mut self) -> SharedSessionIngress {
        if let Some(binding) = &self.ingress {
            return binding.source.clone();
        }
        let consumed = Arc::new(AtomicU64::new(0));
        let source = SharedSessionIngress {
            #[cfg(feature = "verification-harness")]
            verification: Arc::new(verification::IngressVerification::default()),
            inner: Arc::new(Mutex::new(IngressState {
                closed: false,
                accounts: self
                    .state
                    .accounts
                    .values()
                    .map(|account| (account.id(), account.kind()))
                    .collect(),
                receipts: self.state.ingress_receipt_cursors.clone(),
                next_input: 0,
                inputs: VecDeque::new(),
                readers: vec![Arc::downgrade(&consumed)],
                calendar: self.state.civil_clock.calendar().clone(),
                stock_exchanges: self
                    .state
                    .setup
                    .stocks
                    .iter()
                    .map(|stock| {
                        (
                            stock.code.clone(),
                            super::session_calendar_exchange(stock.exchange),
                        )
                    })
                    .collect(),
                publication: CalendarPublication {
                    date: self.civil_date(),
                    epoch: 0,
                },
                exchanges: self.state.civil_clock.exchanges().to_vec(),
            })),
        };
        self.ingress = Some(IngressBinding {
            source: source.clone(),
            consumed,
        });
        source
    }

    pub(super) fn freeze_shared_ingress(&mut self) -> Result<(), StepFatal> {
        if let Some(binding) = &self.ingress {
            let (inputs, cursors, cutoff) = binding.snapshot().map_err(ingress_fatal)?;
            #[cfg(feature = "verification-harness")]
            binding
                .source
                .verification
                .cutoff(self.state.tick, cutoff, &inputs)
                .map_err(ingress_fatal)?;
            self.state.pending_player.extend(inputs);
            self.state.ingress_receipt_cursors = cursors;
            binding.consumed.store(cutoff, Ordering::Relaxed);
            #[cfg(feature = "verification-harness")]
            binding
                .source
                .verification
                .wait("cutoff", self.state.tick, None)
                .map_err(ingress_fatal)?;
        }
        Ok(())
    }

    pub(super) fn receive_private_intent(
        &mut self,
        owner: AccountId,
        intent: Intent,
    ) -> Result<ReceiptBearingIntent, SessionError> {
        match &self.ingress {
            Some(binding) => {
                let received = binding.source.receive_private(owner, intent)?;
                self.state
                    .ingress_receipt_cursors
                    .next_account_ordinal
                    .insert(owner, received.account_ordinal + 1);
                let code = match &received.intent {
                    Intent::PlaceLimit { code, .. }
                    | Intent::PlaceMarket { code, .. }
                    | Intent::Cancel { code, .. } => code.clone(),
                };
                self.state
                    .ingress_receipt_cursors
                    .next_stock_ordinal
                    .insert(code, received.stock_ordinal + 1);
                Ok(received)
            }
            None => self.state.ingress_receipt_cursors.receive(owner, intent),
        }
    }

    pub(super) fn project_shared_ingress_save(
        &self,
        save: &mut super::SaveSlot,
    ) -> Result<(), StepFatal> {
        if let Some(binding) = &self.ingress {
            let (inputs, cursors, _) = binding.snapshot().map_err(ingress_fatal)?;
            save.pending_player.extend(inputs);
            save.ingress_receipt_cursors = cursors;
        }
        Ok(())
    }
}

pub(super) fn ingress_fatal(error: SessionError) -> StepFatal {
    StepFatal::InvariantViolation {
        location: "session::shared_ingress".to_owned(),
        description: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{LimitPrice, Money, Side};

    fn game() -> GameSession {
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        setup.npcs.inst_count = 0;
        let mut second = setup.stocks[0].clone();
        second.code = crate::StockCode("600889".to_owned());
        setup.stocks.push(second);
        setup.company_system = simple_company_fixture!(crate; codes = setup.stocks.iter().map(|stock| stock.code.0.as_str()));
        GameSession::new(setup, 42).unwrap()
    }

    fn buy(game: &GameSession, stock: usize) -> Intent {
        Intent::PlaceLimit {
            code: game.state.setup.stocks[stock].code.clone(),
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(990)),
            qty: 100,
        }
    }

    #[test]
    fn concurrent_external_receive_precedes_private_npc_completion() {
        let mut game = game();
        let source = game.shared_ingress();
        let player_intent = buy(&game, 0);
        let npc_intent = buy(&game, 0);
        let mut candidate = game.clone_for_tick_shadow().unwrap();
        let (completed_tx, completed_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            scope.spawn(move || {
                source
                    .enqueue_player_intent(AccountId(0), player_intent)
                    .unwrap();
                completed_tx.send(()).unwrap();
            });
            completed_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .unwrap();
            let npc = candidate
                .receive_private_intent(AccountId(0), npc_intent)
                .unwrap();
            let player = game
                .ingress
                .as_ref()
                .unwrap()
                .snapshot()
                .unwrap()
                .0
                .remove(0);
            assert!(player.account_ordinal < npc.account_ordinal);
            assert!(player.stock_ordinal < npc.stock_ordinal);
        });
    }

    #[test]
    fn cutoff_keeps_late_external_input_for_next_tick() {
        let mut game = game();
        let source = game.shared_ingress();
        source
            .enqueue_player_intent(AccountId(0), buy(&game, 0))
            .unwrap();
        let mut candidate = game.clone_for_tick_shadow().unwrap();
        candidate.freeze_shared_ingress().unwrap();
        source
            .enqueue_player_intent(AccountId(0), buy(&game, 1))
            .unwrap();
        assert_eq!(candidate.state.pending_player.len(), 1);
        game.commit_tick_shadow(candidate);
        let mut next = game.clone_for_tick_shadow().unwrap();
        next.freeze_shared_ingress().unwrap();
        assert_eq!(next.state.pending_player.len(), 2);
        assert_eq!(next.state.pending_player[1].account_ordinal, 1);
        assert_eq!(source.lock().unwrap().inputs.len(), 1);
    }

    #[test]
    fn discarded_candidate_does_not_lose_external_inputs_or_publish_private_batch() {
        let mut game = game();
        let source = game.shared_ingress();
        source
            .enqueue_player_intent(AccountId(0), buy(&game, 0))
            .unwrap();
        let before = game.business_state_hash().unwrap();
        let mut failed = game.clone_for_tick_shadow().unwrap();
        failed.freeze_shared_ingress().unwrap();
        failed
            .receive_private_intent(AccountId(0), buy(&game, 0))
            .unwrap();
        drop(failed);
        assert_eq!(game.business_state_hash().unwrap(), before);
        let mut retry = game.clone_for_tick_shadow().unwrap();
        retry.freeze_shared_ingress().unwrap();
        assert_eq!(retry.state.pending_player.len(), 1);
        assert!(retry.state.pending_npc.as_ref().unwrap().intents.is_empty());
        assert_eq!(retry.state.pending_player[0].account_ordinal, 0);
    }

    #[test]
    fn checkpoint_reader_retains_consumed_inputs_until_rollback_finishes() {
        let mut game = game();
        let source = game.shared_ingress();
        let checkpoint = game.clone_for_tick_shadow().unwrap();
        source
            .enqueue_player_intent(AccountId(0), buy(&game, 0))
            .unwrap();
        let mut candidate = game.clone_for_tick_shadow().unwrap();
        candidate.freeze_shared_ingress().unwrap();
        candidate.state.pending_player.clear();
        game.commit_tick_shadow(candidate);
        source
            .enqueue_player_intent(AccountId(0), buy(&game, 1))
            .unwrap();
        assert_eq!(
            game.ingress.as_ref().unwrap().snapshot().unwrap().0.len(),
            1
        );
        assert_eq!(source.lock().unwrap().inputs.len(), 2);
        game = checkpoint;
        game.freeze_shared_ingress().unwrap();
        assert_eq!(game.state.pending_player.len(), 2);
        assert_eq!(game.state.pending_player[0].account_ordinal, 0);
        assert_eq!(game.state.pending_player[1].account_ordinal, 1);
    }

    #[test]
    fn closed_generation_and_non_player_are_rejected_before_receipt_allocation() {
        let mut game =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        let source = game.shared_ingress();
        let cursors = serde_json::to_value(&source.lock().unwrap().receipts).unwrap();
        assert!(matches!(
            source.enqueue_player_intent(AccountId(99), buy(&game, 0)),
            Err(SessionError::UnknownPlayer(_))
        ));
        assert!(matches!(
            source.enqueue_player_intent(AccountId(1), buy(&game, 0)),
            Err(SessionError::NotPlayer(_))
        ));
        source.close().unwrap();
        assert!(source
            .enqueue_player_intent(AccountId(0), buy(&game, 0))
            .is_err());
        assert_eq!(
            serde_json::to_value(&source.lock().unwrap().receipts).unwrap(),
            cursors
        );
        assert!(source.lock().unwrap().inputs.is_empty());
    }

    #[test]
    fn private_plan_receipt_uses_the_same_source_as_late_player_and_npc() {
        let mut game = game();
        let source = game.shared_ingress();
        let mut candidate = game.clone_for_tick_shadow().unwrap();
        candidate.freeze_shared_ingress().unwrap();
        source
            .enqueue_player_intent(AccountId(0), buy(&game, 0))
            .unwrap();
        let plan = candidate
            .receive_private_intent(AccountId(0), buy(&game, 0))
            .unwrap();
        let npc = candidate
            .receive_private_intent(AccountId(0), buy(&game, 0))
            .unwrap();
        assert_eq!(plan.account_ordinal, 1);
        assert_eq!(plan.stock_ordinal, 1);
        assert_eq!(npc.account_ordinal, 2);
        assert_eq!(npc.stock_ordinal, 2);
        assert_eq!(
            candidate
                .save()
                .unwrap()
                .ingress_receipt_cursors
                .next_account_ordinal[&AccountId(0)],
            3
        );
    }

    #[test]
    fn production_step_cash_winner_follows_cross_source_receive_order() {
        for player_first in [true, false] {
            let mut game = game();
            game.state
                .accounts
                .get_mut(&AccountId(0))
                .unwrap()
                .fixture_set_cash(Money::from_cents(100_000));
            let source = game.shared_ingress();
            let player = buy(&game, 0);
            let npc = buy(&game, 1);
            let receipt = if player_first {
                source.enqueue_player_intent(AccountId(0), player).unwrap();
                game.receive_private_intent(AccountId(0), npc).unwrap()
            } else {
                let receipt = game.receive_private_intent(AccountId(0), npc).unwrap();
                source.enqueue_player_intent(AccountId(0), player).unwrap();
                receipt
            };
            game.state.pending_npc = Some(super::super::PendingNpcBatch {
                observed_tick: game.tick(),
                observed_accounts: Vec::new(),
                intents: vec![receipt],
                dependencies: Vec::new(),
            });
            let events = game.step().unwrap();
            let accepted = events
                .iter()
                .filter_map(|event| match event {
                    crate::Event::OrderAccepted { account, code, .. }
                        if *account == AccountId(0) =>
                    {
                        Some(code)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                accepted,
                vec![&game.state.setup.stocks[usize::from(!player_first)].code]
            );
            assert_eq!(events.iter().filter(|event| matches!(event, crate::Event::IntentRejected { account, reason: crate::RejectionReason::InsufficientCash, .. } if *account == AccountId(0))).count(), 1);
        }
    }

    #[test]
    fn production_step_same_price_stock_order_follows_cross_source_receipts() {
        for player_first in [true, false] {
            let mut game = game();
            let source = game.shared_ingress();
            let player = buy(&game, 0);
            let mut npc = buy(&game, 0);
            if let Intent::PlaceLimit { qty, .. } = &mut npc {
                *qty = 200;
            }
            let received = if player_first {
                source.enqueue_player_intent(AccountId(0), player).unwrap();
                game.receive_private_intent(AccountId(0), npc).unwrap()
            } else {
                let received = game.receive_private_intent(AccountId(0), npc).unwrap();
                source.enqueue_player_intent(AccountId(0), player).unwrap();
                received
            };
            game.state.pending_npc = Some(super::super::PendingNpcBatch {
                observed_tick: game.tick(),
                observed_accounts: Vec::new(),
                intents: vec![received],
                dependencies: Vec::new(),
            });
            let events = game.step().unwrap();
            let accepted = events
                .iter()
                .filter_map(|event| match event {
                    crate::Event::OrderAccepted {
                        account,
                        remaining_qty,
                        ..
                    } if *account == AccountId(0) => Some(*remaining_qty),
                    _ => None,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                accepted,
                if player_first {
                    vec![100, 200]
                } else {
                    vec![200, 100]
                }
            );
        }
    }

    #[test]
    fn failed_private_receipt_gap_restores_without_losing_real_player_inputs() {
        let mut game = game();
        let source = game.shared_ingress();
        source
            .enqueue_player_intent(AccountId(0), buy(&game, 0))
            .unwrap();
        let mut discarded = game.clone_for_tick_shadow().unwrap();
        discarded.freeze_shared_ingress().unwrap();
        let receipt = discarded
            .receive_private_intent(AccountId(0), buy(&game, 0))
            .unwrap();
        discarded.state.pending_npc = Some(super::super::PendingNpcBatch {
            observed_tick: discarded.tick(),
            observed_accounts: Vec::new(),
            intents: vec![receipt],
            dependencies: Vec::new(),
        });
        drop(discarded);
        source
            .enqueue_player_intent(AccountId(0), buy(&game, 0))
            .unwrap();
        let save = game.save().unwrap();
        assert_eq!(
            save.pending_player
                .iter()
                .map(|receipt| receipt.account_ordinal)
                .collect::<Vec<_>>(),
            vec![0, 2]
        );
        assert!(save.pending_npc.as_ref().unwrap().intents.is_empty());
        assert_eq!(
            save.ingress_receipt_cursors.next_account_ordinal[&AccountId(0)],
            3
        );
        let mut restored = GameSession::restore(&save).unwrap();
        assert_eq!(restored.step().unwrap().iter().filter(|event| matches!(event, crate::Event::OrderAccepted { account, .. } if *account == AccountId(0))).count(), 2);
    }

    #[cfg(feature = "verification-harness")]
    struct ReleaseGate(SharedSessionIngress);

    #[cfg(feature = "verification-harness")]
    impl Drop for ReleaseGate {
        fn drop(&mut self) {
            self.0
                .verification_release()
                .expect("验证退出必须释放 gate");
        }
    }

    #[cfg(feature = "verification-harness")]
    fn wait_for_gate(
        source: &SharedSessionIngress,
        finished: &std::sync::mpsc::Receiver<()>,
    ) -> bool {
        for _ in 0..1000 {
            if source.verification_snapshot().unwrap().entered {
                return true;
            }
            if finished.try_recv().is_ok() {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        false
    }

    #[cfg(feature = "verification-harness")]
    #[test]
    fn verification_npc_gate_blocks_real_step_without_blocking_player_receipt() {
        let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
        setup.ticks_per_day = 4;
        setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
        setup.strategy_params.retail.arrival_rate = 1.0;
        let mut session = GameSession::new(setup, 1).unwrap();
        let account = AccountId(1);
        let attention = session.state.npc_attention.get_mut(&account).unwrap();
        attention.next_attention_candidate_tick = 1;
        attention.rng_state = 3;
        session.state.attention_scheduler.enqueue(1, account);
        let source = session.shared_ingress();
        source
            .verification_arm("npc_decision", 1, Some(account))
            .unwrap();
        let player_intent = buy(&session, 0);
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let _release = ReleaseGate(source.clone());
            let worker = scope.spawn(|| {
                let result = session.step();
                finished_tx.send(()).unwrap();
                result
            });
            let entered = wait_for_gate(&source, &finished_rx);
            if !entered {
                source.verification_release().unwrap();
                worker
                    .join()
                    .unwrap()
                    .expect("production step 必须成功，不能用其他 fatal 冒充缺失 gate 的红灯");
                panic!("真实 production NPC worker 必须进入已配置的 gate");
            }
            source
                .enqueue_player_intent(AccountId(0), player_intent)
                .unwrap();
            let trace = source.verification_snapshot().unwrap();
            assert!(trace.entered && !trace.released);
            assert_eq!(trace.players.len(), 1);
            assert!(trace.npc_receipts.is_empty());
            source.verification_release().unwrap();
            worker.join().unwrap().unwrap();
        });
        let trace = source.verification_snapshot().unwrap();
        assert!(trace
            .npc_receipts
            .iter()
            .any(|receipt| receipt.receipt.owner == account));
        for npc in &trace.npc_receipts {
            assert!(trace.players[0].stock_ordinal < npc.receipt.stock_ordinal);
        }
        assert_eq!(session.save().unwrap().pending_player.len(), 1);
    }

    #[cfg(feature = "verification-harness")]
    #[test]
    fn verification_cutoff_gate_exposes_real_frozen_prefix_and_defers_late_player() {
        let mut session = game();
        let source = session.shared_ingress();
        source.verification_arm("cutoff", 0, None).unwrap();
        source
            .enqueue_player_intent(AccountId(0), buy(&session, 0))
            .unwrap();
        let mut late = buy(&session, 0);
        if let Intent::PlaceLimit { qty, .. } = &mut late {
            *qty = 200;
        }
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let first = std::thread::scope(|scope| {
            let _release = ReleaseGate(source.clone());
            let worker = scope.spawn(|| {
                let result = session.step();
                finished_tx.send(()).unwrap();
                result
            });
            if !wait_for_gate(&source, &finished_rx) {
                source.verification_release().unwrap();
                worker
                    .join()
                    .unwrap()
                    .expect("production step 必须成功，不能用其他 fatal 冒充缺失 gate 的红灯");
                panic!("production cutoff 已冻结后必须进入 gate");
            }
            source.enqueue_player_intent(AccountId(0), late).unwrap();
            let trace = source.verification_snapshot().unwrap();
            assert_eq!(trace.cutoffs.len(), 1);
            assert_eq!(trace.cutoffs[0].receipts.len(), 1);
            assert_eq!(
                trace.cutoffs[0].receipts[0].account_ordinal,
                trace.players[0].account_ordinal
            );
            assert!(
                trace.players[1].account_ordinal > trace.cutoffs[0].receipts[0].account_ordinal
            );
            source.verification_release().unwrap();
            worker.join().unwrap().unwrap()
        });
        let accepted = |events: &[crate::Event]| {
            events
                .iter()
                .filter_map(|event| match event {
                    crate::Event::OrderAccepted {
                        account,
                        remaining_qty,
                        ..
                    } if *account == AccountId(0) => Some(*remaining_qty),
                    _ => None,
                })
                .collect::<Vec<_>>()
        };
        assert_eq!(accepted(&first), vec![100]);
        assert_eq!(accepted(&session.step().unwrap()), vec![200]);
        let trace = source.verification_snapshot().unwrap();
        assert_eq!(trace.cutoffs.len(), 2);
        assert_eq!(
            trace.cutoffs[1].receipts[0].account_ordinal,
            trace.players[1].account_ordinal
        );
        assert_eq!(session.save().unwrap().pending_player.len(), 0);
    }

    #[cfg(feature = "verification-harness")]
    #[test]
    fn closing_source_releases_entered_gate_and_rejects_rearming() {
        let mut session = game();
        let source = session.shared_ingress();
        source.verification_arm("cutoff", 0, None).unwrap();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        std::thread::scope(|scope| {
            let _release = ReleaseGate(source.clone());
            let worker = scope.spawn(|| {
                source.verification.wait("cutoff", 0, None).unwrap();
                finished_tx.send(()).unwrap();
            });
            assert!(wait_for_gate(&source, &finished_rx));
            source.close().unwrap();
            finished_rx
                .recv_timeout(std::time::Duration::from_secs(1))
                .expect("close 必须唤醒真实 entered waiter");
            assert!(source.verification_snapshot().unwrap().released);
            assert!(source.verification_arm("cutoff", 1, None).is_err());
            worker.join().unwrap();
        });
    }
}

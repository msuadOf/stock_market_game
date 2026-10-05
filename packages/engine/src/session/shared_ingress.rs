use super::{GameSession, IngressReceiptCursors, ReceiptBearingIntent, SessionError, StepFatal};
use crate::{AccountId, AccountKind, Intent};
use std::collections::{BTreeMap, VecDeque};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex, MutexGuard, Weak,
};

#[derive(Clone)]
pub struct SharedSessionIngress {
    inner: Arc<Mutex<IngressState>>,
}

struct IngressState {
    closed: bool,
    accounts: BTreeMap<AccountId, AccountKind>,
    receipts: IngressReceiptCursors,
    next_input: u64,
    inputs: VecDeque<(u64, ReceiptBearingIntent)>,
    readers: Vec<Weak<AtomicU64>>,
}

pub(super) struct IngressBinding {
    pub(super) source: SharedSessionIngress,
    consumed: Arc<AtomicU64>,
}

impl SharedSessionIngress {
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
        let input = state.next_input;
        let next = input.checked_add(1).ok_or_else(|| {
            SessionError::ResourceLimit("session ingress input cursor overflow".to_owned())
        })?;
        let receipt = state.receipts.receive(account, intent)?;
        state.next_input = next;
        state.inputs.push_back((input, receipt));
        Ok(())
    }

    pub fn close(&self) -> Result<(), SessionError> {
        self.lock()?.closed = true;
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
            self.state.pending_player.extend(inputs);
            self.state.ingress_receipt_cursors = cursors;
            binding.consumed.store(cutoff, Ordering::Relaxed);
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
        setup.npcs.inst_count = 0;
        let mut second = setup.stocks[0].clone();
        second.code = crate::StockCode("600889".to_owned());
        setup.stocks.push(second);
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
}

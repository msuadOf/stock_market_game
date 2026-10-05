use super::*;
use crate::session::pipeline::local_admission::{admit_ready_batch, AccountReceipt};
use crate::session::pipeline::{IntentCandidateKey, TickWorkReady};
use crate::{AccountId, Intent, Money, Side, StockCode};
use std::time::Duration;

#[test]
fn real_root_notifies_before_stock_stream_starts_with_one_or_several_workers() {
    for workers in [1, 3] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        let mut session =
            GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
        let notifications = pool.install(|| {
            let sources = ReadyIngressSources {
                initial: Vec::new(),
                observed_accounts: vec![AccountId(1)],
            };
            let mut ingress = sources.capture_roots(&session, None).unwrap();
            ingress.first_ready_batch(&mut session).unwrap();
            let (mut chain, notifications, _) = ingress.into_parts();
            // Consume any outstanding root without starting a stock driver. In
            // the one-worker pool this also exercises cooperative root execution.
            chain.next_ready_batch(&mut session).unwrap();
            notifications
        });
        assert!(matches!(
            notifications.receiver.recv_timeout(Duration::from_secs(2)),
            Ok(TickWorkReady::PlanRoot)
        ));
        assert!(matches!(
            notifications.receiver.try_recv(),
            Err(std::sync::mpsc::TryRecvError::Empty)
        ));
    }
}

#[test]
fn account_receipts_follow_request_time_and_plan_readiness_not_key_order() {
    let owner = AccountId(1);
    let place = |stock: &str| Intent::PlaceLimit {
        code: StockCode(stock.to_owned()),
        side: Side::Buy,
        price: crate::LimitPrice::Fixed(Money::from_cents(100)),
        qty: 100,
    };
    let ready = vec![
        IntentCandidate::new(IntentCandidateKey::plan_chain(99), owner, place("600002")),
        IntentCandidate::new(IntentCandidateKey::npc(owner, 0), owner, place("600001"))
            .with_ingress_order(0, 0),
        IntentCandidate::new(IntentCandidateKey::plan_chain(2), owner, place("600003")),
    ];
    let mut receipts = AccountReceipts::default();
    let admitted = admit_ready_batch(ready, &mut receipts).unwrap();
    assert_eq!(
        admitted
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        vec![
            IntentCandidateKey::npc(owner, 0),
            IntentCandidateKey::plan_chain(99),
            IntentCandidateKey::plan_chain(2),
        ]
    );
    let following = receipts
        .observe(&IntentCandidate::new(
            IntentCandidateKey::plan_chain(1),
            owner,
            place("600004"),
        ))
        .unwrap();
    assert_eq!(following, AccountReceipt::ReadyThisTick(2));
}

#[test]
fn same_stock_fifo_uses_receipt_time_across_player_and_npc_queues() {
    for workers in [1, 3] {
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(workers)
            .build()
            .unwrap();
        let admitted = pool.install(|| {
            let mut session =
                GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42)
                    .unwrap();
            let code = session.state.setup.stocks[0].code.clone();
            session.state.pending_npc = None;
            session
                .enqueue_player_intent(
                    AccountId(0),
                    Intent::PlaceLimit {
                        code: code.clone(),
                        side: Side::Buy,
                        price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                        qty: 100,
                    },
                )
                .unwrap();
            let received_npc = session
                .state
                .ingress_receipt_cursors
                .receive(
                    AccountId(1),
                    Intent::PlaceLimit {
                        code: code.clone(),
                        side: Side::Buy,
                        price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                        qty: 100,
                    },
                )
                .unwrap();
            session.state.pending_npc = Some(crate::session::PendingNpcBatch {
                observed_tick: session.state.tick,
                observed_accounts: vec![AccountId(1)],
                intents: vec![received_npc],
                dependencies: Vec::new(),
            });

            let sources = ReadyIngress::capture_sources(&mut session).unwrap();
            admit_ready_batch(sources.initial, &mut AccountReceipts::default()).unwrap()
        });
        assert!(matches!(
            admitted.as_slice(),
            [first, second]
                if matches!(first.key(), IntentCandidateKey::Player { .. })
                    && matches!(second.key(), IntentCandidateKey::Npc { .. })
        ));
    }
}

#[test]
fn same_account_cash_lane_uses_cross_source_receive_time() {
    let mut session =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 45).unwrap();
    let first_stock = session.state.setup.stocks[0].code.clone();
    let second_stock = session.state.setup.stocks[1].code.clone();
    session
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: first_stock,
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                qty: 100,
            },
        )
        .unwrap();
    let npc = session
        .state
        .ingress_receipt_cursors
        .receive(
            AccountId(0),
            Intent::PlaceLimit {
                code: second_stock,
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(990)),
                qty: 100,
            },
        )
        .unwrap();
    session.state.pending_npc = Some(crate::session::PendingNpcBatch {
        observed_tick: session.state.tick,
        observed_accounts: vec![AccountId(0)],
        intents: vec![npc],
        dependencies: Vec::new(),
    });

    let sources = ReadyIngress::capture_sources(&mut session).unwrap();
    let admitted = admit_ready_batch(sources.initial, &mut AccountReceipts::default()).unwrap();

    assert!(matches!(
        admitted.as_slice(),
        [first, second]
            if matches!(first.key(), IntentCandidateKey::Player { .. })
                && matches!(second.key(), IntentCandidateKey::Npc { .. })
    ));
}

#[test]
fn enqueue_after_tick_capture_is_deferred_to_the_next_capture() {
    let mut session =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 42).unwrap();
    let code = session.state.setup.stocks[0].code.clone();
    let intent = || Intent::PlaceLimit {
        code: code.clone(),
        side: Side::Buy,
        price: crate::LimitPrice::Fixed(Money::from_cents(990)),
        qty: 100,
    };

    session
        .enqueue_player_intent(AccountId(0), intent())
        .unwrap();
    let frozen = ReadyIngress::capture_sources(&mut session).unwrap();
    assert!(matches!(
        frozen.initial.as_slice(),
        [candidate] if matches!(candidate.key(), IntentCandidateKey::Player { .. })
    ));
    session
        .enqueue_player_intent(AccountId(0), intent())
        .unwrap();
    assert_eq!(session.state.pending_player.len(), 1);
}

#[test]
fn failed_tick_keeps_external_queue_for_the_next_committed_attempt() {
    let mut session =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 43).unwrap();
    let code = session.state.setup.stocks[0].code.clone();
    let intent = Intent::PlaceLimit {
        code,
        side: Side::Buy,
        price: crate::LimitPrice::Fixed(Money::from_cents(990)),
        qty: 100,
    };
    session
        .enqueue_player_intent(AccountId(0), intent.clone())
        .unwrap();
    let before = session.business_state_hash().unwrap();
    let fatal = crate::session::StepFatal::InvariantViolation {
        description: "受理后、提交前注入失败".to_owned(),
        location: "ready_ingress_tests".to_owned(),
    };
    session.inject_post_shadow_failure(fatal.clone());

    assert_eq!(session.step().unwrap_err(), fatal);

    assert_eq!(session.state.pending_player.len(), 1);
    let queued = &session.state.pending_player[0];
    assert_eq!(queued.owner, AccountId(0));
    assert_eq!(
        serde_json::to_value(&queued.intent).unwrap(),
        serde_json::to_value(intent).unwrap()
    );
    assert_eq!(session.business_state_hash().unwrap(), before);
}

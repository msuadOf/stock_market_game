use super::*;
use crate::session::pipeline::local_admission::{admit_ready_batch, AccountReceipt};
use crate::session::pipeline::{P2CandidateKey, TickWorkReady};
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
        price: Money::from_cents(100),
        qty: 100,
    };
    let ready = vec![
        P2Candidate::new(P2CandidateKey::plan_chain(99), owner, place("600002")),
        P2Candidate::new(P2CandidateKey::npc(owner, 0), owner, place("600001")),
        P2Candidate::new(P2CandidateKey::plan_chain(2), owner, place("600003")),
    ];
    let mut receipts = AccountReceipts::default();
    let admitted = admit_ready_batch(ready, &mut receipts).unwrap();
    assert_eq!(
        admitted
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        vec![
            P2CandidateKey::npc(owner, 0),
            P2CandidateKey::plan_chain(99),
            P2CandidateKey::plan_chain(2),
        ]
    );
    let following = receipts
        .observe(&P2Candidate::new(
            P2CandidateKey::plan_chain(1),
            owner,
            place("600004"),
        ))
        .unwrap();
    assert_eq!(following, AccountReceipt::ReadyThisTick(2));
}

use super::*;
use crate::session::pipeline::TickWorkReady;
use crate::AccountId;
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
            let (mut chain, notifications) = ingress.into_parts();
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

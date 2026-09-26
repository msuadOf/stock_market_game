use super::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

fn cancel(code: &str, id: u64) -> P3ValidatedOperation {
    P3ValidatedOperation::Cancel {
        candidate_key: super::super::P2CandidateKey::player(id),
        sealed_index: id,
        account: crate::AccountId(id),
        code: StockCode(code.to_owned()),
        order_id: crate::OrderId(id),
    }
}

enum WaitingShard {
    Independent {
        started: mpsc::Sender<()>,
    },
    Unrelated {
        started: mpsc::Sender<()>,
        independent_started: mpsc::Receiver<()>,
    },
}

impl StockShard for WaitingShard {
    type Round = bool;
    const DISPATCH_BOUNDARY: Option<ExecutorBoundary> = None;

    fn apply(&mut self, _: Vec<P3ValidatedOperation>) -> Result<bool, StepFatal> {
        match self {
            Self::Independent { started } => {
                started.send(()).unwrap();
                Ok(true)
            }
            Self::Unrelated {
                started,
                independent_started,
            } => {
                started.send(()).unwrap();
                // A deadline makes the old unnecessary wait fail promptly instead
                // of leaving a test worker permanently blocked.
                Ok(independent_started
                    .recv_timeout(Duration::from_secs(2))
                    .is_ok())
            }
        }
    }
}

#[test]
fn completed_root_enters_idle_stock_before_unrelated_stock_finishes() {
    let (b_started, wait_for_b) = mpsc::channel();
    let (a_started, wait_for_a) = mpsc::channel();
    let root_ready = Arc::new(AtomicBool::new(false));
    let notifications = StockStreamNotifications::new();
    let root_completed = notifications.sender.clone();
    let shards = BTreeMap::from([
        (
            StockCode("A".to_owned()),
            WaitingShard::Independent { started: a_started },
        ),
        (
            StockCode("B".to_owned()),
            WaitingShard::Unrelated {
                started: b_started,
                independent_started: wait_for_a,
            },
        ),
    ]);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(3)
        .build()
        .unwrap();
    let mut independent_started_before_b_finished = false;
    std::thread::scope(|threads| {
        let worker_root_ready = Arc::clone(&root_ready);
        threads.spawn(move || {
            wait_for_b.recv_timeout(Duration::from_secs(2)).unwrap();
            worker_root_ready.store(true, Ordering::Release);
            root_completed.send(TickWorkReady::PlanRoot).unwrap();
        });
        pool.install(|| {
            drive_stock_stream(
                shards,
                vec![cancel("B", 1)],
                notifications,
                |_| Err(invariant("fixture must not create a detached book")),
                |progress| {
                    if let StockStreamProgress::StockCompleted { code, round } = progress {
                        if code.0 == "B" {
                            independent_started_before_b_finished = *round;
                        }
                    }
                    if root_ready.swap(false, Ordering::AcqRel) {
                        Ok(vec![cancel("A", 2)])
                    } else {
                        Ok(Vec::new())
                    }
                },
            )
            .unwrap();
        });
    });
    assert!(
        independent_started_before_b_finished,
        "a completed independent plan root must start stock A while unrelated stock B is still running"
    );
}

#[test]
fn already_consumed_root_notification_keeps_waiting_for_stock_completion() {
    for workers in [1, 2] {
        consumed_notification_fixture(workers);
    }
}

fn consumed_notification_fixture(workers: usize) {
    let (b_started, _wait_for_b) = mpsc::channel();
    let (release_b, wait_for_release) = mpsc::channel();
    let notifications = StockStreamNotifications::new();
    // The result may have been consumed during first_ready_batch or P3. The
    // early notification remains queued when the stock stream starts.
    notifications.sender.send(TickWorkReady::PlanRoot).unwrap();
    let mut root_notifications = 0;
    let mut completed_stock = false;
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(workers)
        .build()
        .unwrap();
    pool.install(|| {
        drive_stock_stream(
            BTreeMap::from([(
                StockCode("B".to_owned()),
                WaitingShard::Unrelated {
                    started: b_started,
                    independent_started: wait_for_release,
                },
            )]),
            vec![cancel("B", 1)],
            notifications,
            |_| Err(invariant("fixture must not create a detached book")),
            |progress| {
                match progress {
                    StockStreamProgress::PlanRootReady => {
                        root_notifications += 1;
                        release_b.send(()).unwrap();
                    }
                    StockStreamProgress::StockCompleted { code, round } => {
                        assert_eq!(code.0, "B");
                        assert!(
                            *round,
                            "stock must continue after an empty root notification"
                        );
                        completed_stock = true;
                    }
                    StockStreamProgress::Idle => {
                        assert!(
                            completed_stock,
                            "empty root notification must not finish the stream"
                        );
                    }
                }
                Ok(Vec::new())
            },
        )
        .unwrap();
    });
    assert_eq!(root_notifications, 1);
    assert!(completed_stock);
}

use super::*;
use std::time::Duration;
use tokio::sync::mpsc;

#[tokio::test(start_paused = true)]
async fn deadline_uses_the_same_monotonic_timeline_as_now() {
    let (_tx, mut rx) = mpsc::unbounded_channel::<u32>();
    let deadline = now() + Duration::from_millis(20);
    assert_eq!(
        wait(&mut rx, WaitPlan::At(deadline)).await,
        Some(Wake::Deadline)
    );
    assert!(now() >= deadline);
}

#[tokio::test(start_paused = true)]
async fn queued_command_beats_expired_deadline_and_yield() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tx.send(7).unwrap();
    tx.send(9).unwrap();
    assert_eq!(
        wait(&mut rx, WaitPlan::At(now())).await,
        Some(Wake::Command(7))
    );
    assert_eq!(wait(&mut rx, WaitPlan::Yield).await, Some(Wake::Command(9)));
    assert_eq!(wait(&mut rx, WaitPlan::Yield).await, Some(Wake::Deadline));
}

#[tokio::test(start_paused = true)]
async fn closed_receiver_preserves_queued_command_and_then_exits() {
    let (tx, mut rx) = mpsc::unbounded_channel();
    tx.send(4).unwrap();
    drop(tx);
    assert_eq!(wait(&mut rx, WaitPlan::Idle).await, Some(Wake::Command(4)));
    assert_eq!(wait(&mut rx, WaitPlan::Yield).await, None);
}

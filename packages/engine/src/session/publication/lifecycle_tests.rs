use super::lifecycle::*;

#[test]
fn same_generation_new_lease_rejects_old_release_ack_and_cut() {
    let mut state = ObserverLifecycle::default();
    let old = state.hold(7);
    assert!(state.fence(old, 4));
    let new = state.hold(7);
    assert!(!state.release(old));
    assert!(!state.fence(old, 5));
    assert!(!state.is_current(old));
    assert!(state.fence(new, 4));
    assert!(state.release(new));
    assert!(state.is_current(new));
}

#[test]
fn transport_confirmation_does_not_advance_application_cursor() {
    let mut state = ObserverLifecycle::default();
    let lease = state.hold(1);
    assert!(state.fence(lease, 0));
    assert!(state.release(lease));
    assert!(state.sent(lease, 8));
    assert!(state.flushed(lease, 8));
    assert_eq!(state.evidence().accepted, 0);
    assert!(!state.accepted(lease, 9));
    assert!(state.accepted(lease, 8));
    assert_eq!(state.evidence().accepted, 8);
}

#[test]
fn terminal_is_once_and_can_report_held_prefix_gap() {
    let mut state = TerminalDelivery::default();
    assert_eq!(
        state.take(super::TerminalReadiness::Pending, false, 2, 4),
        None
    );
    assert_eq!(
        state.take(super::TerminalReadiness::Pending, true, 2, 4),
        Some(Some((3, 4)))
    );
    assert_eq!(
        state.take(super::TerminalReadiness::Ready, false, 4, 4),
        None
    );
}

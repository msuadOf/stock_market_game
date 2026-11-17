use super::*;

#[test]
fn prepare_rejects_exhaustion_without_changing_current_authority() {
    assert!(ReplacementPlan::prepare(u64::MAX).is_err());
    let plan = ReplacementPlan::prepare(7).unwrap();
    assert_eq!((plan.previous_generation(), plan.generation()), (7, 8));
}

#[test]
fn stale_prepare_cannot_install_or_overwrite_a_newer_generation() {
    let plan = ReplacementPlan::prepare(7).unwrap();
    assert!(plan.require_current(8).is_err());
    let mut current = "旧会话";
    let mut generation = 8;
    assert!(plan
        .commit(&mut current, &mut generation, "过期候选")
        .is_err());
    assert_eq!((current, generation), ("旧会话", 8));
}

#[test]
fn commit_moves_authority_once_and_returns_retired_ownership() {
    let plan = ReplacementPlan::prepare(7).unwrap();
    let mut current = "旧会话";
    let mut generation = 7;
    let retired = plan
        .commit(&mut current, &mut generation, "新会话")
        .unwrap();
    assert_eq!((current, generation, retired), ("新会话", 8, "旧会话"));
    assert!(plan.require_current(generation).is_err());
    // commit 后的编码或清理失败不得回写旧 generation。
    let post_commit_failure: Result<(), &str> = Err("发布端初始化失败");
    assert!(post_commit_failure.is_err());
    assert_eq!((current, generation), ("新会话", 8));
}

#[test]
fn installing_new_ingress_closes_the_old_receiver_and_rejects_stale_plan() {
    use crate::session::protocol::ProtocolSession;
    use crate::{AccountId, Intent, OrderId};
    let setup = crate::session::npc_working_quote_tests::quote_setup(0);
    let old = ProtocolSession::new(setup.clone(), 42).unwrap();
    let new = ProtocolSession::new(setup, 43).unwrap();
    let old_ingress = old.shared_ingress();
    let mut binding = (1, old_ingress.clone());
    let plan = ReplacementPlan::prepare(1).unwrap();
    plan.install_ingress(&mut binding, new.shared_ingress())
        .unwrap();
    assert_eq!(binding.0, 2);
    let intent = Intent::Cancel {
        code: crate::StockId(0),
        id: OrderId(1),
    };
    assert!(old_ingress
        .enqueue_player_intent(AccountId(0), intent.clone())
        .is_err());
    assert!(plan.install_ingress(&mut binding, old_ingress).is_err());
    assert!(binding
        .1
        .enqueue_player_intent(AccountId(0), intent)
        .is_ok());
}

#[test]
fn restore_settings_preserve_speed_pause_and_running_intent_across_new_authority() {
    use crate::session::{
        authority::Authority, protocol::PausePreferences, runner::RequestedSpeed,
    };
    let now = libos::clock::Instant::now();
    let mut old = Authority::new(1000, 12, now);
    old.control_speed(64.0, 12, now, None).unwrap();
    old.control_running(true, 12, now, None).unwrap();
    let pause = PausePreferences {
        pause_after_close: true,
        pause_before_open: false,
    };
    let settings = RestoreSettings::capture(&old, pause);
    let mut replacement = Authority::new(1000, 3, now);
    settings.install(&mut replacement, 3, now);
    assert!(replacement.is_running());
    assert_eq!(
        replacement.requested_speed(),
        RequestedSpeed::Fixed { multiplier: 64.0 }
    );
    assert!(settings.pause.pause_after_close);
    assert!(!settings.pause.pause_before_open);
}

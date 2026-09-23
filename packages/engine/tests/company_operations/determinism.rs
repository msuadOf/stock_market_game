//! 确定性金样（K4 验收：同 seed 同事件/分录序列）+ RNG 状态持久化。

use super::fixtures::*;
use engine::company::events::ShockParams;
use engine::company::operations::CompanyOperations;

fn start() -> engine::calendar::CivilDate {
    d(START)
}

fn stress_ops(seed: u64) -> CompanyOperations {
    CompanyOperations::new(
        four_company_config(seed, ShockParams::stress_v1(), d("2029-12-31")),
        start(),
    )
    .expect("stress ops assembles")
}

fn run_days(ops: &mut CompanyOperations, days: i64) {
    let mut cursor = start();
    for _ in 0..days {
        ops.advance_civil_day(cursor).expect("day advances");
        cursor = cursor.next().expect("fixture dates advance");
    }
}

/// 同 seed + 压力冲击参数（每日必发事件）跑 30 日：事件序列、逐公司账套
/// （分录序列）与调度队列完全一致。
#[test]
fn same_seed_replays_identical_event_and_entry_sequence() {
    let mut first = stress_ops(99);
    let mut second = stress_ops(99);
    let mut first_reports = Vec::new();
    let mut second_reports = Vec::new();
    let mut cursor = start();
    for _ in 0..30 {
        first_reports.push(first.advance_civil_day(cursor).expect("day 1"));
        second_reports.push(second.advance_civil_day(cursor).expect("day 2"));
        cursor = cursor.next().expect("dates advance");
    }

    assert_eq!(first_reports, second_reports);
    assert_eq!(
        first.industrial_books(&cid("C-IND-A")).expect("A present"),
        second.industrial_books(&cid("C-IND-A")).expect("A present")
    );
    assert_eq!(
        first.bank_books(&cid("C-BANK")).expect("bank present"),
        second.bank_books(&cid("C-BANK")).expect("bank present")
    );
    assert_eq!(
        first.insurance_books(&cid("C-INS")).expect("ins present"),
        second.insurance_books(&cid("C-INS")).expect("ins present")
    );
    assert_eq!(
        first.real_estate_books(&cid("C-RE")).expect("re present"),
        second.real_estate_books(&cid("C-RE")).expect("re present")
    );
    assert_eq!(first.scheduler().pending(), second.scheduler().pending());
}

fn cid(id: &str) -> engine::company::CompanyId {
    engine::company::CompanyId(id.to_string())
}

/// 不同 seed → 事件序列分歧（压力参数下每日冲击幅度/持续期由 seed 决定）。
#[test]
fn different_seed_diverges() {
    let mut seed_a = stress_ops(99);
    let mut seed_b = stress_ops(100);
    let mut reports_a = Vec::new();
    let mut reports_b = Vec::new();
    let mut cursor = start();
    for _ in 0..10 {
        reports_a.push(seed_a.advance_civil_day(cursor).expect("day a"));
        reports_b.push(seed_b.advance_civil_day(cursor).expect("day b"));
        cursor = cursor.next().expect("dates advance");
    }
    assert_ne!(reports_a, reports_b);
}

/// RNG 状态持久化：serde 往返后继续推进，与不间断运行逐字节一致。
#[test]
fn rng_state_survives_serde_round_trip() {
    let mut uninterrupted = stress_ops(31);
    let mut restored = stress_ops(31);
    run_days(&mut uninterrupted, 10);
    run_days(&mut restored, 10);

    let saved = serde_json::to_string(&restored).expect("ops serializes");
    let mut round_tripped: CompanyOperations = serde_json::from_str(&saved).expect("ops restores");

    let mut cursor = d("2030-01-11");
    for _ in 0..5 {
        uninterrupted.advance_civil_day(cursor).expect("straight");
        round_tripped.advance_civil_day(cursor).expect("restored");
        cursor = cursor.next().expect("dates advance");
    }
    assert_eq!(
        round_tripped.industrial_books(&cid("C-IND-A")).expect("A"),
        uninterrupted.industrial_books(&cid("C-IND-A")).expect("A")
    );
    assert_eq!(
        round_tripped.scheduler().pending(),
        uninterrupted.scheduler().pending()
    );
}

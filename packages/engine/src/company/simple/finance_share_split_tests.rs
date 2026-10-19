use super::super::dividend_tests::{bind_capital, profitable_annual_fixture};
use super::super::tests::amount;
use super::*;
use crate::company::share_split::{ShareSplitDeclaration, ShareSplitDirection};
use crate::money::Money;

fn in_2031(month: u8, day: u8) -> CivilDate {
    CivilDate::from_ymd(2031, month, day).unwrap()
}

/// 面值从 2 分（注册资本 20_000 ÷ 10_000 股）出发的拆股／缩股声明。
fn declaration(
    event_id: &str,
    direction: ShareSplitDirection,
    ratio: u64,
    par_before: i64,
    par_after: i64,
    capital_at_approval: i128,
    approved_on: CivilDate,
) -> ShareSplitDeclaration {
    ShareSplitDeclaration {
        event_id: event_id.into(),
        approval_reference: format!("shareholders-resolution-{event_id}"),
        direction,
        approved_on,
        ratio,
        par_value_before: Money::from_cents(par_before),
        par_value_after: Money::from_cents(par_after),
        registered_capital_at_approval: AccountingAmount::from_cents(capital_at_approval),
    }
}

#[test]
fn split_declare_and_credit_keeps_registered_capital_constant() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    // 面值权威链首：注册资本 10_000 分 ÷ 5_000 股 = 面值 2 分；1 拆 2 → 面值 1 分。
    state
        .declare_share_split(declaration(
            "split-1",
            ShareSplitDirection::Split,
            2,
            2,
            1,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    // 非整除面值的拆股拒绝（面值最小单位为分）。
    assert!(state
        .declare_share_split(declaration(
            "split-bad",
            ShareSplitDirection::Split,
            3,
            2,
            0,
            10_000,
            in_2031(1, 1),
        ))
        .is_err());
    state
        .record_share_split_credit("split-1", in_2031(1, 2), 5_000, 10_000)
        .unwrap();
    // 拆股：注册资本不变；股数翻倍、面值减半。
    let facts = state.legal_facts().clone().unwrap();
    assert_eq!(facts.registered_capital, amount(10_000));
    let fact = state
        .share_split_facts()
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "split-1")
        .unwrap();
    assert_eq!(fact.destroyed_shares, 0);
    assert_eq!(fact.registered_capital_reduction, amount(0));
    assert_eq!(fact.settled_on, Some(in_2031(1, 2)));
    // 重复回填幂等。
    assert!(state
        .record_share_split_credit("split-1", in_2031(1, 2), 5_000, 10_000)
        .unwrap());
    assert_eq!(
        state.legal_facts().as_ref().unwrap().registered_capital,
        amount(10_000)
    );
    // 换算不一致拒绝（1 拆 2 只接受精确翻倍）。
    assert!(state
        .record_share_split_credit("split-1", in_2031(1, 2), 5_000, 9_999)
        .is_err());
}

#[test]
fn consolidation_declare_and_credit_reduces_capital_by_destroyed_face_value() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_share_split(declaration(
            "consolidation-1",
            ShareSplitDirection::Consolidate,
            2,
            1,
            2,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    // 缩股面值必须按整数比例放大。
    assert!(state
        .declare_share_split(declaration(
            "consolidation-bad",
            ShareSplitDirection::Consolidate,
            2,
            1,
            3,
            10_000,
            in_2031(1, 1),
        ))
        .is_err());
    // 8_001 股 2 并 1 → 4_000 股（碎股消灭 1 股旧股等价量）：核减额 =
    // par_before 1 分 × (8_001 − 2×4_000) = 1 分；注册资本 10_000 − 1 = 9_999 分
    // （面值×股本近似守恒：新面值 2 分 × 4_000 = 8_000 + 账面核减口径勾稽）。
    state
        .record_share_split_credit("consolidation-1", in_2031(1, 2), 8_001, 4_000)
        .unwrap();
    let facts = state.legal_facts().clone().unwrap();
    assert_eq!(facts.registered_capital, amount(9_999));
    let fact = state
        .share_split_facts()
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "consolidation-1")
        .unwrap();
    assert_eq!(fact.destroyed_shares, 4_001);
    assert_eq!(fact.registered_capital_reduction, amount(1));
}

#[test]
fn par_authority_chain_anchors_to_latest_split_and_validates_consistency() {
    let mut state = profitable_annual_fixture();
    bind_capital(&mut state);
    state
        .declare_share_split(declaration(
            "split-1",
            ShareSplitDirection::Split,
            2,
            2,
            1,
            10_000,
            in_2031(1, 1),
        ))
        .unwrap();
    state
        .record_share_split_credit("split-1", in_2031(1, 2), 5_000, 10_000)
        .unwrap();
    // 拆股后的新声明必须沿用重锚后的面值（权威 = 1 分；声明前值 2 分被拒）。
    assert!(state
        .declare_share_split(declaration(
            "stale-par",
            ShareSplitDirection::Consolidate,
            2,
            2,
            4,
            10_000,
            in_2031(1, 3),
        ))
        .is_err());
    state
        .declare_share_split(declaration(
            "consolidation-1",
            ShareSplitDirection::Consolidate,
            2,
            1,
            2,
            10_000,
            in_2031(1, 3),
        ))
        .unwrap();
    assert_eq!(
        state.current_par_value().unwrap(),
        Some(Money::from_cents(2))
    );
    // 恢复（serde 往返）保留拆股事实与面值权威。
    let restored: SimpleFinanceState =
        serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(restored, state);
    assert_eq!(restored.current_par_value().unwrap(), Some(Money::from_cents(2)));
    // 严格持久化：未知字段显式拒绝。
    let mut injected = serde_json::to_value(&state).unwrap();
    injected
        .as_object_mut()
        .unwrap()
        .insert("surprise".into(), serde_json::json!(1));
    assert!(serde_json::from_str::<SimpleFinanceState>(&injected.to_string()).is_err());
}

#[test]
fn share_split_facts_require_legal_capital_binding() {
    let mut state = profitable_annual_fixture();
    // 未绑定注册资本法定事实时显式拒绝。
    assert!(state
        .declare_share_split(declaration(
            "split-no-capital",
            ShareSplitDirection::Split,
            2,
            1,
            1,
            10_000,
            in_2031(1, 1),
        ))
        .is_err());
}

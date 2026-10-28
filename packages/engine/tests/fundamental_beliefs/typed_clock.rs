//! typed INFO 的旧标量拒绝首红；完整信念簿必须来自真实公开、本人获知与 cause。

use crate::gold::{momentum_analysis, momentum_profile, stock_code};
use crate::{
    assumptions_rng, hour_after, market, scenario, BeliefIssuerInputs, FundamentalBeliefCase,
    ISSUED_SHARES,
};
use engine::company::CompanyKind;
use engine::strategy::{BeliefBook, BeliefCause, BeliefError, FundamentalMethod};
use engine::AccountId;

fn produced_book() -> serde_json::Value {
    let mut rng = assumptions_rng(0.5);
    let mut case = FundamentalBeliefCase::new(
        scenario(),
        AccountId(2),
        market(40_000),
        momentum_profile(),
        momentum_analysis(FundamentalMethod::EarningsMultiple),
        BeliefIssuerInputs {
            kind: CompanyKind::Industrial,
            total_issued_shares: ISSUED_SHARES,
        },
        &mut rng,
    );
    let report = case.scenario.annual_ids[3];
    case.acquire(report, hour_after(case.scenario.annual_instants[3]))
        .expect("必须通过本人真实获知形成输入");
    case.apply_cause(&stock_code(), BeliefCause::NewMaterial { report }, 1_000)
        .expect("必须通过真实 cause 形成条目及 CauseRecord");
    let value = serde_json::to_value(&case.book).expect("真实 BeliefBook 必须可序列化");
    assert_eq!(value["entries"].as_object().unwrap().len(), 1);
    assert!(!value["entries"]["0"]["last_cause"].is_null());
    let roundtrip: BeliefBook =
        serde_json::from_value(value.clone()).expect("变异前的真实生产结果必须可以恢复");
    assert_eq!(roundtrip, case.book);
    value
}

#[test]
fn typed_info_rejects_scalar_entry_anchor_in_real_belief_book() {
    let mut value = produced_book();
    value["entries"]["0"]["anchor_trading_day"] = serde_json::json!(0);
    assert!(
        serde_json::from_value::<BeliefBook>(value).is_err(),
        "typed INFO 必须拒绝标量 entry anchor，不能把 closed 或 NoPriorSession 静默补成交易日0"
    );
}

#[test]
fn typed_info_rejects_scalar_cause_anchor_in_real_belief_book() {
    let mut value = produced_book();
    value["entries"]["0"]["last_cause"]["as_of_trading_day"] = serde_json::json!(0);
    assert!(
        serde_json::from_value::<BeliefBook>(value).is_err(),
        "typed INFO 必须拒绝标量 CauseRecord 时点，不能把 closed 或 NoPriorSession 静默补成交易日0"
    );
}

use engine::observation::information_clock::InformationHorizonError;
use engine::observation::information_clock::InformationTradingAnchor::{
    self, DayBefore, NoPriorSession, TradingDay,
};
use engine::orderbook::OrderId;

/// 显式锚只用于纯 Belief 输入测试；真实 calendar 三场景另经生产 caller 导出。
fn case_at(anchor: InformationTradingAnchor) -> FundamentalBeliefCase {
    let mut rng = assumptions_rng(0.5);
    let mut case = FundamentalBeliefCase::new(
        scenario(),
        AccountId(2),
        market(40_000),
        momentum_profile(),
        momentum_analysis(FundamentalMethod::EarningsMultiple),
        BeliefIssuerInputs {
            kind: CompanyKind::Industrial,
            total_issued_shares: ISSUED_SHARES,
        },
        &mut rng,
    );
    let report = case.scenario.annual_ids[3];
    case.acquire(report, hour_after(case.scenario.annual_instants[3]))
        .unwrap();
    case.apply_cause_at(&stock_code(), BeliefCause::NewMaterial { report }, anchor)
        .unwrap();
    case
}

#[test]
fn typed_belief_book_preserves_three_states_after_real_acquisition_and_cause() {
    for anchor in [
        NoPriorSession,
        TradingDay { ordinal: 0 },
        DayBefore { ordinal: 0 },
    ] {
        let case = case_at(anchor);
        let entry = case.book.entry(&stock_code()).unwrap();
        assert_eq!(entry.anchor, anchor);
        assert_eq!(entry.last_cause.as_ref().unwrap().as_of_anchor, anchor);
        let wire = serde_json::to_value(&case.book).unwrap();
        assert!(wire["entries"]["0"].get("anchor_trading_day").is_none());
        assert!(wire["entries"]["0"]["last_cause"]
            .get("as_of_trading_day")
            .is_none());
        let restored: BeliefBook = serde_json::from_value(wire).unwrap();
        assert_eq!(restored, case.book);
    }
}

#[test]
fn typed_belief_book_rejects_new_scalar_anchor_and_nested_extra_fields() {
    let original = serde_json::to_value(case_at(TradingDay { ordinal: 0 }).book).unwrap();
    for (cause, field, value) in [
        (false, "anchor", serde_json::json!(0)),
        (true, "as_of_anchor", serde_json::json!(0)),
        (false, "extra", serde_json::json!(false)),
        (true, "extra", serde_json::json!(false)),
    ] {
        let mut changed = original.clone();
        let entry = &mut changed["entries"]["0"];
        let target = if cause {
            &mut entry["last_cause"]
        } else {
            entry
        };
        target[field] = value;
        assert!(
            serde_json::from_value::<BeliefBook>(changed).is_err(),
            "必须拒绝 nested field {field}, cause={cause}"
        );
    }
}

#[test]
fn typed_belief_rejects_time_reversal_before_used_report_shortcut_without_mutation() {
    let mut case = case_at(DayBefore { ordinal: 0 });
    let before = serde_json::to_vec(&case.book).unwrap();
    let report = case.scenario.annual_ids[3];
    assert!(matches!(
        case.apply_cause_at(
            &stock_code(),
            BeliefCause::NewMaterial { report },
            TradingDay { ordinal: 0 }
        ),
        Err(BeliefError::InformationClock(
            InformationHorizonError::TimeReversed { .. }
        ))
    ));
    assert_eq!(serde_json::to_vec(&case.book).unwrap(), before);
}

#[test]
fn typed_belief_rejects_time_reversal_before_consumed_order_shortcut_without_mutation() {
    let mut case = case_at(TradingDay { ordinal: 0 });
    let cause = BeliefCause::ExperienceFailure { order: OrderId(41) };
    case.apply_cause_at(&stock_code(), cause.clone(), DayBefore { ordinal: 0 })
        .unwrap();
    let before = serde_json::to_vec(&case.book).unwrap();
    assert!(matches!(
        case.apply_cause_at(&stock_code(), cause.clone(), TradingDay { ordinal: 0 }),
        Err(BeliefError::InformationClock(
            InformationHorizonError::TimeReversed { .. }
        ))
    ));
    assert_eq!(serde_json::to_vec(&case.book).unwrap(), before);
    case.apply_cause_at(&stock_code(), cause, DayBefore { ordinal: 0 })
        .unwrap();
    assert_eq!(serde_json::to_vec(&case.book).unwrap(), before);
}

#[test]
fn typed_belief_rejects_future_entry_or_cause_and_zero_horizon_before_any_mutation() {
    let original = serde_json::to_value(case_at(TradingDay { ordinal: 0 }).book).unwrap();
    for (field, value) in [
        (
            "anchor",
            serde_json::json!({"kind":"DayBefore","ordinal":"0"}),
        ),
        (
            "cause",
            serde_json::json!({"kind":"DayBefore","ordinal":"0"}),
        ),
        ("horizon_trading_days", serde_json::json!(0)),
    ] {
        let mut changed = original.clone();
        if field == "cause" {
            changed["entries"]["0"]["last_cause"]["as_of_anchor"] = value;
        } else {
            changed["entries"]["0"][field] = value;
        }
        let mut case = case_at(TradingDay { ordinal: 0 });
        case.book = serde_json::from_value(changed).unwrap();
        let before = serde_json::to_vec(&case.book).unwrap();
        let report = case.scenario.annual_ids[3];
        let error = case
            .apply_cause_at(
                &stock_code(),
                BeliefCause::NewMaterial { report },
                TradingDay { ordinal: 0 },
            )
            .unwrap_err();
        if field == "horizon_trading_days" {
            assert!(matches!(
                error,
                BeliefError::InformationClock(InformationHorizonError::ZeroHorizon)
            ));
        } else {
            assert!(matches!(
                error,
                BeliefError::InformationClock(InformationHorizonError::TimeReversed { .. })
            ));
        }
        assert_eq!(serde_json::to_vec(&case.book).unwrap(), before);
    }
}

#[test]
fn typed_belief_rejects_cause_before_entry_even_when_current_anchor_is_reached() {
    let mut case = case_at(TradingDay { ordinal: 1 });
    let mut changed = serde_json::to_value(&case.book).unwrap();
    changed["entries"]["0"]["last_cause"]["as_of_anchor"] =
        serde_json::json!({"kind":"TradingDay","ordinal":"0"});
    case.book = serde_json::from_value(changed).unwrap();
    let before = serde_json::to_vec(&case.book).unwrap();
    let report = case.scenario.annual_ids[3];
    assert!(matches!(
        case.apply_cause_at(
            &stock_code(),
            BeliefCause::NewMaterial { report },
            TradingDay { ordinal: 1 }
        ),
        Err(BeliefError::InformationClock(
            InformationHorizonError::TimeReversed { .. }
        ))
    ));
    assert_eq!(serde_json::to_vec(&case.book).unwrap(), before);
}

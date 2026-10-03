//! 保险 owner 迁移的短行为保护：存档接受集、事件槽与赔案累计边界。

use super::*;
use crate::accounting::{FractionUnits, PostingSide};
use crate::calendar::CivilDate;
use crate::company::CounterpartyKind;

fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

fn date() -> CivilDate {
    CivilDate::from_iso("2030-01-01").unwrap()
}

fn config() -> InsuranceConfig {
    InsuranceConfig {
        chart: insurance_account_chart(),
        as_of: date(),
        opening_lines: vec![
            line(chart::acct::CASH, PostingSide::Debit, amount(200_000)),
            line(chart::acct::CAPITAL, PostingSide::Credit, amount(200_000)),
        ],
        counterparties: vec![crate::company::ExternalCounterparty {
            id: CounterpartyId("POL".into()),
            kind: CounterpartyKind::Customer,
            name: "测试投保人".into(),
        }],
        // Fixture：显式游戏假设，不代表真实精算利率。
        discount: DiscountAssumption {
            version: 1,
            rate_bp: 400,
        },
    }
}

fn seeded() -> (InsuranceBooks, ContractId) {
    let mut books = InsuranceBooks::new(config()).unwrap();
    let group = ContractId("GROUP".into());
    books
        .establish_group(
            InsuranceProductKind::TermProtection,
            group.clone(),
            &CounterpartyId("POL".into()),
            amount(100_000),
            amount(80_000),
            amount(5_000),
            date(),
            CivilDate::from_iso("2031-01-01").unwrap(),
        )
        .unwrap();
    (books, group)
}

#[test]
fn opening_guard_keeps_discount_precedence_and_first_seeded_account() {
    let mut input = config();
    input.opening_lines = vec![
        line(chart::acct::LIC, PostingSide::Credit, amount(1)),
        line(chart::acct::LRC, PostingSide::Debit, amount(1)),
    ];
    input.discount.rate_bp = 0;
    assert_eq!(
        InsuranceBooks::new(input.clone()),
        Err(InsuranceError::InvalidDiscountRate { rate_bp: 0 })
    );
    input.discount.rate_bp = 400;
    assert_eq!(
        InsuranceBooks::new(input),
        Err(InsuranceError::OpeningInsuranceBooksSeeded {
            account: LedgerAccountId(chart::acct::LIC.into()),
        })
    );
}

#[test]
fn multiple_partial_payments_exhaust_claim_then_reject_one_cent() {
    let (mut books, group) = seeded();
    let claim = ClaimId("CLAIM".into());
    books
        .record_claim(&group, claim.clone(), amount(101), date())
        .unwrap();
    for cents in [30, 70, 1] {
        books
            .pay_claim(&group, &claim, amount(cents), date())
            .unwrap();
    }
    let state = books.group(&group).unwrap().claim(&claim).unwrap();
    assert_eq!(state.paid(), amount(101));
    assert_eq!(state.unpaid(), Ok(amount(0)));
    let before = serde_json::to_vec(&books).unwrap();
    assert_eq!(
        books.pay_claim(&group, &claim, amount(1), date()),
        Err(InsuranceError::ClaimPaymentBeyondOutstanding {
            claim,
            requested: amount(1),
            outstanding: amount(0),
        })
    );
    assert_eq!(serde_json::to_vec(&books).unwrap(), before);
}

#[test]
fn flat_group_snapshot_keeps_required_duplicate_and_unknown_field_behavior() {
    let (books, group) = seeded();
    let state = books.group(&group).unwrap();
    let mut value = serde_json::to_value(state).unwrap();
    let keys = [
        "policyholder",
        "premium",
        "premium_collected",
        "expected_claims_remaining",
        "risk_adjustment_remaining",
        "csm",
        "loss_component",
        "finance_remaining",
        "units_total",
        "units_released",
        "coverage_start",
        "coverage_end",
        "day_one_loss",
        "released_revenue",
        "released_finance",
        "remeasure_finance",
        "remeasure_loss",
        "reestimated_csm",
        "carried_claims",
        "carried_risk_adjustment",
        "carried_csm",
        "carried_finance",
        "carried_loss",
        "claims",
    ];
    assert_eq!(value.as_object().unwrap().len(), keys.len());
    let sequence = serde_json::Value::Array(keys.iter().map(|key| value[*key].clone()).collect());
    assert_eq!(
        serde_json::from_value::<ContractGroupState>(sequence).unwrap(),
        *state
    );
    for key in keys {
        assert!(value.get(key).is_some(), "缺少存档字段 {key}");
        let mut missing = value.clone();
        missing.as_object_mut().unwrap().remove(key);
        assert!(serde_json::from_value::<ContractGroupState>(missing).is_err());
    }
    value["future_field"] = serde_json::json!({"nested": true});
    assert_eq!(
        serde_json::from_value::<ContractGroupState>(value).unwrap(),
        *state
    );
    let json = serde_json::to_string(state).unwrap();
    let duplicate = format!("{{\"units_total\":365,{}", &json[1..]);
    assert!(serde_json::from_str::<ContractGroupState>(&duplicate).is_err());
}

#[test]
fn restored_remeasurement_and_release_keep_all_tail_components_and_carries() {
    let (mut books, group) = seeded();
    books.release_service(&group, 100, date()).unwrap();
    books.remeasure(&group, date(), amount(66_082)).unwrap();
    let mut restored: InsuranceBooks =
        serde_json::from_slice(&serde_json::to_vec(&books).unwrap()).unwrap();
    for units in [7, 258] {
        assert_eq!(
            books.release_service(&group, units, date()),
            restored.release_service(&group, units, date())
        );
        assert_eq!(books, restored);
        let state = books.group(&group).unwrap();
        let lrc = books
            .books()
            .ledger()
            .account_net_debit(&LedgerAccountId(chart::acct::LRC.into()))
            .unwrap()
            .neg()
            .unwrap();
        assert_eq!(
            lrc,
            state
                .expected_claims_remaining()
                .add(state.risk_adjustment_remaining())
                .unwrap()
                .add(state.csm())
                .unwrap()
                .sub(state.finance_remaining())
                .unwrap()
        );
    }
    let value = serde_json::to_value(books.group(&group).unwrap()).unwrap();
    for key in [
        "expected_claims_remaining",
        "risk_adjustment_remaining",
        "csm",
        "loss_component",
        "finance_remaining",
    ] {
        assert_eq!(value[key], serde_json::to_value(amount(0)).unwrap());
    }
    for key in [
        "carried_claims",
        "carried_risk_adjustment",
        "carried_csm",
        "carried_finance",
        "carried_loss",
    ] {
        assert_eq!(
            value[key],
            serde_json::to_value(FractionUnits::ZERO).unwrap()
        );
    }
    assert_eq!(books.group(&group).unwrap().units_remaining(), 0);
}

#[test]
fn zero_remeasurement_consumes_one_event_slot_without_group_writes() {
    let (mut books, group) = seeded();
    let before = books.group(&group).unwrap().clone();
    let slot = books.next_event_id;
    assert_eq!(books.remeasure(&group, date(), amount(80_000)), Ok(None));
    assert_eq!(books.next_event_id, slot + 1);
    assert_eq!(*books.group(&group).unwrap(), before);
}

#[test]
fn release_apply_overflow_preserves_existing_post_and_partial_write_order() {
    let (mut books, group) = seeded();
    let mut saved = serde_json::to_value(books.group(&group).unwrap()).unwrap();
    saved["released_revenue"] = serde_json::to_value(AccountingAmount::MAX).unwrap();
    books
        .groups
        .insert(group.clone(), serde_json::from_value(saved).unwrap());
    let slot = books.next_event_id;
    assert!(matches!(
        books.release_service(&group, 365, date()),
        Err(InsuranceError::Accounting(
            AccountingError::AmountOverflow { op: "add", .. }
        ))
    ));
    // 既有边界：post 已提交，五个余额已递减；累计溢出时进度尚未推进。
    let state = books.group(&group).unwrap();
    assert_eq!(state.expected_claims_remaining(), amount(0));
    assert_eq!(state.risk_adjustment_remaining(), amount(0));
    assert_eq!(state.csm(), amount(0));
    assert_eq!(state.finance_remaining(), amount(0));
    assert_eq!(state.released_revenue_total(), AccountingAmount::MAX);
    assert_eq!(state.released_finance_total(), amount(0));
    assert_eq!(state.units_released(), 0);
    assert_eq!(books.next_event_id, slot + 2);
}

#[test]
fn measurement_previews_are_pure_and_keep_exact_release_and_remeasurement_gold() {
    let (mut books, group) = seeded();
    let before = books.group(&group).unwrap().clone();
    let batch = before.measurement().preview_release(100).unwrap();
    assert_eq!(
        (
            batch.claims,
            batch.risk_adjustment,
            batch.csm,
            batch.finance,
            batch.loss_memo
        ),
        (21_918, 1_370, 4_953, 843, 0)
    );
    assert_eq!(
        (
            batch.carried_claims.units(),
            batch.carried_risk_adjustment.units(),
            batch.carried_csm.units(),
            batch.carried_finance.units(),
            batch.carried_loss.units()
        ),
        (-70, -50, -145, 5, 0)
    );
    assert_eq!(*books.group(&group).unwrap(), before);
    books.release_service(&group, 100, date()).unwrap();
    let before = books.group(&group).unwrap().clone();
    let delta = before
        .measurement()
        .preview_remeasure(amount(66_082), 400)
        .unwrap()
        .unwrap();
    assert_eq!(delta.finance_posted, amount(226));
    assert_eq!(delta.loss_posted, amount(0));
    assert_eq!(*books.group(&group).unwrap(), before);
    let mut predicted = before.clone();
    predicted.measurement_mut().apply_remeasure(delta).unwrap();
    assert_eq!(predicted.csm(), amount(5_350));
    assert_eq!(predicted.reestimated_csm_total(), amount(7_774));
    books.remeasure(&group, date(), amount(66_082)).unwrap();
    assert_eq!(*books.group(&group).unwrap(), predicted);
}

#[test]
fn failed_post_leaves_measurement_unchanged_for_release_and_remeasurement() {
    for remeasure in [false, true] {
        let (mut books, group) = seeded();
        // 已有建组 source=2；注入重复事件槽，强制 post 拒绝。
        books.next_event_id = 2;
        let before = serde_json::to_vec(&books).unwrap();
        let result = if remeasure {
            books.remeasure(&group, date(), amount(108_000))
        } else {
            books.release_service(&group, 100, date())
        };
        assert!(matches!(result, Err(InsuranceError::Accounting(_))));
        assert_eq!(serde_json::to_vec(&books).unwrap(), before);
    }
}

#[test]
fn exhausted_csm_keeps_existing_inactive_carry_after_final_release() {
    let (mut books, group) = seeded();
    books.release_service(&group, 100, date()).unwrap();
    books.remeasure(&group, date(), amount(108_082)).unwrap();
    assert_eq!(books.group(&group).unwrap().csm(), amount(0));
    books.release_service(&group, 265, date()).unwrap();
    let saved = serde_json::to_value(books.group(&group).unwrap()).unwrap();
    // 既有口径：非正 CSM 不调用 unit_release，保留失活 carry；本轮不改规则。
    assert_eq!(
        saved["carried_csm"],
        serde_json::to_value(FractionUnits::from_units(-145)).unwrap()
    );
    assert_eq!(books.group(&group).unwrap().units_remaining(), 0);
}

#[test]
fn remeasurement_apply_overflow_preserves_existing_partial_write_order() {
    let (mut books, group) = seeded();
    books.release_service(&group, 100, date()).unwrap();
    let mut saved = serde_json::to_value(books.group(&group).unwrap()).unwrap();
    saved["reestimated_csm"] = serde_json::to_value(AccountingAmount::MAX).unwrap();
    books
        .groups
        .insert(group.clone(), serde_json::from_value(saved).unwrap());
    let slot = books.next_event_id;
    assert!(matches!(
        books.remeasure(&group, date(), amount(66_082)),
        Err(InsuranceError::Accounting(
            AccountingError::AmountOverflow { op: "add", .. }
        ))
    ));
    let state = books.group(&group).unwrap();
    assert_eq!(state.expected_claims_remaining(), amount(66_082));
    assert_eq!(state.csm(), amount(5_350));
    assert_eq!(state.loss_component(), amount(0));
    assert_eq!(state.reestimated_csm_total(), AccountingAmount::MAX);
    assert_eq!(state.remeasure_finance_total(), amount(0));
    assert_eq!(books.next_event_id, slot + 2);
}

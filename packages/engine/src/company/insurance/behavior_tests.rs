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
        income_tax_policy: crate::accounting::IncomeTaxPolicy {
            rate_bp: 2_500,
            loss_carryforward_years: 5,
        },
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
    groups::restore_tests::set_released_revenue(
        books.groups.get_mut(&group).unwrap(),
        AccountingAmount::MAX,
    );
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
    groups::restore_tests::set_reestimated_csm(
        books.groups.get_mut(&group).unwrap(),
        AccountingAmount::MAX,
    );
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

#[test]
fn restored_books_reject_corrupt_group_json_and_direct_memory_state() {
    let (mut books, group) = seeded();
    books.validate_restore().unwrap();
    let mut saved = serde_json::to_value(&books).unwrap();
    saved["groups"]["GROUP"]["released_revenue"] =
        serde_json::to_value(AccountingAmount::MAX).unwrap();
    assert!(serde_json::from_value::<InsuranceBooks>(saved).is_err());
    groups::restore_tests::set_released_revenue(
        books.groups.get_mut(&group).unwrap(),
        AccountingAmount::MAX,
    );
    let error = books.validate_restore().unwrap_err().to_string();
    assert!(error.contains("GROUP"), "{error}");
}

#[test]
fn restored_books_revalidate_discount_and_policyholder_reference() {
    let (mut books, group) = seeded();
    books.discount.rate_bp = 0;
    assert_eq!(
        books.validate_restore(),
        Err(InsuranceError::InvalidDiscountRate { rate_bp: 0 })
    );
    books.discount.rate_bp = 400;
    let mut saved = serde_json::to_value(books.group(&group).unwrap()).unwrap();
    saved["policyholder"] = serde_json::json!("UNKNOWN");
    books
        .groups
        .insert(group, serde_json::from_value(saved).unwrap());
    assert!(matches!(
        books.validate_restore(),
        Err(InsuranceError::Company(
            crate::company::CompanyError::UnknownCounterparty { .. }
        ))
    ));
}

#[test]
fn maximum_premium_established_by_public_api_roundtrips() {
    let mut books = InsuranceBooks::new(config()).unwrap();
    let group = ContractId("MAX".into());
    books
        .establish_group(
            InsuranceProductKind::TermProtection,
            group,
            &CounterpartyId("POL".into()),
            AccountingAmount::MAX,
            amount(80_000),
            amount(0),
            date(),
            CivilDate::from_iso("2031-01-01").unwrap(),
        )
        .unwrap();
    assert_eq!(books.validate_restore(), Ok(()));
    assert_eq!(
        serde_json::from_slice::<InsuranceBooks>(&serde_json::to_vec(&books).unwrap()).unwrap(),
        books
    );
}

#[test]
fn annual_income_tax_is_idempotent_and_payment_does_not_repeat_expense() {
    let (mut books, group) = seeded();
    books.release_service(&group, 365, date()).unwrap();
    let closing_date = CivilDate::from_iso("2030-12-31").unwrap();
    let first = books.accrue_income_tax(closing_date).unwrap();
    assert_eq!(first.current_tax, amount(25_000));
    let after_first = books.clone();
    let repeated = books.accrue_income_tax(closing_date).unwrap();
    assert_eq!(repeated.current_tax_delta, amount(0));
    assert_eq!(books, after_first);
    books.pay_income_tax(amount(10_000), closing_date).unwrap();
    assert_eq!(
        books
            .books()
            .ledger()
            .account_net_debit(&LedgerAccountId("222104".into()))
            .unwrap(),
        amount(-15_000)
    );
    assert_eq!(
        books
            .books()
            .ledger()
            .account_net_debit(&LedgerAccountId("6801".into()))
            .unwrap(),
        amount(25_000)
    );
    books.validate_restore().unwrap();
}

#[test]
fn failed_income_tax_payment_and_corrupt_owner_restore_are_atomic() {
    let (mut books, group) = seeded();
    books.release_service(&group, 365, date()).unwrap();
    let closing_date = CivilDate::from_iso("2030-12-31").unwrap();
    books.accrue_income_tax(closing_date).unwrap();
    let before = books.clone();
    assert!(books.pay_income_tax(amount(25_001), closing_date).is_err());
    assert_eq!(books, before);
    let mut saved = serde_json::to_value(&books).unwrap();
    saved["income_tax_position"]["assessments"] = serde_json::json!({});
    let corrupt: InsuranceBooks = serde_json::from_value(saved).unwrap();
    assert!(corrupt.validate_restore().is_err());
    let mut saved = serde_json::to_value(&books).unwrap();
    saved.as_object_mut().unwrap().remove("income_tax_policy");
    assert!(serde_json::from_value::<InsuranceBooks>(saved).is_err());
}

#[test]
fn initial_deferred_tax_asset_is_real_and_invalid_policy_is_rejected() {
    let mut input = config();
    input
        .opening_lines
        .push(line("1811", PostingSide::Debit, amount(100)));
    input
        .opening_lines
        .push(line("4001", PostingSide::Credit, amount(100)));
    let books = InsuranceBooks::new(input.clone()).unwrap();
    let saved = serde_json::to_value(&books).unwrap();
    assert_eq!(
        saved["income_tax_position"]["initial_deferred_tax_asset"],
        serde_json::to_value(amount(100)).unwrap()
    );
    books.validate_restore().unwrap();
    input.income_tax_policy.rate_bp = -1;
    assert!(InsuranceBooks::new(input).is_err());
}

#[test]
fn restore_rejects_insurance_subledger_drift_and_reused_event_slot() {
    let (mut books, group) = seeded();
    books.release_service(&group, 100, date()).unwrap();
    books.validate_restore().unwrap();
    let valid = books.clone();
    books.next_event_id = 2;
    assert!(books.validate_restore().is_err());
    books = valid;
    books
        .books
        .post_batch(vec![crate::accounting::JournalEntry {
            source: crate::accounting::BusinessEventId::new(books.next_event_id),
            date: date(),
            kind: crate::accounting::BusinessKind::InsuranceServiceRevenue,
            cash_flow: crate::accounting::CashFlowClass::NonCash,
            lines: vec![
                line("2501", PostingSide::Debit, amount(1)),
                line("6051", PostingSide::Credit, amount(1)),
            ],
        }])
        .unwrap();
    books.next_event_id += 1;
    assert!(books.validate_restore().is_err());
}

#[test]
fn paid_income_tax_reassessment_creates_asset_without_cash_refund() {
    let (mut books, group) = seeded();
    books.release_service(&group, 365, date()).unwrap();
    let closing_date = CivilDate::from_iso("2030-12-31").unwrap();
    books.accrue_income_tax(closing_date).unwrap();
    books.pay_income_tax(amount(25_000), closing_date).unwrap();
    let cash = books
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId("1002".into()))
        .unwrap();
    books
        .record_claim(
            &group,
            ClaimId("LATE".into()),
            amount(120_000),
            closing_date,
        )
        .unwrap();
    let outcome = books.accrue_income_tax(closing_date).unwrap();
    assert_eq!(outcome.current_tax_delta, amount(-25_000));
    assert_eq!(outcome.deferred_delta, amount(5_000));
    assert_eq!(
        books
            .books()
            .ledger()
            .account_net_debit(&LedgerAccountId("1002".into()))
            .unwrap(),
        cash
    );
    assert_eq!(
        books
            .books()
            .ledger()
            .account_net_debit(&LedgerAccountId("222104".into()))
            .unwrap(),
        amount(25_000)
    );
    books.validate_restore().unwrap();
    let before = books.clone();
    books.accrue_income_tax(closing_date).unwrap();
    assert_eq!(books, before);
}

#[test]
fn failed_tax_post_preserves_owner_and_insufficient_cash_is_not_replenished() {
    let (mut books, group) = seeded();
    books.release_service(&group, 365, date()).unwrap();
    let closing_date = CivilDate::from_iso("2030-12-31").unwrap();
    let valid_next = books.next_event_id;
    books.next_event_id = 2;
    let before = books.clone();
    assert!(books.accrue_income_tax(closing_date).is_err());
    assert_eq!(books, before);
    books.next_event_id = valid_next;
    books.accrue_income_tax(closing_date).unwrap();
    books
        .books
        .post_batch(vec![crate::accounting::JournalEntry {
            source: crate::accounting::BusinessEventId::new(books.next_event_id),
            date: closing_date,
            kind: crate::accounting::BusinessKind::CashExpense,
            cash_flow: crate::accounting::CashFlowClass::Financing,
            lines: vec![
                line("4001", PostingSide::Debit, amount(200_000)),
                line("1002", PostingSide::Credit, amount(200_000)),
            ],
        }])
        .unwrap();
    books.next_event_id += 1;
    let before = books.clone();
    assert!(matches!(
        books.pay_income_tax(amount(25_000), closing_date),
        Err(InsuranceError::PaymentFailed { .. })
    ));
    assert_eq!(books, before);
}

#[test]
fn historical_tax_cascade_survives_restore_and_keeps_real_cash_dates() {
    let (mut books, group) = seeded();
    let first_end = CivilDate::from_iso("2030-12-31").unwrap();
    books.release_service(&group, 365, first_end).unwrap();
    books
        .record_claim(&group, ClaimId("FIRST".into()), amount(120_000), first_end)
        .unwrap();
    books.accrue_income_tax(first_end).unwrap();
    assert_eq!(books.loss_pool()[0].remaining, amount(20_000));
    let mut restored: InsuranceBooks =
        serde_json::from_slice(&serde_json::to_vec(&books).unwrap()).unwrap();
    restored.validate_restore().unwrap();
    let second_start = CivilDate::from_iso("2031-01-01").unwrap();
    let second_end = CivilDate::from_iso("2031-12-31").unwrap();
    let second_group = ContractId("SECOND".into());
    restored
        .establish_group(
            InsuranceProductKind::TermProtection,
            second_group.clone(),
            &CounterpartyId("POL".into()),
            amount(100_000),
            amount(80_000),
            amount(5_000),
            second_start,
            CivilDate::from_iso("2032-01-01").unwrap(),
        )
        .unwrap();
    restored
        .release_service(&second_group, 365, second_end)
        .unwrap();
    assert_eq!(
        restored.accrue_income_tax(second_end).unwrap().current_tax,
        amount(20_000)
    );
    let posted_on = CivilDate::from_iso("2032-01-01").unwrap();
    let late_source = restored
        .record_claim(
            &group,
            ClaimId("LATE-HISTORY".into()),
            amount(80_000),
            posted_on,
        )
        .unwrap();
    let adjustments = std::collections::BTreeMap::from([(
        late_source,
        crate::accounting::AccountingPeriod::from_ymd(2030, 12).unwrap(),
    )]);
    let cash = restored
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId("1002".into()))
        .unwrap();
    let outcome = restored
        .reassess_income_tax(2030, posted_on, &adjustments)
        .unwrap();
    assert_eq!(outcome.current_tax_delta, amount(-20_000));
    assert!(restored.loss_pool().is_empty());
    assert_eq!(
        restored
            .books()
            .journal()
            .entries()
            .find(|entry| entry.source == late_source)
            .unwrap()
            .date,
        posted_on
    );
    assert_eq!(
        restored
            .books()
            .ledger()
            .account_net_debit(&LedgerAccountId("1002".into()))
            .unwrap(),
        cash
    );
    restored.validate_restore().unwrap();
    let mut final_restore: InsuranceBooks =
        serde_json::from_slice(&serde_json::to_vec(&restored).unwrap()).unwrap();
    final_restore.validate_restore().unwrap();
    final_restore
        .reassess_income_tax(2030, posted_on, &adjustments)
        .unwrap();
    assert_eq!(final_restore, restored);
}

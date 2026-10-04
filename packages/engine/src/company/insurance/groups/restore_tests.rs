use super::*;

pub(in crate::company::insurance) fn set_released_revenue(
    state: &mut ContractGroupState,
    amount: AccountingAmount,
) {
    state.measurement.released_revenue = amount;
}

pub(in crate::company::insurance) fn set_reestimated_csm(
    state: &mut ContractGroupState,
    amount: AccountingAmount,
) {
    state.measurement.reestimated_csm = amount;
}

fn seeded() -> ContractGroupState {
    ContractGroupState::new(
        CounterpartyId("POL".into()),
        AccountingAmount::from_cents(100_000),
        AccountingAmount::from_cents(80_000),
        AccountingAmount::from_cents(5_000),
        AccountingAmount::from_cents(18_077),
        AccountingAmount::ZERO,
        AccountingAmount::from_cents(3_077),
        CivilDate::from_iso("2030-01-01").unwrap(),
        CivilDate::from_iso("2031-01-01").unwrap(),
        365,
    )
}

fn rejects(key: &str, replacement: serde_json::Value) {
    let mut saved = serde_json::to_value(seeded()).unwrap();
    saved[key] = replacement;
    let restored = serde_json::from_value::<ContractGroupState>(saved);
    assert!(restored.is_err(), "损坏字段 {key} 被接受：{restored:?}");
}

#[test]
fn restore_rejects_released_revenue_overflow_and_reconciliation_drift() {
    rejects(
        "released_revenue",
        serde_json::to_value(AccountingAmount::MAX).unwrap(),
    );
    rejects(
        "released_revenue",
        serde_json::to_value(AccountingAmount::from_cents(1)).unwrap(),
    );
}

#[test]
fn restore_rejects_invalid_coverage_progress() {
    for (key, value) in [
        ("units_total", 0),
        ("units_total", i64::MAX),
        ("units_released", -1),
        ("units_released", 366),
        ("units_released", 365),
    ] {
        rejects(key, serde_json::json!(value));
    }
    rejects("coverage_end", serde_json::json!("2029-12-31"));
}

#[test]
fn restore_rejects_negative_balances_and_excess_collections() {
    for key in [
        "premium",
        "premium_collected",
        "expected_claims_remaining",
        "risk_adjustment_remaining",
        "csm",
        "loss_component",
        "finance_remaining",
        "day_one_loss",
        "released_revenue",
        "released_finance",
        "reestimated_csm",
    ] {
        rejects(key, serde_json::json!("-1"));
    }
    rejects("premium", serde_json::json!("0"));
    rejects("premium_collected", serde_json::json!("100001"));
    rejects("loss_component", serde_json::json!("1"));
}

#[test]
fn restore_rejects_out_of_range_carries_but_accepts_signed_carries() {
    for key in [
        "carried_claims",
        "carried_risk_adjustment",
        "carried_csm",
        "carried_finance",
        "carried_loss",
    ] {
        rejects(key, serde_json::json!("183"));
        rejects(key, serde_json::json!(i128::MIN.to_string()));
    }
    let mut state = seeded();
    let batch = state.measurement.preview_release(100).unwrap();
    state.measurement.apply_release(batch).unwrap();
    assert!(state.measurement.carried_csm.units() < 0);
    assert_eq!(
        serde_json::from_value::<ContractGroupState>(serde_json::to_value(&state).unwrap())
            .unwrap(),
        state
    );
}

#[test]
fn restore_rejects_inconsistent_claim_balances() {
    for (incurred, paid) in [(0, 0), (-1, 0), (1, -1), (1, 2)] {
        rejects(
            "claims",
            serde_json::json!({"CLAIM": {
                "incurred": incurred.to_string(), "paid": paid.to_string(), "date_incurred": "2030-01-01"
            }}),
        );
    }
}

#[test]
fn restore_accepts_remeasurement_and_terminal_inactive_carries() {
    let mut state = seeded();
    let batch = state.measurement.preview_release(100).unwrap();
    state.measurement.apply_release(batch).unwrap();
    for estimate in [108_082, 50_000, 70_000] {
        let delta = state
            .measurement
            .preview_remeasure(AccountingAmount::from_cents(estimate), 400)
            .unwrap()
            .unwrap();
        state.measurement.apply_remeasure(delta).unwrap();
        assert_eq!(
            serde_json::from_value::<ContractGroupState>(serde_json::to_value(&state).unwrap())
                .unwrap(),
            state
        );
    }
    let batch = state.measurement.preview_release(265).unwrap();
    state.measurement.apply_release(batch).unwrap();
    assert_eq!(
        serde_json::from_value::<ContractGroupState>(serde_json::to_value(&state).unwrap())
            .unwrap(),
        state
    );
    let mut state = seeded();
    let batch = state.measurement.preview_release(100).unwrap();
    state.measurement.apply_release(batch).unwrap();
    let delta = state
        .measurement
        .preview_remeasure(AccountingAmount::from_cents(108_082), 400)
        .unwrap()
        .unwrap();
    state.measurement.apply_remeasure(delta).unwrap();
    let batch = state.measurement.preview_release(265).unwrap();
    state.measurement.apply_release(batch).unwrap();
    assert_eq!(state.measurement.carried_csm.units(), -145);
    assert_eq!(
        serde_json::from_value::<ContractGroupState>(serde_json::to_value(&state).unwrap())
            .unwrap(),
        state
    );
}

#[test]
fn restore_accepts_maximum_premium_with_cancelling_component_sums() {
    let mut state = seeded();
    state.premium = AccountingAmount::MAX;
    state.measurement.csm = AccountingAmount::MAX
        .sub(AccountingAmount::from_cents(81_923))
        .unwrap();
    assert_eq!(state.validate_restore(), Ok(()));
    assert_eq!(
        serde_json::from_value::<ContractGroupState>(serde_json::to_value(&state).unwrap())
            .unwrap(),
        state
    );
}

#[test]
fn restore_accepts_both_half_unit_remainder_ties() {
    for (claims, expected_carry) in [(1, 1), (3, -1)] {
        let mut state = ContractGroupState::new(
            CounterpartyId("POL".into()),
            AccountingAmount::from_cents(claims),
            AccountingAmount::from_cents(claims),
            AccountingAmount::ZERO,
            AccountingAmount::ZERO,
            AccountingAmount::ZERO,
            AccountingAmount::ZERO,
            CivilDate::from_iso("2030-01-01").unwrap(),
            CivilDate::from_iso("2030-01-03").unwrap(),
            2,
        );
        let batch = state.measurement.preview_release(1).unwrap();
        state.measurement.apply_release(batch).unwrap();
        assert_eq!(state.measurement.carried_claims.units(), expected_carry);
        assert_eq!(
            serde_json::from_value::<ContractGroupState>(serde_json::to_value(&state).unwrap())
                .unwrap(),
            state
        );
    }
}

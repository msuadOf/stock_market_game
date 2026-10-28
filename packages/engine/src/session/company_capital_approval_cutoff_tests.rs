//! 批准可在当前日保存；已执行事实仍严格服从已提交自然日截止。

use super::*;

fn before_approval() -> Fixture {
    let mut f = Fixture::new();
    // 只回置开局名册静点，不制造或删改任何已发生 movement。
    f.actions.registries[0] = ShareRegistry::new(
        code(),
        CompanyId(0),
        1_000,
        date("2030-06-02"),
        f.actions.registries[0].holdings().to_vec(),
    )
    .unwrap();
    f
}

fn approve_split(f: &mut Fixture) {
    f.system
        .declare_share_split(
            &CompanyId(0),
            ShareSplitDeclaration {
                event_id: "cutoff-split".into(),
                approval_reference: "批准-cutoff-split".into(),
                direction: ShareSplitDirection::Split,
                approved_on: date("2030-06-03"),
                ratio: 2,
                par_value_before: Money::from_cents(100),
                par_value_after: Money::from_cents(50),
                registered_capital_at_approval: AccountingAmount::from_cents(100_000),
            },
        )
        .unwrap();
    f.actions.share_splits.push(
        ShareSplitBook::new(ShareSplitEventPlan {
            event_id: "cutoff-split".into(),
            approval_reference: "批准-cutoff-split".into(),
            issuer: CompanyId(0),
            stock: code(),
            exchange: CalendarExchange::Sse,
            direction: ShareSplitDirection::Split,
            ratio: 2,
            approved_on: date("2030-06-03"),
            announced_on: date("2030-06-03"),
            registered_on: date("2030-06-03"),
            ex_rights_on: f
                .calendar
                .next_trading_day(CalendarExchange::Sse, date("2030-06-03"))
                .unwrap(),
        })
        .unwrap(),
    );
}

fn approve_repurchase(f: &mut Fixture, approved_on: CivilDate) {
    let plan = IssuerRepurchasePlan {
        event_id: "cutoff-repurchase".into(),
        approval_reference: "批准-cutoff-repurchase".into(),
        issuer: CompanyId(0),
        stock: code(),
        exchange: CalendarExchange::Sse,
        approved_on,
        announced_on: approved_on,
        window_start_on: approved_on,
        window_deadline_on: date("2030-06-04"),
        price_cap_per_share: Money::from_cents(100),
        total_budget: Money::from_cents(20_000),
        max_shares: 100,
        purpose: RepurchasePurpose::ReduceCapital,
        completion_policy: RepurchaseCompletionPolicy::CancelOnCompletion,
        approval_unit_event_id: None,
    };
    f.system
        .declare_issuer_repurchase(
            &CompanyId(0),
            IssuerRepurchaseFinanceFact {
                event_id: plan.event_id.clone(),
                approval_reference: plan.approval_reference.clone(),
                approved_on,
                synthetic_funding: AccountingAmount::from_money(plan.total_budget),
                purpose: plan.purpose.clone(),
                spent: None,
                withdrawn_remainder: None,
                completed_on: None,
                cancelled_shares: 0,
                cancelled_on: None,
                capital_reduction: None,
            },
        )
        .unwrap();
    f.actions
        .issuer_repurchases
        .push(IssuerRepurchaseBook::new(plan).unwrap());
}

fn assert_approved_without_movement(f: &Fixture) {
    let summary = f
        .validate_with_approval("2030-06-02", "2030-06-03")
        .expect("当前日只有 Approved 的方案必须可恢复");
    let company = &summary.companies[&CompanyId(0)];
    assert_eq!(company.stock, Some(StockId(0)));
    assert_eq!(company.issued_shares, 1_000);
    assert_eq!(company.receipt_frontier, 0);
    assert_eq!(
        company.registered_capital,
        Some(AccountingAmount::from_cents(100_000))
    );
    assert!(f.actions.registries[0].receipts().is_empty());
    assert_eq!(f.actions.registries[0].settled_on(), date("2030-06-02"));
}

fn assert_future_fact(
    result: Result<CapitalReplaySummary, CompanyCapitalRestoreError>,
    kind: CorporateCapitalKind,
    event: &str,
    day: &str,
    committed: &str,
) {
    assert!(
        matches!(result, Err(CompanyCapitalRestoreError::FutureFact {
        context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(actual_kind), event_id: Some(actual_event), day: Some(actual_day), .. },
        committed_through,
    }) if actual_kind == kind && actual_event == event && actual_day == date(day)
        && committed_through == date(committed))
    );
}

#[test]
fn capital_53_current_approved_rights_restores_without_shares_capital_or_receipts() {
    let mut f = before_approval();
    f.rights(0, false);
    assert_approved_without_movement(&f);
}

#[test]
fn capital_54_current_approved_stock_restores_without_shares_capital_or_receipts() {
    let mut f = before_approval();
    f.distribution("cutoff-stock", "2030-06-03", "2030-06-04", 100);
    assert_approved_without_movement(&f);
}

#[test]
fn capital_55_current_approved_split_restores_without_changing_current_units() {
    let mut f = before_approval();
    approve_split(&mut f);
    assert_approved_without_movement(&f);
}

#[test]
fn capital_56_current_approved_repurchase_still_replays_empty_inventory() {
    let mut f = before_approval();
    approve_repurchase(&mut f, date("2030-06-03"));
    assert_approved_without_movement(&f);
}

#[test]
fn capital_57_tomorrow_approved_rights_reports_approval_cutoff_and_exact_plan() {
    let mut f = before_approval();
    f.rights_approved_on(0, false, date("2030-06-04"));
    assert!(
        matches!(f.validate_with_approval("2030-06-02", "2030-06-03"),
        Err(CompanyCapitalRestoreError::FutureApproval {
            context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(CorporateCapitalKind::RightsOffering), event_id: Some(event), day: Some(day), receipt_index: None },
            approved_through,
        }) if event == "rights-a" && day == date("2030-06-04")
            && approved_through == date("2030-06-03"))
    );
}

#[test]
fn capital_58_approval_window_does_not_admit_current_zero_delta_registry_receipt() {
    let mut f = before_approval();
    f.rights(0, false);
    f.actions.registries[0]
        .close_day(ShareDayRequest {
            event_id: "cutoff-zero-market".into(),
            day: date("2030-06-03"),
            scope: MovementScope::PublicMarket,
            changes: vec![],
        })
        .unwrap();
    assert!(
        matches!(f.validate_with_approval("2030-06-02", "2030-06-03"),
        Err(CompanyCapitalRestoreError::FutureFact {
            context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(CorporateCapitalKind::PublicMarket), event_id: Some(event), receipt_index: Some(0), day: Some(day) },
            committed_through,
        }) if event == "cutoff-zero-market" && day == date("2030-06-03")
            && committed_through == date("2030-06-02"))
    );
}

#[test]
fn capital_59_approval_window_does_not_admit_current_rights_announcement() {
    let mut f = before_approval();
    f.rights(0, false);
    f.actions.rights_offerings[0]
        .announce(date("2030-06-03"))
        .unwrap();
    assert_future_fact(
        f.validate_with_approval("2030-06-02", "2030-06-03"),
        CorporateCapitalKind::RightsOffering,
        "rights-a",
        "2030-06-03",
        "2030-06-02",
    );
}

#[test]
fn capital_60_approval_window_does_not_admit_current_stock_announcement() {
    let mut f = before_approval();
    f.distribution("cutoff-stock", "2030-06-03", "2030-06-04", 100);
    f.actions.stock_distributions[0]
        .announce(date("2030-06-03"))
        .unwrap();
    assert_future_fact(
        f.validate_with_approval("2030-06-02", "2030-06-03"),
        CorporateCapitalKind::StockDistribution,
        "cutoff-stock",
        "2030-06-03",
        "2030-06-02",
    );
}

#[test]
fn capital_61_approval_window_does_not_admit_current_split_announcement() {
    let mut f = before_approval();
    approve_split(&mut f);
    f.actions.share_splits[0]
        .announce(date("2030-06-03"))
        .unwrap();
    assert_future_fact(
        f.validate_with_approval("2030-06-02", "2030-06-03"),
        CorporateCapitalKind::ShareReDenomination,
        "cutoff-split",
        "2030-06-03",
        "2030-06-02",
    );
}

#[test]
fn capital_62_approval_window_does_not_admit_current_repurchase_announcement() {
    let mut f = before_approval();
    approve_repurchase(&mut f, date("2030-06-03"));
    f.actions.issuer_repurchases[0]
        .announce(date("2030-06-03"))
        .unwrap();
    assert_future_fact(
        f.validate_with_approval("2030-06-02", "2030-06-03"),
        CorporateCapitalKind::IssuerRepurchaseCancellation,
        "cutoff-repurchase",
        "2030-06-03",
        "2030-06-02",
    );
}

#[test]
fn capital_63_approval_window_does_not_admit_current_repurchase_order_marker() {
    let mut f = before_approval();
    approve_repurchase(&mut f, date("2030-06-03"));
    f.actions.issuer_repurchases[0].set_last_order_day(date("2030-06-03"));
    assert_future_fact(
        f.validate_with_approval("2030-06-02", "2030-06-03"),
        CorporateCapitalKind::IssuerRepurchaseCancellation,
        "cutoff-repurchase",
        "2030-06-03",
        "2030-06-02",
    );
}

#[test]
fn capital_64_approval_window_does_not_admit_current_registration() {
    let mut f = Fixture::new();
    let ex = f
        .calendar
        .next_trading_day(CalendarExchange::Sse, date("2030-06-04"))
        .unwrap();
    f.distribution("cutoff-register", "2030-06-04", &ex.to_string(), 100);
    let registry = &mut f.actions.registries[0];
    registry
        .close_day(ShareDayRequest {
            event_id: "market-2030-06-04".into(),
            day: date("2030-06-04"),
            scope: MovementScope::PublicMarket,
            changes: vec![],
        })
        .unwrap();
    let snapshot = registry
        .register("cutoff-register".into(), date("2030-06-04"))
        .unwrap()
        .clone();
    let book = &mut f.actions.stock_distributions[0];
    book.announce(date("2030-06-03")).unwrap();
    book.register(snapshot, &f.calendar).unwrap();
    assert_future_fact(
        f.validate_with_approval("2030-06-03", "2030-06-04"),
        CorporateCapitalKind::StockDistribution,
        "cutoff-register",
        "2030-06-04",
        "2030-06-03",
    );
}

#[test]
fn capital_65_approval_window_does_not_admit_current_actual_stock_credit() {
    let mut f = Fixture::new();
    f.distribution("cutoff-credit", "2030-06-03", "2030-06-04", 100);
    f.credit_distribution();
    assert_future_fact(
        f.validate_with_approval("2030-06-03", "2030-06-04"),
        CorporateCapitalKind::StockDistribution,
        "cutoff-credit",
        "2030-06-04",
        "2030-06-03",
    );
}

#[test]
fn capital_66_approval_window_does_not_admit_current_actual_repurchase_fill() {
    let mut f = Fixture::new();
    f.repurchase(false, false);
    assert_future_fact(
        f.validate_with_approval("2030-06-03", "2030-06-04"),
        CorporateCapitalKind::IssuerRepurchaseCancellation,
        "buy-a",
        "2030-06-04",
        "2030-06-03",
    );
}

#[test]
fn capital_67_approval_window_does_not_admit_current_repurchase_completion() {
    let mut f = Fixture::new();
    f.repurchase(true, false);
    assert_future_fact(
        f.validate_with_approval("2030-06-03", "2030-06-04"),
        CorporateCapitalKind::IssuerRepurchaseCancellation,
        "buy-a",
        "2030-06-04",
        "2030-06-03",
    );
}

#[test]
fn capital_68_tomorrow_approved_repurchase_reports_exact_approval_cutoff() {
    let mut f = before_approval();
    approve_repurchase(&mut f, date("2030-06-04"));
    assert!(
        matches!(f.validate_with_approval("2030-06-02", "2030-06-03"),
        Err(CompanyCapitalRestoreError::FutureApproval {
            context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(CorporateCapitalKind::IssuerRepurchaseCancellation), event_id: Some(event), day: Some(day), receipt_index: None },
            approved_through,
        }) if event == "cutoff-repurchase" && day == date("2030-06-04")
            && approved_through == date("2030-06-03"))
    );
}

#[test]
fn capital_69_tomorrow_approved_stock_reports_exact_approval_cutoff() {
    let mut f = before_approval();
    f.distribution("cutoff-stock", "2030-06-03", "2030-06-04", 100);
    assert!(
        matches!(f.validate_with_approval("2030-06-02", "2030-06-02"),
        Err(CompanyCapitalRestoreError::FutureApproval {
            context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(CorporateCapitalKind::StockDistribution), event_id: Some(event), day: Some(day), receipt_index: None },
            approved_through,
        }) if event == "cutoff-stock" && day == date("2030-06-03")
            && approved_through == date("2030-06-02"))
    );
}

#[test]
fn capital_70_tomorrow_approved_split_reports_exact_approval_cutoff() {
    let mut f = before_approval();
    approve_split(&mut f);
    assert!(
        matches!(f.validate_with_approval("2030-06-02", "2030-06-02"),
        Err(CompanyCapitalRestoreError::FutureApproval {
            context: CapitalRestoreContext { company: Some(CompanyId(0)), stock: Some(StockId(0)), kind: Some(CorporateCapitalKind::ShareReDenomination), event_id: Some(event), day: Some(day), receipt_index: None },
            approved_through,
        }) if event == "cutoff-split" && day == date("2030-06-03")
            && approved_through == date("2030-06-02"))
    );
}

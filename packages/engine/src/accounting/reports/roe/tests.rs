use super::*;
use crate::accounting::{
    BusinessEventId, CashFlowClass, JournalEntry, JournalLine, LedgerAccountId, MemberId,
    PostingSide,
};

fn date(month: u8, day: u8) -> CivilDate {
    CivilDate::from_ymd(2030, month, day).unwrap()
}

fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}

fn books() -> Books {
    let mut books = Books::new(crate::company::industrial::industrial_account_chart());
    books
        .post_batch(vec![JournalEntry {
            source: BusinessEventId::new(1),
            date: date(1, 1).prev().unwrap(),
            kind: BusinessKind::OpeningBalance,
            cash_flow: CashFlowClass::Financing,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("1002".into()),
                    side: PostingSide::Debit,
                    amount: amount(10_000),
                },
                JournalLine {
                    account: LedgerAccountId("4001".into()),
                    side: PostingSide::Credit,
                    amount: amount(10_000),
                },
            ],
        }])
        .unwrap();
    books
}

fn input_period() -> DisclosureReportPeriod {
    DisclosureReportPeriod {
        start: date(1, 1),
        end: date(3, 31),
    }
}

#[test]
fn dividend_declaration_is_weighted_from_the_following_month() {
    let mut books = books();
    books
        .post_batch(vec![JournalEntry {
            source: BusinessEventId::new(2),
            date: date(1, 15),
            kind: BusinessKind::CompanyDividendDeclaration,
            cash_flow: CashFlowClass::NonCash,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("4103".into()),
                    side: PostingSide::Debit,
                    amount: amount(1_000),
                },
                JournalLine {
                    account: LedgerAccountId("2202".into()),
                    side: PostingSide::Credit,
                    amount: amount(1_000),
                },
            ],
        }])
        .unwrap();
    let scope = ScopeId::Standalone(MemberId("issuer".into()));

    let report = calculate_report_roe(
        Some(&books),
        &scope,
        input_period(),
        amount(10_000),
        amount(3_000),
        false,
    )
    .unwrap();

    assert_eq!(
        report.weighted_average_parent_equity_cents,
        ReportRoeValue::Available(ReportRational {
            numerator: "65000".into(),
            denominator: "6".into(),
        })
    );
    assert_eq!(
        report.ordinary_roe,
        ReportRoeValue::Available(ReportRational {
            numerator: "18000".into(),
            denominator: "65000".into(),
        })
    );
    assert_eq!(
        report.adjusted_roe,
        ReportRoeValue::Unavailable {
            reason: ReportRoeUnavailable::MissingNonRecurringIncomeFacts,
        }
    );
}

#[test]
fn equity_posting_without_a_supported_business_kind_is_not_ignored() {
    let mut books = books();
    books
        .post_batch(vec![JournalEntry {
            source: BusinessEventId::new(2),
            date: date(1, 15),
            kind: BusinessKind::CashRevenue,
            cash_flow: CashFlowClass::Operating,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("1002".into()),
                    side: PostingSide::Debit,
                    amount: amount(1_000),
                },
                JournalLine {
                    account: LedgerAccountId("4001".into()),
                    side: PostingSide::Credit,
                    amount: amount(1_000),
                },
            ],
        }])
        .unwrap();
    let scope = ScopeId::Standalone(MemberId("issuer".into()));

    let report = calculate_report_roe(
        Some(&books),
        &scope,
        input_period(),
        amount(10_000),
        amount(0),
        false,
    )
    .unwrap();

    assert_eq!(
        report.basis,
        ReportRoeBasis::Unsupported {
            reason: ReportRoeUnavailable::UnclassifiedEquityEvent,
        }
    );
}

fn generated_report(
    books: &Books,
    period: crate::accounting::AccountingPeriod,
    kind: crate::accounting::reports::ReportKind,
    adjustments: &std::collections::BTreeMap<BusinessEventId, crate::accounting::AccountingPeriod>,
) -> crate::accounting::reports::ReportSet {
    use crate::accounting::reports::{
        generate_report_set, IndustryPresentation, ReportRequest, ReportSource, ReportVersion,
        VersionKind,
    };
    let report = generate_report_set(ReportRequest {
        period,
        kind,
        source: ReportSource::Standalone {
            id: MemberId("issuer".into()),
            books,
            industry: IndustryPresentation::Industrial,
        },
        version: ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments,
    })
    .unwrap();
    report.validate().unwrap();
    report
}

fn books_with_later_cash_adjustment() -> Books {
    let mut books = books();
    books
        .post_batch(vec![JournalEntry {
            source: BusinessEventId::new(2),
            date: CivilDate::from_ymd(2031, 2, 1).unwrap(),
            kind: BusinessKind::CashRevenue,
            cash_flow: CashFlowClass::Operating,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("1002".into()),
                    side: PostingSide::Debit,
                    amount: amount(1_000),
                },
                JournalLine {
                    account: LedgerAccountId("6001".into()),
                    side: PostingSide::Credit,
                    amount: amount(1_000),
                },
            ],
        }])
        .unwrap();
    books
}

#[test]
fn later_effective_restatements_leave_all_earlier_report_kinds_unchanged() {
    use crate::accounting::AccountingPeriod;
    use crate::accounting::reports::ReportKind;
    use std::collections::BTreeMap;

    let original_books = books();
    let corrected_books = books_with_later_cash_adjustment();
    for (period, kind, target) in [
        ("2030-01", ReportKind::Monthly, "2030-02"),
        ("2030-03", ReportKind::Quarter, "2030-04"),
        ("2030-06", ReportKind::HalfYear, "2030-07"),
        ("2030-12", ReportKind::Annual, "2031-01"),
    ] {
        let period = AccountingPeriod::from_iso(period).unwrap();
        let original = generated_report(&original_books, period, kind, &BTreeMap::new());
        assert!(matches!(original.roe.ordinary_roe, ReportRoeValue::Available(_)));
        let adjustments = BTreeMap::from([(
            BusinessEventId::new(2),
            AccountingPeriod::from_iso(target).unwrap(),
        )]);
        let rebuilt = generated_report(&corrected_books, period, kind, &adjustments);
        assert_eq!(rebuilt, original, "{period} {kind:?} 不受更后有效期间影响");
    }
}

#[test]
fn restatements_at_or_before_report_end_keep_equity_history_unavailable() {
    use crate::accounting::AccountingPeriod;
    use crate::accounting::reports::ReportKind;
    use std::collections::BTreeMap;

    let books = books_with_later_cash_adjustment();
    let period = AccountingPeriod::from_iso("2030-06").unwrap();
    for target in ["2029-11", "2030-01", "2030-04", "2030-06"] {
        let target = AccountingPeriod::from_iso(target).unwrap();
        let adjustments = BTreeMap::from([(BusinessEventId::new(2), target)]);
        let report = generated_report(&books, period, ReportKind::Quarter, &adjustments);
        let unavailable = ReportRoeValue::Unavailable {
            reason: ReportRoeUnavailable::IncompleteEquityEventHistory,
        };
        assert_eq!(report.roe.basis, ReportRoeBasis::Unsupported {
            reason: ReportRoeUnavailable::IncompleteEquityEventHistory,
        });
        assert_eq!(report.roe.ordinary_roe, unavailable);
        assert_eq!(report.roe.adjusted_roe, unavailable);
        assert_eq!(report.roe.weighted_average_parent_equity_cents, unavailable);
        let before_window = target < AccountingPeriod::from_iso("2030-04").unwrap();
        assert_eq!(report.equity.opening_parent, amount(if before_window { 11_000 } else { 10_000 }));
        assert_eq!(report.income.report_period.net_income, amount(if before_window { 0 } else { 1_000 }));
        assert_eq!(report.cash_flow.operating, amount(0), "现金不回写有效期间");
    }
}

#[test]
fn consolidated_sources_reject_unnamespaced_adjustments_even_after_report_end() {
    use crate::accounting::AccountingPeriod;
    use crate::accounting::consolidation::{ConsolidationRequest, GroupMember, MemberSpec};
    use crate::accounting::reports::{
        generate_report_set, ReportError, ReportKind, ReportRequest, ReportSource, ReportVersion,
        VersionKind,
    };
    use std::collections::BTreeMap;

    let books = books_with_later_cash_adjustment();
    let group = || ConsolidationRequest {
        root: MemberId("root".into()),
        members: vec![
            GroupMember {
                spec: MemberSpec {
                    id: MemberId("root".into()),
                    group_parent: None,
                    issued_shares: 100,
                    parent_held_shares: 0,
                },
                books: &books,
            },
            GroupMember {
                spec: MemberSpec {
                    id: MemberId("sub".into()),
                    group_parent: Some(MemberId("root".into())),
                    issued_shares: 100,
                    parent_held_shares: 80,
                },
                books: &books,
            },
        ],
        intercompany_balances: vec![],
        intercompany_sales: vec![],
    };
    let adjustments = BTreeMap::from([(
        BusinessEventId::new(2),
        AccountingPeriod::from_iso("2031-01").unwrap(),
    )]);
    let member_adjustments = BTreeMap::new();
    let no_adjustments = BTreeMap::new();
    for restated_source in [false, true] {
        let source = || if restated_source {
            ReportSource::ConsolidatedRestated { request: group(), member_adjustments: &member_adjustments }
        } else {
            ReportSource::Consolidated { request: group() }
        };
        let request = |adjustments| ReportRequest {
            period: AccountingPeriod::from_iso("2030-01").unwrap(),
            kind: ReportKind::Monthly,
            source: source(),
            version: ReportVersion { sequence: 1, supersedes: None, kind: VersionKind::Original },
            adjustments,
        };
        let legal = generate_report_set(request(&no_adjustments)).unwrap();
        legal.validate().unwrap();
        assert_eq!(legal.roe.basis, ReportRoeBasis::Unsupported {
            reason: ReportRoeUnavailable::ConsolidatedAttributionFactsUnavailable,
        });
        let error = generate_report_set(request(&adjustments)).unwrap_err();
        if restated_source {
            assert!(matches!(error, ReportError::InternalWindowInconsistent { detail }
                if detail.contains("成员命名空间")));
        } else {
            assert!(matches!(error, ReportError::ConsolidatedRestatementUnsupported));
        }
    }
}

#[test]
fn final_supported_month_has_a_representable_month_end() {
    let december = crate::accounting::AccountingPeriod::from_ymd(2199, 12).unwrap();

    assert_eq!(
        super::period_dates(december, december).unwrap(),
        DisclosureReportPeriod {
            start: CivilDate::from_ymd(2199, 12, 1).unwrap(),
            end: CivilDate::from_ymd(2199, 12, 31).unwrap(),
        }
    );
}

use crate::accounting::consolidation::{ConsolidationRequest, GroupMember, MemberId, MemberSpec};
use crate::accounting::reports::{
    generate_report_set, IndustryPresentation, ReportKind, ReportRequest, ReportSource,
    ReportVersion, VersionKind,
};
use crate::accounting::{
    AccountingAmount, AccountingPeriod, Books, BusinessEventId, BusinessKind, CashFlowClass,
    JournalEntry, JournalLine, LedgerAccountId, PostingSide,
};
use crate::calendar::{CivilDate, CivilInstant};
use crate::company::CompanyId;
use crate::information::{AccountingPolicyRef, PublicationId, PublicationOrigin, PublishedReport};
use std::collections::BTreeMap;

fn assert_same_window_growth(kind: ReportKind, month: u8, consolidated: bool) {
    let mut books = Books::new(crate::company::industrial::industrial_account_chart());
    let mut source = 0;
    for (year, months, amount) in [
        (2028, vec![12], 100000),
        (2029, vec![1, 2, 5, 9], 1000),
        (2030, vec![1, 2, 5, 9], 1100),
    ] {
        for event_month in months {
            source += 1;
            books
                .post_batch(vec![JournalEntry {
                    source: BusinessEventId::new(source),
                    date: CivilDate::from_ymd(year, event_month, 15).unwrap(),
                    kind: if source == 1 {
                        BusinessKind::OpeningBalance
                    } else {
                        BusinessKind::CashRevenue
                    },
                    cash_flow: if source == 1 {
                        CashFlowClass::Financing
                    } else {
                        CashFlowClass::Operating
                    },
                    lines: vec![
                        JournalLine {
                            account: LedgerAccountId("1002".into()),
                            side: PostingSide::Debit,
                            amount: AccountingAmount::from_cents(amount),
                        },
                        JournalLine {
                            account: LedgerAccountId(
                                if source == 1 { "4001" } else { "6001" }.into(),
                            ),
                            side: PostingSide::Credit,
                            amount: AccountingAmount::from_cents(amount),
                        },
                    ],
                }])
                .unwrap();
        }
    }
    let root = MemberId("company".into());
    let child_books = books.clone();
    let report_source = if consolidated {
        ReportSource::Consolidated {
            request: ConsolidationRequest {
                root: root.clone(),
                members: vec![
                    GroupMember {
                        spec: MemberSpec {
                            id: root.clone(),
                            group_parent: None,
                            issued_shares: 100,
                            parent_held_shares: 0,
                        },
                        books: &books,
                    },
                    GroupMember {
                        spec: MemberSpec {
                            id: MemberId("child".into()),
                            group_parent: Some(root.clone()),
                            issued_shares: 100,
                            parent_held_shares: 80,
                        },
                        books: &child_books,
                    },
                ],
                intercompany_balances: vec![],
                intercompany_sales: vec![],
            },
        }
    } else {
        ReportSource::Standalone {
            id: root,
            books: &books,
            industry: IndustryPresentation::Industrial,
        }
    };
    let reports = generate_report_set(ReportRequest {
        period: AccountingPeriod::from_ymd(2030, month).unwrap(),
        kind,
        source: report_source,
        version: ReportVersion {
            sequence: 2,
            supersedes: Some(1),
            kind: VersionKind::Correction {
                reason: "同窗同比纯抽取测试".into(),
            },
        },
        adjustments: &BTreeMap::new(),
    })
    .unwrap();
    reports.validate().unwrap();
    let published_at =
        CivilInstant::from_hms(CivilDate::from_ymd(2031, 3, 20).unwrap(), 18, 0, 0).unwrap();
    let published = PublishedReport {
        id: PublicationId::new(2),
        company: CompanyId("company".into()),
        source: crate::information::PublicationSource::SimulationAccounting,
        policy: AccountingPolicyRef { chart_version: 2 },
        approved_at: CivilInstant::from_hms(published_at.date(), 8, 0, 0).unwrap(),
        published_at,
        origin: PublicationOrigin::Correction,
        supersedes: Some(PublicationId::new(1)),
        reports,
    };
    assert_eq!(
        super::facts::extract_interim_growth(&published, &published.company, published_at).unwrap(),
        super::GrowthObservation::TwoYear(1000)
    );
}

#[test]
fn february_monthly_growth_compares_actual_same_month_not_year_to_date() {
    assert_same_window_growth(ReportKind::Monthly, 2, false);
}

#[test]
fn consolidated_february_monthly_growth_uses_main_notes_without_double_counting_split() {
    assert_same_window_growth(ReportKind::Monthly, 2, true);
}

#[test]
fn third_quarter_growth_compares_actual_same_quarter_not_year_to_date() {
    assert_same_window_growth(ReportKind::Quarter, 9, false);
}

#[test]
fn consolidated_third_quarter_growth_keeps_same_report_scope_and_window() {
    assert_same_window_growth(ReportKind::Quarter, 9, true);
}

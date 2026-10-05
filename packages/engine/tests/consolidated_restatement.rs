use std::collections::BTreeMap;

use engine::accounting::consolidation::{ConsolidationRequest, GroupMember, MemberId, MemberSpec};
use engine::accounting::reports::{
    generate_report_set, ReportKind, ReportRequest, ReportSource, ReportVersion, VersionKind,
};
use engine::accounting::{
    AccountChart, AccountingAmount, AccountingPeriod, Books, BusinessEventId, BusinessKind,
    CashFlowClass, JournalEntry, JournalLine, LedgerAccountId, PostingSide,
};
use engine::calendar::CivilDate;

fn entry(source: u64, date: &str, debit: &str, credit: &str, amount: i128) -> JournalEntry {
    JournalEntry {
        source: BusinessEventId::new(source),
        date: CivilDate::from_iso(date).unwrap(),
        kind: if source == 1 {
            BusinessKind::OpeningBalance
        } else if credit == "6001" {
            BusinessKind::CashRevenue
        } else {
            BusinessKind::CashExpense
        },
        cash_flow: if source == 1 {
            CashFlowClass::Financing
        } else {
            CashFlowClass::Operating
        },
        lines: vec![
            JournalLine {
                account: LedgerAccountId(debit.into()),
                side: PostingSide::Debit,
                amount: AccountingAmount::from_cents(amount),
            },
            JournalLine {
                account: LedgerAccountId(credit.into()),
                side: PostingSide::Credit,
                amount: AccountingAmount::from_cents(amount),
            },
        ],
    }
}

fn member<'a>(id: &str, parent: Option<&str>, books: &'a Books) -> GroupMember<'a> {
    GroupMember {
        spec: MemberSpec {
            id: MemberId(id.into()),
            group_parent: parent.map(|id| MemberId(id.into())),
            issued_shares: 100,
            parent_held_shares: if parent.is_some() { 80 } else { 0 },
        },
        books,
    }
}

#[test]
fn member_local_sources_restate_profit_equity_without_backdating_cash() {
    let mut root = Books::new(AccountChart::generic_account_chart());
    let mut child = Books::new(AccountChart::generic_account_chart());
    for books in [&mut root, &mut child] {
        books
            .post_batch(vec![
                entry(1, "2029-12-31", "1002", "4001", 10000),
                entry(2, "2031-01-01", "6602", "1002", 1000),
            ])
            .unwrap();
    }
    let root_map = BTreeMap::from([(
        BusinessEventId::new(2),
        AccountingPeriod::from_iso("2030-01").unwrap(),
    )]);
    let child_map = BTreeMap::from([(
        BusinessEventId::new(2),
        AccountingPeriod::from_iso("2030-02").unwrap(),
    )]);
    let maps = BTreeMap::from([
        (MemberId("root".into()), &root_map),
        (MemberId("child".into()), &child_map),
    ]);
    for (month, income, minority, equity) in [
        ("2030-01", -1000, 2000, 19000),
        ("2030-02", -2000, 1800, 18000),
        ("2031-01", 0, 1800, 18000),
    ] {
        let report = generate_report_set(ReportRequest {
            period: AccountingPeriod::from_iso(month).unwrap(),
            kind: ReportKind::Monthly,
            source: ReportSource::ConsolidatedRestated {
                request: ConsolidationRequest {
                    root: MemberId("root".into()),
                    members: vec![
                        member("root", None, &root),
                        member("child", Some("root"), &child),
                    ],
                    intercompany_balances: vec![],
                    intercompany_sales: vec![],
                },
                member_adjustments: &maps,
            },
            version: ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: VersionKind::Original,
            },
            adjustments: &BTreeMap::new(),
        })
        .unwrap();
        report.validate().unwrap();
        assert_eq!(report.income.cumulative.net_income.cents(), income);
        assert_eq!(report.balance_sheet.total_equity.cents(), equity);
        assert!(report.balance_sheet.equity_lines.contains(&(
            engine::accounting::reports::BsLine::MinorityEquity,
            AccountingAmount::from_cents(minority)
        )));
        assert_eq!(
            report.cash_flow.operating.cents(),
            if month == "2031-01" { -2000 } else { 0 }
        );
        assert_eq!(
            report.cash_flow.closing_cash.cents(),
            if month == "2031-01" { 18000 } else { 20000 }
        );
        if month == "2031-01" {
            let engine::accounting::reports::Comparative::Available(prior) =
                &report.income.prior_year
            else {
                panic!("已有历史更正必须产生真实比较期");
            };
            assert_eq!(prior.net_income.cents(), -1000);
            let engine::accounting::reports::Comparative::Available(prior) =
                &report.balance_sheet.prior_year_end
            else {
                panic!("已有历史必须有上年年末余额");
            };
            assert!(prior.contains(&(
                engine::accounting::reports::BsLine::MinorityEquity,
                AccountingAmount::from_cents(1800)
            )));
        }
    }
    assert_eq!(
        root.journal().entries().last().unwrap().date,
        CivilDate::from_iso("2031-01-01").unwrap()
    );
    assert_eq!(
        child.journal().entries().last().unwrap().source,
        BusinessEventId::new(2)
    );
}

#[test]
fn historical_projection_does_not_revalidate_actual_payment_against_past_cash() {
    let mut root = Books::new(AccountChart::generic_account_chart());
    let mut child = Books::new(AccountChart::generic_account_chart());
    for books in [&mut root, &mut child] {
        books
            .post_batch(vec![
                entry(1, "2029-12-31", "1002", "4001", 1000),
                entry(2, "2031-01-01", "1002", "6001", 2000),
                entry(3, "2031-01-02", "6602", "1002", 1500),
            ])
            .unwrap();
    }
    let snapshot = serde_json::to_value((&root, &child)).unwrap();
    let map = BTreeMap::from([(
        BusinessEventId::new(3),
        AccountingPeriod::from_iso("2030-01").unwrap(),
    )]);
    let maps = BTreeMap::from([
        (MemberId("root".into()), &map),
        (MemberId("child".into()), &map),
    ]);
    let report = generate_report_set(ReportRequest {
        period: AccountingPeriod::from_iso("2030-01").unwrap(),
        kind: ReportKind::Monthly,
        source: ReportSource::ConsolidatedRestated {
            request: ConsolidationRequest {
                root: MemberId("root".into()),
                members: vec![
                    member("root", None, &root),
                    member("child", Some("root"), &child),
                ],
                intercompany_balances: vec![],
                intercompany_sales: vec![],
            },
            member_adjustments: &maps,
        },
        version: ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments: &BTreeMap::new(),
    })
    .unwrap();
    report.validate().unwrap();
    assert_eq!(report.income.cumulative.net_income.cents(), -3000);
    assert_eq!(report.cash_flow.operating, AccountingAmount::ZERO);
    assert_eq!(report.cash_flow.closing_cash.cents(), 2000);
    assert_eq!(root.ledger().cash_total().unwrap().cents(), 1500);
    assert_eq!(serde_json::to_value((&root, &child)).unwrap(), snapshot);
}

#[test]
fn member_restatement_rejects_unknown_owner_source_and_nonhistorical_period() {
    let mut root = Books::new(AccountChart::generic_account_chart());
    let mut child = Books::new(AccountChart::generic_account_chart());
    for books in [&mut root, &mut child] {
        books
            .post_batch(vec![entry(1, "2029-12-31", "1002", "4001", 1000)])
            .unwrap();
    }
    for (owner, source, period, expected) in [
        ("outside", 1, "2029-01", "未知成员"),
        ("root", 9, "2029-01", "未知来源"),
        ("root", 1, "2029-12", "早于实际期间"),
    ] {
        let map = BTreeMap::from([(
            BusinessEventId::new(source),
            AccountingPeriod::from_iso(period).unwrap(),
        )]);
        let maps = BTreeMap::from([(MemberId(owner.into()), &map)]);
        let error = generate_report_set(ReportRequest {
            period: AccountingPeriod::from_iso("2030-01").unwrap(),
            kind: ReportKind::Monthly,
            source: ReportSource::ConsolidatedRestated {
                request: ConsolidationRequest {
                    root: MemberId("root".into()),
                    members: vec![
                        member("root", None, &root),
                        member("child", Some("root"), &child),
                    ],
                    intercompany_balances: vec![],
                    intercompany_sales: vec![],
                },
                member_adjustments: &maps,
            },
            version: ReportVersion {
                sequence: 1,
                supersedes: None,
                kind: VersionKind::Original,
            },
            adjustments: &BTreeMap::new(),
        })
        .unwrap_err();
        assert!(
            matches!(error, engine::accounting::reports::ReportError::InternalWindowInconsistent { ref detail } if detail.contains(expected))
        );
    }
}

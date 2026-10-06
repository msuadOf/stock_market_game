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

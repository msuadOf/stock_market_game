use engine::accounting::{
    AccountingAmount, BusinessEventId, BusinessKind, CashFlowClass, IncomeTaxPolicy, JournalEntry,
    JournalLine, LedgerAccountId, PostingSide, TaxPolicy, VatPolicy,
};
use engine::calendar::CivilDate;
use engine::company::industrial::{
    industrial_account_chart, ExpenseKind, IndustrialBooks, IndustrialConfig,
};
use engine::company::OperatingBudget;

fn main() {
    let date = |iso| CivilDate::from_iso(iso).unwrap();
    let amount = |cents| AccountingAmount::from_cents(cents);
    let mut books = IndustrialBooks::new(IndustrialConfig {
        chart: industrial_account_chart(),
        as_of: date("2029-12-31"),
        opening_lines: vec![
            JournalLine {
                account: LedgerAccountId("1002".into()),
                side: PostingSide::Debit,
                amount: amount(10000),
            },
            JournalLine {
                account: LedgerAccountId("4001".into()),
                side: PostingSide::Credit,
                amount: amount(10000),
            },
        ],
        opening_inventory: vec![],
        opening_assets: vec![],
        opening_debt: None,
        counterparties: vec![],
        budget: OperatingBudget::new(amount(0), vec![]).unwrap(),
        tax_policy: TaxPolicy {
            version: 1,
            vat: VatPolicy {
                output_rate_bp: 1300,
                input_rate_bp: 1300,
                deductible_share_bp: 10000,
            },
            income_tax: IncomeTaxPolicy {
                rate_bp: 2500,
                loss_carryforward_years: 5,
            },
        },
    })
    .unwrap();
    books
        .books_mut()
        .post_batch(vec![JournalEntry {
            source: BusinessEventId::new(9000),
            date: date("2030-03-31"),
            kind: BusinessKind::CashRevenue,
            cash_flow: CashFlowClass::Operating,
            lines: vec![
                JournalLine {
                    account: LedgerAccountId("1002".into()),
                    side: PostingSide::Debit,
                    amount: amount(800),
                },
                JournalLine {
                    account: LedgerAccountId("6001".into()),
                    side: PostingSide::Credit,
                    amount: amount(800),
                },
            ],
        }])
        .unwrap();
    books.accrue_income_tax(date("2030-03-31")).unwrap();
    books
        .pay_income_tax(amount(200), date("2030-04-01"))
        .unwrap();
    books
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-06-01"))
        .unwrap();
    let cash = books
        .books()
        .ledger()
        .account_net_debit(&LedgerAccountId("1002".into()))
        .unwrap();
    books.accrue_income_tax(date("2030-06-30")).unwrap();
    assert_eq!(
        books
            .books()
            .ledger()
            .account_net_debit(&LedgerAccountId("1002".into()))
            .unwrap(),
        cash
    );
    use engine::accounting::reports::*;
    let report = generate_report_set(ReportRequest {
        period: engine::accounting::AccountingPeriod::from_ymd(2030, 6).unwrap(),
        kind: ReportKind::Monthly,
        source: ReportSource::Standalone {
            id: engine::accounting::consolidation::MemberId("TAX".into()),
            books: books.books(),
            industry: IndustryPresentation::Industrial,
        },
        version: ReportVersion {
            sequence: 1,
            supersedes: None,
            kind: VersionKind::Original,
        },
        adjustments: &std::collections::BTreeMap::new(),
    })
    .unwrap();
    assert!(report
        .balance_sheet
        .asset_lines
        .iter()
        .any(|(line, value)| format!("{line:?}") == "CurrentTaxAssets" && *value == amount(100)));
    report.validate().unwrap();
}

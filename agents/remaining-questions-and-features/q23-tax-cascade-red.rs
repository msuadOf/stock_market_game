use engine::accounting::{
    AccountingAmount, IncomeTaxPolicy, JournalLine, LedgerAccountId, PostingSide, TaxPolicy,
    VatPolicy,
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
        .pay_expense(ExpenseKind::Admin, amount(400), date("2030-01-01"))
        .unwrap();
    books.accrue_income_tax(date("2030-12-31")).unwrap();
    books.accrue_income_tax(date("2031-12-31")).unwrap();
    let before = books.clone();
    assert_eq!(
        books.accrue_income_tax(date("2030-12-31")).unwrap().event,
        None
    );
    assert_eq!(books, before);
}

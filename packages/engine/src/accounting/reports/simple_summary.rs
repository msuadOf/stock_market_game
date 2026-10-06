use super::{notes::a, Assignment, BsLine, IncomeLine, NoteTarget};
use crate::accounting::{
    AccountChart, AccountDef, AccountElement, AccountingError, LedgerAccountId,
};

pub const RECEIVABLE: &str = "simple_receivable";
pub const PAYABLE: &str = "simple_payable";
pub const REVENUE: &str = "simple_revenue";
pub const FIXED_EXPENSE: &str = "simple_fixed_expense";
pub const VARIABLE_EXPENSE: &str = "simple_variable_expense";
pub const DIVIDEND_PAYABLE: &str = "simple_dividend_payable";
pub const STATUTORY_RESERVE: &str = "simple_statutory_reserve";
pub const DIVIDEND_SETTLEMENT_ASSET: &str = "simple_dividend_settlement_asset";

pub fn account_chart(base: AccountChart) -> Result<AccountChart, AccountingError> {
    use AccountElement::*;
    let mut accounts: Vec<_> = base
        .iter()
        .map(|(id, definition)| (id.clone(), definition.clone()))
        .collect();
    for (code, name, element) in [
        (RECEIVABLE, "Simple 汇总应收", Asset),
        (PAYABLE, "Simple 汇总应付", Liability),
        (REVENUE, "Simple 汇总营业收入", Revenue),
        (FIXED_EXPENSE, "Simple 汇总固定费用", Expense),
        (VARIABLE_EXPENSE, "Simple 汇总变动费用", Expense),
        (DIVIDEND_PAYABLE, "应付股利", Liability),
        (STATUTORY_RESERVE, "法定公积金", Equity),
        (DIVIDEND_SETTLEMENT_ASSET, "Simple 分红账面资产调整（非现金）", Asset),
    ] {
        accounts.push((LedgerAccountId(code.into()), AccountDef::new(name, element)));
    }
    AccountChart::new(base.version(), accounts)
}

pub(crate) fn assignments() -> Vec<Assignment> {
    vec![
        a(RECEIVABLE, NoteTarget::BalanceSheet(BsLine::Receivables)),
        a(PAYABLE, NoteTarget::BalanceSheet(BsLine::AccountsPayable)),
        a(REVENUE, NoteTarget::Income(IncomeLine::OperatingRevenue)),
        a(
            FIXED_EXPENSE,
            NoteTarget::Income(IncomeLine::AdministrativeExpense),
        ),
        a(
            VARIABLE_EXPENSE,
            NoteTarget::Income(IncomeLine::OperatingCost),
        ),
        a(DIVIDEND_PAYABLE, NoteTarget::BalanceSheet(BsLine::AccountsPayable)),
        a(STATUTORY_RESERVE, NoteTarget::BalanceSheet(BsLine::StatutoryReserve)),
        a(DIVIDEND_SETTLEMENT_ASSET, NoteTarget::BalanceSheet(BsLine::SimpleDividendSettlementAdjustment)),
    ]
}

//! 公司领域入口关账的真实行为负控（架构项 #15）：Session 不参与账套细节。

use super::fixtures::*;
use engine::accounting::closing::ClosingEngine;
use engine::accounting::consolidation::{MemberId, ScopeId};
use engine::accounting::reports::ReportKind;
use engine::accounting::{AccountingPeriod, PeriodStatus};
use engine::company::operations::CompanyOperations;
use engine::company::CompanyId;

#[test]
fn company_domain_month_close_keeps_industrial_cash_debt_and_expenses_reconciled() {
    let date = d("2030-01-31");
    let period = AccountingPeriod::from_ymd(2030, 1).unwrap();
    let mut ops = CompanyOperations::new(four_company_config(21, quiet_params(), date.prev().unwrap()), date).unwrap();
    ops.advance_civil_day(date).unwrap();
    let company = ops.catalog().id("C-IND-A").unwrap();
    let before = serde_json::to_value(ops.company(&company).unwrap().books().books()).unwrap();
    let cash = ops.company(&company).unwrap().books().books().ledger().cash_total().unwrap();
    let liabilities = ops.company(&company).unwrap().books().books().ledger().liabilities_total().unwrap();
    let equity = ops.company(&company).unwrap().books().books().ledger().equity_rolling().unwrap();
    let trial = ops.company(&company).unwrap().books().books().ledger().trial_balance().unwrap();
    assert_eq!(trial.total_debits, trial.total_credits, "经营后必须先已借贷平衡");
    assert!(liabilities.is_positive(), "fixture 含真实借款/费用应计，不能全零假绿");
    let mut closing = ClosingEngine::new();
    assert_eq!(ops.close_accounting_periods(date, &[], &mut closing).unwrap(), Some(period));
    let books = ops.company(&company).unwrap().books().books();
    assert_eq!(books.journal().period_status(period), PeriodStatus::Closed, "领域入口必须真正封月");
    assert_eq!(books.ledger().cash_total().unwrap(), cash, "关账不制造付款或覆盖现金");
    assert_eq!(books.ledger().liabilities_total().unwrap(), liabilities, "关账不抹掉债务/应计");
    assert_eq!(books.ledger().equity_rolling().unwrap(), equity, "关账不修改已形成的权益");
    let report = closing.version(&ScopeId::Standalone(MemberId(company.0)), period, ReportKind::Monthly, 1).unwrap();
    assert_eq!(report.balance_sheet.closing_cash, cash);
    assert_eq!(report.balance_sheet.total_liabilities, liabilities);
    assert_eq!(report.balance_sheet.total_assets, report.balance_sheet.liabilities_and_equity);
    assert!(report.income.cumulative.operating.len() > 0, "费用/经营损益须来自实际分录");
    let after = serde_json::to_value(books).unwrap();
    assert_eq!(before["journal"]["batches"], after["journal"]["batches"], "封账只登记期间，不重写成本或重复确认收入费用");
}

#[test]
fn company_domain_month_close_keeps_bank_deposits_loans_and_interest_distinct() {
    let date = d("2030-01-31");
    let period = AccountingPeriod::from_ymd(2030, 1).unwrap();
    let mut ops = CompanyOperations::new(four_company_config(21, quiet_params(), date.prev().unwrap()), date).unwrap();
    ops.advance_civil_day(date).unwrap();
    let bank = ops.catalog().id("C-BANK").unwrap();
    let before = serde_json::to_value(ops.company(&bank).unwrap().books()).unwrap();
    let cash = ops.bank_books(&bank).unwrap().books().ledger().cash_total().unwrap();
    let mut closing = ClosingEngine::new();
    assert_eq!(ops.close_accounting_periods(date, &[], &mut closing).unwrap(), Some(period));
    let books = ops.bank_books(&bank).unwrap();
    assert_eq!(books.books().journal().period_status(period), PeriodStatus::Closed);
    assert_eq!(books.books().ledger().cash_total().unwrap(), cash);
    let report = closing.version(&ScopeId::Standalone(MemberId(bank.0)), period, ReportKind::Monthly, 1).unwrap();
    assert_eq!(report.balance_sheet.total_assets, report.balance_sheet.liabilities_and_equity);
    assert_eq!(report.balance_sheet.closing_cash, cash);
    assert_eq!(serde_json::to_value(ops.company(&bank).unwrap().books()).unwrap()["Bank"]["loans"], before["Bank"]["loans"], "封账不能把贷款当费用或重复收本息");
    assert_eq!(serde_json::to_value(ops.company(&bank).unwrap().books()).unwrap()["Bank"]["deposits"], before["Bank"]["deposits"], "存款负债不能被关账当收入");
}

#[test]
fn domain_close_before_operating_day_rejects_without_any_accounting_change() {
    let date = d("2030-01-31");
    let mut ops = CompanyOperations::new(four_company_config(21, quiet_params(), date.prev().unwrap()), date).unwrap();
    let before = serde_json::to_vec(&ops).unwrap();
    let mut closing = ClosingEngine::new();
    let closing_before = serde_json::to_vec(&closing).unwrap();
    assert!(ops.close_accounting_periods(date, &[], &mut closing).is_err(),
        "经营日尚未完成，领域入口不可提前封账/定稿");
    assert_eq!(serde_json::to_vec(&ops).unwrap(), before);
    assert_eq!(serde_json::to_vec(&closing).unwrap(), closing_before);
    ops.advance_civil_day(date).unwrap();
    assert_eq!(ops.close_accounting_periods(date, &[], &mut closing).unwrap(),
        Some(AccountingPeriod::of_date(date)));
}

#[test]
fn domain_year_close_records_month_and_annual_without_repaying_or_reaccruing() {
    let date = d("2030-12-31");
    let period = AccountingPeriod::of_date(date);
    let start = d("2030-01-01");
    let mut ops = CompanyOperations::new(four_company_config(21, quiet_params(), start.prev().unwrap()), start).unwrap();
    let mut cursor = start;
    while cursor <= date {
        ops.advance_civil_day(cursor).unwrap();
        cursor = cursor.next().unwrap();
    }
    let before: Vec<_> = ops.catalog().iter().map(|(id, _)| {
        let books = ops.company(&id).unwrap().books().books();
        (id, books.ledger().cash_total().unwrap(), books.ledger().liabilities_total().unwrap(),
         serde_json::to_value(books).unwrap()["journal"]["batches"].clone())
    }).collect();
    let mut closing = ClosingEngine::new();
    assert_eq!(ops.close_accounting_periods(date, &[], &mut closing).unwrap(), Some(period));
    for (id, cash, liabilities, batches) in before {
        let books = ops.company(&id).unwrap().books().books();
        assert_eq!(books.ledger().cash_total().unwrap(), cash);
        assert_eq!(books.ledger().liabilities_total().unwrap(), liabilities);
        assert_eq!(serde_json::to_value(books).unwrap()["journal"]["batches"], batches);
        for kind in [ReportKind::Monthly, ReportKind::Annual] {
            let reports = closing.versions(&ScopeId::Standalone(MemberId(id.0)), period, kind);
            assert_eq!(reports.len(), 1, "月报/年报各自唯一，不能混成格式版本或重复关账");
            assert_eq!(reports[0].balance_sheet.closing_cash, cash);
            assert_eq!(reports[0].balance_sheet.total_assets, reports[0].balance_sheet.liabilities_and_equity);
        }
    }
}

#[test]
fn non_month_end_domain_close_does_not_touch_books_or_registry() {
    let date = d("2030-01-30");
    let mut ops = CompanyOperations::new(four_company_config(21, quiet_params(), date.prev().unwrap()), date).unwrap();
    ops.advance_civil_day(date).unwrap();
    let before = serde_json::to_vec(&ops).unwrap();
    let mut closing = ClosingEngine::new();
    let closing_before = serde_json::to_vec(&closing).unwrap();
    assert_eq!(ops.close_accounting_periods(date, &[], &mut closing).unwrap(), None);
    assert_eq!(serde_json::to_vec(&ops).unwrap(), before);
    assert_eq!(serde_json::to_vec(&closing).unwrap(), closing_before);
}

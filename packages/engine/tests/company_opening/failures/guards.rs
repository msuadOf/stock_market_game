//! 对手方与预算结构守卫：重复登记、未知对手方收付、非正金额收付、
//! 非正/重复授信、负现金下限，以及空登记簿的读取语义。

use super::super::*;
use engine::accounting::AccountingAmount;
use engine::company::{
    CompanyError, ContractBook, CounterpartyFlow, CounterpartyId, CounterpartyLedger, CreditLine,
    FlowDirection, OperatingBudget,
};

/// 对手方登记簿守卫：重复登记、未知对手方收付、非正金额收付。
#[test]
fn counterparty_ledger_guards_reject_invalid_flows() {
    let mut ledger = CounterpartyLedger::new();
    ledger
        .register(lender_counterparty("EXT-LDR", "虚构合作银行"))
        .expect("first registration works");
    match ledger.register(lender_counterparty("EXT-LDR", "重复登记")) {
        Err(CompanyError::DuplicateCounterparty { counterparty }) => {
            assert_eq!(counterparty, CounterpartyId("EXT-LDR".to_string()))
        }
        other => panic!("expected DuplicateCounterparty, got {other:?}"),
    }

    let flow = |counterparty: &str, cents: i128| CounterpartyFlow {
        date: d("2030-01-06"),
        counterparty: CounterpartyId(counterparty.to_string()),
        direction: FlowDirection::Inbound,
        amount: AccountingAmount::from_cents(cents),
        memo: "虚构收付".to_string(),
    };
    match ledger.record_flow(flow("EXT-GHOST", 100)) {
        Err(CompanyError::UnknownCounterparty { .. }) => {}
        other => panic!("expected UnknownCounterparty, got {other:?}"),
    }
    match ledger.record_flow(flow("EXT-LDR", 0)) {
        Err(CompanyError::FlowAmountNotPositive { .. }) => {}
        other => panic!("expected FlowAmountNotPositive, got {other:?}"),
    }
    // 合法收付后净头寸可查；守卫拒绝不改变流水。
    ledger
        .record_flow(flow("EXT-LDR", 100))
        .expect("valid flow records");
    assert_eq!(
        ledger
            .net_position(&CounterpartyId("EXT-LDR".to_string()))
            .expect("net position"),
        AccountingAmount::from_cents(100)
    );
    assert_eq!(ledger.flow_count(), 1);
}

/// 授信结构守卫：非正上限、同一贷款人重复授信、负现金下限。
#[test]
fn budget_guards_reject_invalid_credit_lines() {
    match OperatingBudget::new(
        yuan(1_000_000),
        vec![CreditLine {
            lender: CounterpartyId("EXT-LDR".to_string()),
            limit: AccountingAmount::ZERO,
        }],
    ) {
        Err(CompanyError::CreditLineInvalid { .. }) => {}
        other => panic!("expected CreditLineInvalid, got {other:?}"),
    }
    let line = CreditLine {
        lender: CounterpartyId("EXT-LDR".to_string()),
        limit: yuan(1_000_000),
    };
    match OperatingBudget::new(yuan(1_000_000), vec![line.clone(), line]) {
        Err(CompanyError::DuplicateCreditLine { .. }) => {}
        other => panic!("expected DuplicateCreditLine, got {other:?}"),
    }
    match OperatingBudget::new(AccountingAmount::from_cents(-1), Vec::<CreditLine>::new()) {
        Err(CompanyError::BudgetInvalid { .. }) => {}
        other => panic!("expected BudgetInvalid for negative floor, got {other:?}"),
    }
    // 空登记簿与未占用授信的读取语义。
    assert_eq!(ContractBook::new().len(), 0);
    assert!(ContractBook::new()
        .outstanding_borrowings(&CounterpartyId("EXT-LDR".to_string()))
        .expect("outstanding on empty book")
        .is_zero());
}

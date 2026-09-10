//! 授信与合同拒绝：无授信借款、超授信借款（含恰好用满边界）、合同数据守卫。

use super::super::*;
use super::{borrowing, credited_registry};
use engine::company::{CompanyConfig, CompanyError, CompanyId, CompanyKind, ContractId};

/// 无授信仍新增债务 → 拒绝；公司状态不变。
#[test]
fn new_debt_without_credit_line_rejected() {
    // 有贷款人对手方、但预算无任何授信额度。
    let config = CompanyConfig {
        spec: unlisted_spec("T-NO-CREDIT", CompanyKind::Industrial, 100_000_000),
        opening: generic_opening(vec![
            opening_line("1002", PostingSide::Debit, 50_000_000),
            opening_line("1601", PostingSide::Debit, 50_000_000),
            opening_line("4001", PostingSide::Credit, 100_000_000),
        ]),
        counterparties: vec![lender_counterparty("EXT-LDR", "虚构合作银行")],
        budget: empty_budget(),
    };
    let mut registry =
        engine::company::CompanyRegistry::new(vec![config]).expect("company constructs");
    let company_id = CompanyId("T-NO-CREDIT".to_string());
    let before = registry.get(&company_id).expect("company").clone();

    match registry
        .get_mut(&company_id)
        .expect("company")
        .register_contract(borrowing("L-1", "EXT-LDR", 1_000))
    {
        Err(CompanyError::NoCreditLine {
            company,
            lender,
            requested,
        }) => {
            assert_eq!(company, company_id);
            assert_eq!(
                lender,
                engine::company::CounterpartyId("EXT-LDR".to_string())
            );
            assert_eq!(requested, yuan(1_000));
        }
        other => panic!("expected NoCreditLine, got {other:?}"),
    }
    assert_eq!(
        *registry.get(&company_id).expect("company"),
        before,
        "rejected borrowing must not change company state"
    );
}

/// 新增债务超出授信 → 拒绝并携带占用/申请/上限；恰好用满授信合法；拒绝后状态不变。
#[test]
fn new_debt_beyond_credit_line_rejected() {
    let mut registry = credited_registry(1_000_000);
    let company_id = CompanyId("T-CREDIT".to_string());

    // 60 万占用授信；再借 50 万 → 超出 100 万上限。
    registry
        .get_mut(&company_id)
        .expect("company")
        .register_contract(borrowing("L-1", "EXT-LDR", 600_000))
        .expect("first borrowing within line");
    let before = registry.get(&company_id).expect("company").clone();

    match registry
        .get_mut(&company_id)
        .expect("company")
        .register_contract(borrowing("L-2", "EXT-LDR", 500_000))
    {
        Err(CompanyError::DebtBeyondCreditLine {
            company,
            credit_limit,
            outstanding,
            requested,
            ..
        }) => {
            assert_eq!(company, company_id);
            assert_eq!(credit_limit, yuan(1_000_000));
            assert_eq!(outstanding, yuan(600_000));
            assert_eq!(requested, yuan(500_000));
        }
        other => panic!("expected DebtBeyondCreditLine, got {other:?}"),
    }
    assert_eq!(
        *registry.get(&company_id).expect("company"),
        before,
        "rejected borrowing must not change company state"
    );

    // 边界：恰好用满授信（60 万 + 40 万 = 100 万）合法。
    registry
        .get_mut(&company_id)
        .expect("company")
        .register_contract(borrowing("L-3", "EXT-LDR", 400_000))
        .expect("borrowing exactly up to the line is allowed");
    assert_eq!(
        registry
            .get(&company_id)
            .expect("company")
            .contracts()
            .outstanding_borrowings(&engine::company::CounterpartyId("EXT-LDR".to_string()))
            .expect("outstanding"),
        yuan(1_000_000)
    );
}

/// 合同数据守卫：未知对手方、非正本金、负利率、到期不晚于起息、重复 id。
#[test]
fn contract_guards_reject_invalid_shapes() {
    let mut registry = credited_registry(1_000_000);
    let company_id = CompanyId("T-CREDIT".to_string());

    // 未知对手方。
    match registry
        .get_mut(&company_id)
        .expect("company")
        .register_contract(borrowing("L-1", "EXT-GHOST", 1))
    {
        Err(CompanyError::UnknownCounterparty { counterparty }) => {
            assert_eq!(
                counterparty,
                engine::company::CounterpartyId("EXT-GHOST".to_string())
            )
        }
        other => panic!("expected UnknownCounterparty, got {other:?}"),
    }
    // 非正本金。
    let mut zero_principal = borrowing("L-1", "EXT-LDR", 1);
    zero_principal.principal = AccountingAmount::ZERO;
    match registry
        .get_mut(&company_id)
        .expect("company")
        .register_contract(zero_principal)
    {
        Err(CompanyError::NonPositivePrincipal { contract, .. }) => {
            assert_eq!(contract, ContractId("L-1".to_string()))
        }
        other => panic!("expected NonPositivePrincipal, got {other:?}"),
    }
    // 负利率。
    let mut negative_rate = borrowing("L-1", "EXT-LDR", 1);
    negative_rate.annual_rate_bp = -1;
    match registry
        .get_mut(&company_id)
        .expect("company")
        .register_contract(negative_rate)
    {
        Err(CompanyError::NegativeRate { rate_bp, .. }) => assert_eq!(rate_bp, -1),
        other => panic!("expected NegativeRate, got {other:?}"),
    }
    // 到期日不晚于起息日。
    let mut same_day = borrowing("L-1", "EXT-LDR", 1);
    same_day.maturity_date = same_day.start_date;
    match registry
        .get_mut(&company_id)
        .expect("company")
        .register_contract(same_day)
    {
        Err(CompanyError::MaturityNotAfterStart { .. }) => {}
        other => panic!("expected MaturityNotAfterStart, got {other:?}"),
    }
    // 合法登记后重复 id。
    registry
        .get_mut(&company_id)
        .expect("company")
        .register_contract(borrowing("L-1", "EXT-LDR", 1))
        .expect("valid borrowing registers");
    match registry
        .get_mut(&company_id)
        .expect("company")
        .register_contract(borrowing("L-1", "EXT-LDR", 1))
    {
        Err(CompanyError::DuplicateContract { contract }) => {
            assert_eq!(contract, ContractId("L-1".to_string()))
        }
        other => panic!("expected DuplicateContract, got {other:?}"),
    }
}

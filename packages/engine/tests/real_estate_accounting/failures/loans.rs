//! 借款/债务偿付守卫拒绝：合同条款、授信（无授信/超授信/重复合同）、
//! 计提回拨、无应计付息、还本超额、尾款超收（AR 开项透传）。

use super::super::{base_config, d, yuan};
use engine::company::real_estate::{ProjectId, RealEstateBooks, RealEstateError};
use engine::company::{ContractId, CounterpartyId};

const LAND: &str = "EXT-LAND-1";
const CON: &str = "EXT-CON-1";
const BUY: &str = "EXT-BUY-1";
const LEND: &str = "EXT-LEND-1";

#[test]
fn rejects_borrow_term_and_credit_violations() {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly");
    let lender = CounterpartyId(LEND.to_string());
    let before = re.clone();
    // 到期不晚于起息日（合同数据校验 → Company 错误透传）。
    assert!(matches!(
        re.borrow_project_loan(
            ContractId("L-1".to_string()),
            &lender,
            yuan(1_000),
            500,
            d("2030-01-02"),
            d("2030-01-02"),
            None,
        ),
        Err(RealEstateError::Company(
            engine::company::CompanyError::MaturityNotAfterStart { .. }
        ))
    ));
    // 非正本金。
    assert!(matches!(
        re.borrow_project_loan(
            ContractId("L-1".to_string()),
            &lender,
            yuan(0),
            500,
            d("2030-01-02"),
            d("2031-01-02"),
            None,
        ),
        Err(RealEstateError::Company(
            engine::company::CompanyError::NonPositivePrincipal { .. }
        ))
    ));
    // 指定到不存在的项目。
    assert!(matches!(
        re.borrow_project_loan(
            ContractId("L-1".to_string()),
            &lender,
            yuan(1_000),
            500,
            d("2030-01-02"),
            d("2031-01-02"),
            Some(ProjectId("P-404".to_string())),
        ),
        Err(RealEstateError::UnknownProject { .. })
    ));
    assert_eq!(re, before);
    // 无授信贷款人。
    let mut no_credit = base_config();
    no_credit.budget = engine::company::OperatingBudget::new(
        engine::accounting::AccountingAmount::ZERO,
        Vec::new(),
    )
    .expect("budget");
    let mut re_nc = RealEstateBooks::new(no_credit).expect("assembly");
    let before_nc = re_nc.clone();
    assert!(matches!(
        re_nc.borrow_project_loan(
            ContractId("L-1".to_string()),
            &lender,
            yuan(1_000),
            500,
            d("2030-01-02"),
            d("2031-01-02"),
            None,
        ),
        Err(RealEstateError::NoCreditLine { .. })
    ));
    assert_eq!(re_nc, before_nc);
    // 超授信（限额 20,000 元，借 25,000 元）。
    assert!(matches!(
        re.borrow_project_loan(
            ContractId("L-1".to_string()),
            &lender,
            yuan(25_000),
            500,
            d("2030-01-02"),
            d("2031-01-02"),
            None,
        ),
        Err(RealEstateError::DebtBeyondCreditLine { .. })
    ));
    assert_eq!(re, before);
    // 重复合同 id。
    re.borrow_project_loan(
        ContractId("L-1".to_string()),
        &lender,
        yuan(5_000),
        500,
        d("2030-01-02"),
        d("2031-01-02"),
        None,
    )
    .expect("borrow");
    let before = re.clone();
    assert!(matches!(
        re.borrow_project_loan(
            ContractId("L-1".to_string()),
            &lender,
            yuan(1_000),
            500,
            d("2030-01-03"),
            d("2031-01-03"),
            None,
        ),
        Err(RealEstateError::DuplicateContract { .. })
    ));
    assert_eq!(re, before);
}

#[test]
fn rejects_accrual_pay_repay_violations() {
    let mut re = fixture_with_loan();
    let contract = ContractId("L-1".to_string());
    re.accrue_interest(d("2030-02-01")).expect("accrue");
    let before = re.clone();
    // 时间回拨。
    assert!(matches!(
        re.accrue_interest(d("2030-01-15")),
        Err(RealEstateError::AccrualNotForward { .. })
    ));
    assert_eq!(re, before);
    // 无应计付息（先付清再付）。
    re.pay_interest(&contract, d("2030-02-02")).expect("pay");
    let before = re.clone();
    assert!(matches!(
        re.pay_interest(&contract, d("2030-02-03")),
        Err(RealEstateError::NothingAccrued { .. })
    ));
    // 还本超额。
    assert!(matches!(
        re.repay_principal(&contract, yuan(10_001), d("2030-02-04")),
        Err(RealEstateError::PrincipalBeyondOutstanding { .. })
    ));
    // 非正还本。
    assert!(matches!(
        re.repay_principal(&contract, yuan(0), d("2030-02-04")),
        Err(RealEstateError::NonPositiveAmount { .. })
    ));
    assert_eq!(re, before);
}

/// 尾款超收（超出应收开项余额 → Trade 开项错误透传）。
#[test]
fn rejects_final_collection_beyond_open_item() {
    let mut re = completed_project_with_presale();
    let contract = ContractId("C-1".to_string());
    let outcome = re.deliver(&contract, d("2030-03-01")).expect("deliver");
    let item = outcome.receivable.expect("ar");
    let before = re.clone();
    assert!(matches!(
        re.collect_final(&item, yuan(12_001), d("2030-03-02")),
        Err(RealEstateError::Trade(
            engine::accounting::TradeLedgerError::OverApplication { .. }
        ))
    ));
    assert_eq!(re, before);
}

// ===== 夹具 =====

/// 开局现金 1000 元 + 无项目借款 10000 元（计提/付息/还本守卫）。
fn fixture_with_loan() -> RealEstateBooks {
    let mut config = base_config();
    config.opening_lines = vec![
        super::super::cent_line("1002", engine::accounting::PostingSide::Debit, 100_000),
        super::super::cent_line("4001", engine::accounting::PostingSide::Credit, 100_000),
    ];
    let mut re = RealEstateBooks::new(config).expect("assembly");
    re.borrow_project_loan(
        ContractId("L-1".to_string()),
        &CounterpartyId(LEND.to_string()),
        yuan(10_000),
        730,
        d("2030-01-01"),
        d("2031-06-30"),
        None,
    )
    .expect("borrow");
    re
}

/// 完工项目 + 已收款预售（交付/尾款夹具）：10 套、总价 30000 元已收 18000 元。
pub(super) fn completed_project_with_presale() -> RealEstateBooks {
    let mut re = RealEstateBooks::new(base_config()).expect("assembly");
    re.acquire_land(
        ProjectId("P-1".to_string()),
        &CounterpartyId(LAND.to_string()),
        10,
        yuan(12_000),
        d("2030-01-01"),
    )
    .expect("land");
    re.incur_development(
        &ProjectId("P-1".to_string()),
        &CounterpartyId(CON.to_string()),
        yuan(8_000),
        d("2030-01-01"),
    )
    .expect("dev");
    re.sign_presale(
        ContractId("C-1".to_string()),
        &ProjectId("P-1".to_string()),
        &CounterpartyId(BUY.to_string()),
        6,
        yuan(30_000),
        d("2030-01-15"),
    )
    .expect("sign");
    re.collect_presale(
        &ContractId("C-1".to_string()),
        yuan(18_000),
        d("2030-01-15"),
    )
    .expect("collect");
    re.complete_project(&ProjectId("P-1".to_string()), d("2030-02-01"))
        .expect("complete");
    re
}

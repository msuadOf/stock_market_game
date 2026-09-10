//! 负向用例（类型化拒绝 + 完整状态不变断言）：超可支付现金提款 →
//! PaymentFailed（不是负现金、银行继续运行）；非法 PD/LGD/权重；重复核销；
//! 结构化/未支持合同显式 UnsupportedContract；处理器守卫（越权提取/收款、
//! 未知合同、时间回拨、非法利率、重复 id、未登记对手方、无已提利息、
//! 准备不足核销、非核销贷款回收、开局子账种子）。

mod contracts;
mod ecl;
mod guards;

use super::base_config;

use engine::company::bank::{BankBooks, BankProductKind, EclScenario};
use engine::company::{ContractId, CounterpartyId};

pub(crate) fn dep_cp() -> CounterpartyId {
    CounterpartyId("EXT-DEP-1".to_string())
}

pub(crate) fn bor_cp() -> CounterpartyId {
    CounterpartyId("EXT-BOR-1".to_string())
}

pub(crate) fn dep_id() -> ContractId {
    ContractId("D1".to_string())
}

pub(crate) fn loan_id() -> ContractId {
    ContractId("L1".to_string())
}

pub(crate) fn fresh() -> BankBooks {
    BankBooks::new(base_config()).expect("base opening must construct")
}

/// 标准存款 1000 元（2030-01-02 起，@150bp）。
pub(crate) fn with_deposit() -> BankBooks {
    let mut bank = fresh();
    bank.accept_deposit(
        BankProductKind::TermDeposit,
        dep_id(),
        &dep_cp(),
        super::yuan(1_000),
        150,
        super::d("2030-01-02"),
        super::d("2030-07-02"),
    )
    .expect("deposit");
    bank
}

/// 标准存款 1000 元 + 贷款 600 元（含日终 ECL 3.00 元，阶段 1）。
pub(crate) fn with_loan() -> BankBooks {
    let mut bank = with_deposit();
    bank.issue_loan(
        BankProductKind::TermLoan,
        loan_id(),
        &bor_cp(),
        super::yuan(600),
        600,
        super::d("2030-01-05"),
        super::d("2030-07-05"),
    )
    .expect("loan");
    bank
}

/// 单一 100% 情景（核销前足额计提用：PD=LGD=100%）。
pub(crate) fn full_loss() -> Vec<EclScenario> {
    vec![EclScenario {
        weight_bp: 10_000,
        pd_bp: 10_000,
        lgd_bp: 10_000,
    }]
}

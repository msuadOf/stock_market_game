//! 类型化拒绝金样（共享夹具）：每个失败路径都断言精确错误变体（含上下文）
//! 且完整状态不变（公司实体逐字段相等 / 注册表整体不产生 / 账套仍为零分录）。
//! 按拒绝对象拆分：`registry`（规格/映射/开局）、`credit`（授信与合同）、
//! `guards`（对手方与预算结构守卫）。

mod credit;
mod guards;
mod registry;

use super::*;
use engine::company::{
    CompanyConfig, CompanyKind, CompanyRegistry, ContractId, ContractRole, CounterpartyId,
    CreditLine, DayCountBasis, OperatingBudget, OperatingContract,
};

/// 带授信的单一公司（贷款人 EXT-LDR / 授信上限由参数指定）注册表。
pub(super) fn credited_registry(limit_yuan: i128) -> CompanyRegistry {
    let spec = unlisted_spec("T-CREDIT", CompanyKind::Industrial, 100_000_000);
    let config = CompanyConfig {
        spec,
        opening: generic_opening(vec![
            opening_line("1002", PostingSide::Debit, 50_000_000),
            opening_line("1601", PostingSide::Debit, 50_000_000),
            opening_line("4001", PostingSide::Credit, 100_000_000),
        ]),
        counterparties: vec![lender_counterparty("EXT-LDR", "虚构合作银行")],
        budget: OperatingBudget::new(
            yuan(1_000_000),
            vec![CreditLine {
                lender: CounterpartyId("EXT-LDR".to_string()),
                limit: yuan(limit_yuan),
            }],
        )
        .expect("credited budget is valid"),
    };
    CompanyRegistry::new(vec![config]).expect("credited company constructs")
}

/// 借款合同夹具（ACT/365F、固定年利率 3.5%）。
pub(super) fn borrowing(id: &str, lender: &str, principal_yuan: i128) -> OperatingContract {
    OperatingContract {
        id: ContractId(id.to_string()),
        role: ContractRole::Borrowing,
        counterparty: CounterpartyId(lender.to_string()),
        principal: yuan(principal_yuan),
        annual_rate_bp: 350,
        start_date: d("2030-01-06"),
        maturity_date: d("2031-01-06"),
        basis: DayCountBasis::Act365F,
    }
}

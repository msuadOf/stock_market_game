//! 地产列报分类层（K3，任务 11）：总账科目 → CAS 30（2026）+ CAS 14（2017）
//! 地产开发企业列示行的映射。官方依据已核验（docs/company-accounting.md
//! §2.5）：CAS 14 §39（预收款确认为合同负债）、§4/§13（交付控制权转移确认
//! 收入）+ CAS 30（2026）§25/§27（资产负债表单列项目，含合同负债）。
//! 开发存货计价（移动加权）是 K3 固定游戏假设（CAS 1 原文受阻）。
//!
//! 本模块是**分类层**：只读取总账净借方余额并组合成列报行，不生成完整
//! 报表、不结账、不触现金（任务 13 的报表生成器消费这里的结果）。
//!
//! 科目代码的单一真源：`codes` 模块同时被 `company::real_estate::chart`
//! （科目表构造）引用——列报映射与过账科目共享同一份代码表，杜绝两层漂移。

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::ledger::Ledger;
use crate::accounting::ledger::LedgerAccountId;

/// 地产科目表 v5 科目代码（company/real_estate/chart.rs 以此构造科目表）。
pub mod codes {
    pub const CASH: &str = "1002"; // 银行存款（现金类）
    pub const AR: &str = "1122"; // 应收账款（交付尾款）
    pub const DEV_INVENTORY: &str = "1541"; // 开发存货（土地+开发成本+资本化利息）
    pub const DEV_IMPAIR_ALLOW: &str = "1542"; // 开发存货减值准备（资产备抵）
    pub const ST_DEBT: &str = "2001"; // 短期借款（≤365 天）
    pub const CONTRACT_LIAB: &str = "2203"; // 合同负债（预售收款，CAS 14 §39）
    pub const INT_PAYABLE: &str = "2231"; // 应付利息
    pub const LT_DEBT: &str = "2501"; // 长期借款（>365 天）
    pub const CAPITAL: &str = "4001"; // 实收资本
    pub const PROFIT_CURRENT: &str = "4103"; // 本年利润（结账科目，任务 13）
    pub const REVENUE: &str = "6001"; // 主营业务收入（营业收入）
    pub const COGS: &str = "6401"; // 主营业务成本（营业成本）
    pub const FIN_EXP: &str = "6603"; // 财务费用（费用化借款利息）
    pub const IMPAIR_LOSS: &str = "6701"; // 资产减值损失
}

/// 地产列报行（分类结果；正负号 = 借贷方向，负债/收入行以正数呈现其
/// 贷方金额，费用/资产备抵行以正数呈现其损失/备抵属性）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct RealEstatePresentationLines {
    /// 开发存货（毛额：土地 + 开发成本 + 资本化借款费用 − 已结转成本）。
    pub development_inventory_gross: AccountingAmount,
    /// 开发存货减值准备（1542 贷方余额，正数呈现）。
    pub development_inventory_impairment: AccountingAmount,
    /// 开发存货净额 = 毛额 − 减值准备。
    pub development_inventory_net: AccountingAmount,
    /// 合同负债（预售已收未交付款项；CAS 14 §39 + CAS 30 (2026) §27(五)）。
    pub contract_liabilities: AccountingAmount,
    /// 应收账款（已交付未收尾款）。
    pub final_payment_receivable: AccountingAmount,
    /// 现金头寸（银行存款）。
    pub cash_position: AccountingAmount,
    /// 营业收入（交付时点确认，6001 贷方余额；预售收款不在此行）。
    pub operating_revenue: AccountingAmount,
    /// 营业成本（交付结转，6401 借方余额）。
    pub operating_cost: AccountingAmount,
    /// 财务费用（费用化借款利息 + 其他财务费用）。
    pub finance_cost: AccountingAmount,
    /// 资产减值损失（开发存货减值；转回为负）。
    pub impairment_loss: AccountingAmount,
}

/// 由总账读净借方余额（负值 = 贷方余额）。
fn debit(ledger: &Ledger, code: &str) -> Result<AccountingAmount, AccountingError> {
    ledger.account_net_debit(&LedgerAccountId(code.to_string()))
}

/// 地产列报分类：总账 → 列报行（纯读投影，不修改任何状态）。
pub fn real_estate_presentation_lines(
    ledger: &Ledger,
) -> Result<RealEstatePresentationLines, AccountingError> {
    let credit_of =
        |code: &str| -> Result<AccountingAmount, AccountingError> { debit(ledger, code)?.neg() };
    let gross = debit(ledger, codes::DEV_INVENTORY)?;
    let impairment = credit_of(codes::DEV_IMPAIR_ALLOW)?;
    Ok(RealEstatePresentationLines {
        development_inventory_gross: gross,
        development_inventory_impairment: impairment,
        development_inventory_net: gross.sub(impairment)?,
        contract_liabilities: credit_of(codes::CONTRACT_LIAB)?,
        final_payment_receivable: debit(ledger, codes::AR)?,
        cash_position: debit(ledger, codes::CASH)?,
        operating_revenue: credit_of(codes::REVENUE)?,
        operating_cost: debit(ledger, codes::COGS)?,
        finance_cost: debit(ledger, codes::FIN_EXP)?,
        impairment_loss: debit(ledger, codes::IMPAIR_LOSS)?,
    })
}

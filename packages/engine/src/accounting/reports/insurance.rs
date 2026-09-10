//! 保险列报分类层（K3，任务 10）：总账科目 → CAS 25（2020）§84/§85 保险
//! 合同资产负债表四项/利润表行 + CAS 30（2026）§55(二) 保险财务损益（经营
//! 类别）的映射。官方依据已核验（docs/company-accounting.md §2.4）。
//!
//! 本模块是**分类层**：只读取总账净借方余额并组合成列报行，不生成完整
//! 报表、不结账、不触现金（任务 13 的报表生成器消费这里的结果）。
//!
//! 科目代码的单一真源：`codes` 模块同时被 `company::insurance::chart`
//! （科目表构造）引用——列报映射与过账科目共享同一份代码表，杜绝两层
//! 漂移。CSM 是 2501 的组合成分（备查子账量），不是独立总账科目；
//! 调节表（CSM 释放 ↔ 收入 ↔ 现金）由 tests/insurance_accounting 用
//! 子账累计量对账。

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::ledger::Ledger;
use crate::accounting::ledger::LedgerAccountId;

/// 保险科目表 v4 科目代码（company/insurance/chart.rs 以此构造科目表）。
pub mod codes {
    pub const CASH: &str = "1002"; // 银行存款（现金类）
    pub const PREMIUM_RECEIVABLE: &str = "1122"; // 应收保费
    pub const LRC: &str = "2501"; // 未到期责任负债
    pub const LIC: &str = "2502"; // 已发生赔款负债
    pub const CAPITAL: &str = "4001"; // 实收资本
    pub const PROFIT_CURRENT: &str = "4103"; // 本年利润（结账科目，任务 13）
    pub const INSURANCE_REVENUE: &str = "6051"; // 保险服务收入
    pub const INSURANCE_EXPENSE: &str = "6451"; // 保险服务费用
    pub const INSURANCE_FINANCE: &str = "6541"; // 保险财务损益（费用要素）
}

/// 保险列报行（分类结果；正负号 = 借贷方向，负债/收入行以正数呈现其
/// 贷方金额，费用行以正数呈现其损失属性；贷方余额的费用行如实为负）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct InsurancePresentationLines {
    /// 保险服务收入（6051 贷方发生额余额；含预期赔付/风险调整/CSM 的
    /// 责任单元释放——不含投资成分，CAS 25 §30/§31）。
    pub insurance_revenue: AccountingAmount,
    /// 保险服务费用（6451 借方余额；赔案发生 + 首日亏损/亏损成分变动）。
    pub insurance_expense: AccountingAmount,
    /// 保险服务业绩 = 收入 − 费用（CAS 25 §85 利润表列示口径）。
    pub insurance_service_result: AccountingAmount,
    /// 保险财务损益（6541 借方余额；正数 = 损失/费用，负数 = 收益）。
    pub insurance_finance_expense: AccountingAmount,
    /// 未到期责任负债（2501 贷方余额；含 CSM 组合成分）。
    pub lrc_balance: AccountingAmount,
    /// 已发生赔款负债（2502 贷方余额）。
    pub lic_balance: AccountingAmount,
    /// 应收保费（1122 借方余额）。
    pub premiums_receivable: AccountingAmount,
    /// 银行存款头寸（1002）。
    pub cash_position: AccountingAmount,
}

/// 由总账读净借方余额（负值 = 贷方余额）。
fn debit(ledger: &Ledger, code: &str) -> Result<AccountingAmount, AccountingError> {
    ledger.account_net_debit(&LedgerAccountId(code.to_string()))
}

/// 保险列报分类：总账 → 列报行（纯读投影，不修改任何状态）。
pub fn insurance_presentation_lines(
    ledger: &Ledger,
) -> Result<InsurancePresentationLines, AccountingError> {
    let credit_of =
        |code: &str| -> Result<AccountingAmount, AccountingError> { debit(ledger, code)?.neg() };
    let insurance_revenue = credit_of(codes::INSURANCE_REVENUE)?;
    let insurance_expense = debit(ledger, codes::INSURANCE_EXPENSE)?;
    Ok(InsurancePresentationLines {
        insurance_service_result: insurance_revenue.sub(insurance_expense)?,
        insurance_revenue,
        insurance_expense,
        insurance_finance_expense: debit(ledger, codes::INSURANCE_FINANCE)?,
        lrc_balance: credit_of(codes::LRC)?,
        lic_balance: credit_of(codes::LIC)?,
        premiums_receivable: debit(ledger, codes::PREMIUM_RECEIVABLE)?,
        cash_position: debit(ledger, codes::CASH)?,
    })
}

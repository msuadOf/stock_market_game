//! 银行列报分类层（K3，任务 9）：总账科目 → CAS 30（2026）银行列示行的
//! 映射。官方依据已核验（docs/company-accounting.md §2.3）：CAS 30（2026）
//! §45–§47、§16——「向客户提供融资」为主要业务活动的经营/筹资归类与流动性
//! 列示；损益行覆盖 利息净收入 / 手续费及佣金净收入 / 信用减值损失。
//!
//! 本模块是**分类层**：只读取总账净借方余额并组合成列报行，不生成完整
//! 报表、不结账、不触现金（任务 13 的报表生成器消费这里的结果）。
//!
//! 科目代码的单一真源：`codes` 模块同时被 `company::bank::chart`（科目表
//! 构造）引用——列报映射与过账科目共享同一份代码表，杜绝两层漂移。

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::ledger::Ledger;
use crate::accounting::ledger::LedgerAccountId;

/// 银行科目表 v3 科目代码（company/bank/chart.rs 以此构造科目表）。
pub mod codes {
    pub const CASH: &str = "1003"; // 存放中央银行款项（现金类）
    pub const LOAN_INT_RCV: &str = "1131"; // 应收利息（贷款）
    pub const LOAN_PRINCIPAL: &str = "1301"; // 贷款——本金
    pub const LOAN_ALLOWANCE: &str = "1303"; // 贷款减值准备（资产备抵）
    pub const ST_DEPOSIT: &str = "2011"; // 吸收存款——短期（≤365 天）
    pub const LT_DEPOSIT: &str = "2601"; // 吸收存款——长期（>365 天）
    pub const DEP_INT_PAYABLE: &str = "2231"; // 应付利息（存款）
    pub const CAPITAL: &str = "4001"; // 实收资本
    pub const PROFIT_CURRENT: &str = "4103"; // 本年利润（结账科目，任务 13）
    pub const INTEREST_INCOME: &str = "6011"; // 利息收入
    pub const FEE_INCOME: &str = "6021"; // 手续费及佣金收入
    pub const INTEREST_EXPENSE: &str = "6411"; // 利息支出
    pub const CREDIT_IMPAIR: &str = "6701"; // 信用减值损失
}

/// 银行列报行（分类结果；正负号 = 借贷方向，负债/收入行以正数呈现其
/// 贷方金额，费用/资产备抵行以正数呈现其损失/备抵属性）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct BankPresentationLines {
    /// 利息收入（6011 贷方发生额余额）。
    pub interest_income: AccountingAmount,
    /// 利息支出（6411 借方余额）。
    pub interest_expense: AccountingAmount,
    /// 利息净收入 = 利息收入 − 利息支出（CAS 30 (2026) 银行损益首行）。
    pub net_interest_income: AccountingAmount,
    /// 手续费及佣金净收入（本游戏只实现收入侧）。
    pub fee_and_commission_income: AccountingAmount,
    /// 信用减值损失（6701 借方余额；贷方余额 = 转回利得，如实为负）。
    pub credit_impairment_loss: AccountingAmount,
    /// 贷款及垫款总额 = 本金 + 应计利息（毛额）。
    pub loans_and_advances_gross: AccountingAmount,
    /// 贷款损失准备（1303 贷方余额，正数呈现）。
    pub loan_loss_allowance: AccountingAmount,
    /// 贷款及垫款净额 = 总额 − 准备。
    pub loans_and_advances_net: AccountingAmount,
    /// 客户存款 = 短期 + 长期吸收存款（负债，正数呈现）。
    pub customer_deposits: AccountingAmount,
    /// 现金及存放中央银行款项。
    pub cash_position: AccountingAmount,
    /// 应付利息（存款已提未付；流动性列示项）。
    pub deposit_interest_payable: AccountingAmount,
}

/// 由总账读净借方余额（负值 = 贷方余额）。
fn debit(ledger: &Ledger, code: &str) -> Result<AccountingAmount, AccountingError> {
    ledger.account_net_debit(&LedgerAccountId(code.to_string()))
}

/// 银行列报分类：总账 → 列报行（纯读投影，不修改任何状态）。
pub fn bank_presentation_lines(ledger: &Ledger) -> Result<BankPresentationLines, AccountingError> {
    let credit_of =
        |code: &str| -> Result<AccountingAmount, AccountingError> { debit(ledger, code)?.neg() };
    let interest_income = credit_of(codes::INTEREST_INCOME)?;
    let interest_expense = debit(ledger, codes::INTEREST_EXPENSE)?;
    let fee_and_commission_income = credit_of(codes::FEE_INCOME)?;
    let credit_impairment_loss = debit(ledger, codes::CREDIT_IMPAIR)?;
    let loan_principal = debit(ledger, codes::LOAN_PRINCIPAL)?;
    let loan_int_rcv = debit(ledger, codes::LOAN_INT_RCV)?;
    let loan_allowance = credit_of(codes::LOAN_ALLOWANCE)?;
    let loans_gross = loan_principal.add(loan_int_rcv)?;
    let st_deposit = credit_of(codes::ST_DEPOSIT)?;
    let lt_deposit = credit_of(codes::LT_DEPOSIT)?;
    Ok(BankPresentationLines {
        net_interest_income: interest_income.sub(interest_expense)?,
        interest_income,
        interest_expense,
        fee_and_commission_income,
        credit_impairment_loss,
        loans_and_advances_gross: loans_gross,
        loan_loss_allowance: loan_allowance,
        loans_and_advances_net: loans_gross.sub(loan_allowance)?,
        customer_deposits: st_deposit.add(lt_deposit)?,
        cash_position: debit(ledger, codes::CASH)?,
        deposit_interest_payable: credit_of(codes::DEP_INT_PAYABLE)?,
    })
}

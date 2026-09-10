//! 工业列报分类层（K3，任务 13）：总账科目 → CAS 30（2026）工商列示行的
//! 映射 + 工业扩充归类表（完成任务 9–11 建立的四行业集合）。
//!
//! 与 bank/insurance/real_estate 同构：本模块是**分类层**（全期间读投影，
//! 任务 13 报表生成器消费窗口化口径，二者共享科目代码）。工业科目表 v2
//! 的代码真源在 `company::industrial::chart::acct`（任务 8 先建，crate 内
//! 私有）；本文件的 codes 常量与之镜像——漂移由「科目表全覆盖」归类校验
//! 与四行业金样测试锁定（缺码 = `UnclassifiedAccount` 类型化拒绝）。

use crate::accounting::amount::AccountingAmount;
use crate::accounting::error::AccountingError;
use crate::accounting::ledger::Ledger;
use crate::accounting::ledger::LedgerAccountId;

use super::notes::{a, Assignment, NoteTarget};
use super::BsLine;
use super::IncomeLine;

/// 工业科目表 v2 扩充科目代码（v1 基础科目见 notes::base_assignments）。
pub mod codes {
    pub const BAD_DEBT_ALLOW: &str = "1231"; // 坏账准备（资产备抵）
    pub const RAW_MATERIAL: &str = "1403"; // 原材料
    pub const FINISHED_GOODS: &str = "1405"; // 库存商品
    pub const ACC_IMPAIR: &str = "1603"; // 固定资产减值准备（资产备抵）
    pub const DTA: &str = "1811"; // 递延所得税资产
    pub const VAT_OUT: &str = "222101"; // 应交增值税（销项税额）
    pub const VAT_IN: &str = "222102"; // 应交增值税（进项税额，备抵）
    pub const CIT_PAYABLE: &str = "222104"; // 应交所得税
    pub const LT_DEBT: &str = "2501"; // 长期借款
    pub const DTL: &str = "2901"; // 递延所得税负债（预留）
    pub const WIP: &str = "5001"; // 生产成本（在产品）
    pub const SELLING_EXP: &str = "6601"; // 销售费用
    pub const RND_EXP: &str = "660201"; // 研发费用（管理费用子科目位）
    pub const IMPAIR_LOSS: &str = "6701"; // 资产减值损失
}

/// 工业扩充归类表（v1 基表之外）。
pub fn extra_assignments() -> Vec<Assignment> {
    use codes::*;
    vec![
        a(
            BAD_DEBT_ALLOW,
            NoteTarget::BalanceSheet(BsLine::Receivables),
        ),
        a(RAW_MATERIAL, NoteTarget::BalanceSheet(BsLine::Inventory)),
        a(FINISHED_GOODS, NoteTarget::BalanceSheet(BsLine::Inventory)),
        a(ACC_IMPAIR, NoteTarget::BalanceSheet(BsLine::FixedAssets)),
        a(DTA, NoteTarget::BalanceSheet(BsLine::DeferredTaxAssets)),
        a(VAT_OUT, NoteTarget::BalanceSheet(BsLine::TaxesPayable)),
        a(VAT_IN, NoteTarget::BalanceSheet(BsLine::TaxesPayable)),
        a(CIT_PAYABLE, NoteTarget::BalanceSheet(BsLine::TaxesPayable)),
        a(
            LT_DEBT,
            NoteTarget::BalanceSheet(BsLine::LongTermBorrowings),
        ),
        a(
            DTL,
            NoteTarget::BalanceSheet(BsLine::DeferredTaxLiabilities),
        ),
        a(WIP, NoteTarget::BalanceSheet(BsLine::Inventory)),
        a(SELLING_EXP, NoteTarget::Income(IncomeLine::SellingExpense)),
        a(RND_EXP, NoteTarget::Income(IncomeLine::ResearchExpense)),
        a(IMPAIR_LOSS, NoteTarget::Income(IncomeLine::ImpairmentLoss)),
    ]
}

/// 工业列报行（分类结果；全期间读投影，正负号 = 借贷方向，负债/收入行以
/// 正数呈现其贷方金额，费用行以正数呈现其损失属性）。
#[derive(Clone, Eq, PartialEq, Debug)]
pub struct IndustrialPresentationLines {
    /// 货币资金（1001 + 1002）。
    pub cash_position: AccountingAmount,
    /// 应收账款净额（1122 − 1231 坏账准备）。
    pub receivables_net: AccountingAmount,
    /// 存货（1403 原材料 + 1405 库存商品 + 5001 在产品）。
    pub inventory_total: AccountingAmount,
    /// 固定资产净额（1601 − 1602 − 1603）。
    pub fixed_assets_net: AccountingAmount,
    /// 递延所得税资产（1811）。
    pub deferred_tax_assets: AccountingAmount,
    /// 短期借款（2001）。
    pub short_term_debt: AccountingAmount,
    /// 应付账款（2202）。
    pub payables: AccountingAmount,
    /// 应交税费净额（销项 − 进项 + 应交所得税；净借方 = 留抵，如实为负）。
    pub taxes_payable_net: AccountingAmount,
    /// 应付利息（2231）。
    pub interest_payable: AccountingAmount,
    /// 长期借款（2501）。
    pub long_term_debt: AccountingAmount,
    /// 营业收入（6001 贷方余额）。
    pub operating_revenue: AccountingAmount,
    /// 营业成本（6401 借方余额）。
    pub operating_cost: AccountingAmount,
    /// 销售费用（6601）。
    pub selling_expense: AccountingAmount,
    /// 管理费用（6602，不含研发）。
    pub administrative_expense: AccountingAmount,
    /// 研发费用（660201，财会〔2018〕15号单列口径）。
    pub research_expense: AccountingAmount,
    /// 财务费用（6603）。
    pub finance_expense: AccountingAmount,
    /// 资产减值损失（6701）。
    pub impairment_loss: AccountingAmount,
}

/// 由总账读净借方余额（负值 = 贷方余额）。
fn debit(ledger: &Ledger, code: &str) -> Result<AccountingAmount, AccountingError> {
    ledger.account_net_debit(&LedgerAccountId(code.to_string()))
}

/// 工业列报分类：总账 → 列报行（纯读投影，不修改任何状态）。
pub fn industrial_presentation_lines(
    ledger: &Ledger,
) -> Result<IndustrialPresentationLines, AccountingError> {
    let credit_of =
        |code: &str| -> Result<AccountingAmount, AccountingError> { debit(ledger, code)?.neg() };
    let receivable = debit(ledger, "1122")?;
    let bad_debt = credit_of(codes::BAD_DEBT_ALLOW)?;
    let acc_dep = debit(ledger, "1602")?;
    let acc_impair = credit_of(codes::ACC_IMPAIR)?;
    Ok(IndustrialPresentationLines {
        cash_position: debit(ledger, "1001")?.add(debit(ledger, "1002")?)?,
        receivables_net: receivable.sub(bad_debt)?,
        inventory_total: debit(ledger, codes::RAW_MATERIAL)?
            .add(debit(ledger, codes::FINISHED_GOODS)?)?
            .add(debit(ledger, codes::WIP)?)?,
        fixed_assets_net: debit(ledger, "1601")?.add(acc_dep)?.sub(acc_impair)?,
        deferred_tax_assets: debit(ledger, codes::DTA)?,
        short_term_debt: credit_of("2001")?,
        payables: credit_of("2202")?,
        taxes_payable_net: credit_of("2221")?
            .add(credit_of(codes::VAT_OUT)?)?
            .add(credit_of(codes::VAT_IN)?)?
            .add(credit_of(codes::CIT_PAYABLE)?)?,
        interest_payable: credit_of("2231")?,
        long_term_debt: credit_of(codes::LT_DEBT)?,
        operating_revenue: credit_of("6001")?,
        operating_cost: debit(ledger, "6401")?,
        selling_expense: debit(ledger, codes::SELLING_EXP)?,
        administrative_expense: debit(ledger, "6602")?,
        research_expense: debit(ledger, codes::RND_EXP)?,
        finance_expense: debit(ledger, "6603")?,
        impairment_loss: debit(ledger, codes::IMPAIR_LOSS)?,
    })
}

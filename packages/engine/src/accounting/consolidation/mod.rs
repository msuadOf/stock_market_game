//! 固定集团合并（K3，任务 12）：单体汇总 + 工作底稿抵销 + 少数股东。
//!
//! 输入是 task-7 公司规格的会计域镜像（`MemberSpec`：`group_parent` +
//! `issued_shares` + 母公司持股股数——accounting 不得 import company，
//! 由调用方从注册表派生；任务 13/26 接线）。四种行业账套
//! （`IndustrialBooks`/`BankBooks`/`InsuranceBooks`/`RealEstateBooks`）都
//! 暴露 `books()`，混合行业集团因此合法。
//!
//! 流水线（每步先于下一步，任一失败无部分产物）：
//! 1. [`group`]：固定图校验（重复/成环/根/单层/持股数学）→ 无子公司
//!    类型化 `NotApplicable`；
//! 2. [`aggregate`]：期间覆盖一致 + 跨科目表按代码加总；
//! 3. [`eliminate`]：内部往来/内部销售申报校验 → 工作底稿分录并应用到
//!    汇总余额（**绝不回记成员账套**，集团现金逐分不变）；
//! 4. [`minority`]：调整后成员经济量 × 少数比例 → 拆分与合并总量。
//!
//! 明确不在范围（K3 边界）：并购/股权交易、权益法、变动持股、多层集团、
//! 亏损内部交易、完整报表生成（任务 13 消费本模块输出）。

mod aggregate;
mod eliminate;
mod error;
mod group;
mod minority;
mod sale;
mod worksheet;

pub use aggregate::ConsolidatedBalance;
pub use error::{ConsolidationError, DeclaredSide};
pub use group::{GroupMember, MemberId, MemberSpec, ScopeId, SubsidiaryOwnership};
pub use minority::MinorityInterest;
pub use worksheet::{
    IntercompanyBalance, IntercompanySale, WorksheetEntry, WorksheetLine, WorksheetReason,
};

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;

/// 合并请求：根成员 + 全部成员（根 + 直接子公司）+ 内部交易申报。
///
/// 成员顺序任意（内部按 `MemberId` 排序处理，输出稳定）；申报顺序保留
/// （工作底稿分录顺序 = 申报顺序）。
#[derive(Debug)]
pub struct ConsolidationRequest<'a> {
    pub root: MemberId,
    pub members: Vec<GroupMember<'a>>,
    pub intercompany_balances: Vec<IntercompanyBalance>,
    pub intercompany_sales: Vec<IntercompanySale>,
}

/// 合并结果：工作底稿 + 调整后余额 + 少数股东拆分 + 合并总量。
///
/// 所有金额都是**调整后**（含抵销）口径；`scope` 显式标记合并范围
/// （K3：单体与合并分别标记，任务 13 据此生成报表）。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct ConsolidationOutput {
    /// 范围标记（合并，携带母公司 id）。
    pub scope: ScopeId,
    /// 全部成员（按 id 排序）。
    pub members: Vec<MemberId>,
    /// 调整后合并余额（科目代码 → T 型合计 + 抵销；仅有发生额科目）。
    pub adjusted_balances: BTreeMap<crate::accounting::LedgerAccountId, ConsolidatedBalance>,
    /// 工作底稿抵销分录（申报顺序）。
    pub worksheet: Vec<WorksheetEntry>,
    /// 各子公司少数股东拆分（子公司 id 序）。
    pub minority: Vec<MinorityInterest>,
    /// 合并现金（== Σ 成员现金，逐分相等——抵销不触现金）。
    pub consolidated_cash: AccountingAmount,
    /// 合并净利（Σ 调整后成员净利）。
    pub consolidated_net_income: AccountingAmount,
    /// 归母净利 = 合并净利 − Σ 少数股东损益。
    pub net_income_to_parent: AccountingAmount,
    /// 少数股东损益合计。
    pub net_income_to_minority: AccountingAmount,
    /// 合并权益滚动（Σ 调整后成员权益滚动）。
    pub consolidated_equity: AccountingAmount,
    /// 归母权益 = 合并权益 − Σ 少数股东权益。
    pub equity_to_parent: AccountingAmount,
    /// 少数股东权益合计。
    pub minority_equity_total: AccountingAmount,
}

/// 执行固定集团合并（纯函数：只读成员账套，无任何写路径）。
pub fn consolidate(
    request: ConsolidationRequest<'_>,
) -> Result<ConsolidationOutput, ConsolidationError> {
    // 成员表（重复 id = 重复合并，先于一切检查拒绝）。
    let mut specs: BTreeMap<MemberId, MemberSpec> = BTreeMap::new();
    let mut books: BTreeMap<MemberId, &crate::accounting::Books> = BTreeMap::new();
    for member in &request.members {
        if specs
            .insert(member.spec.id.clone(), member.spec.clone())
            .is_some()
        {
            return Err(ConsolidationError::DuplicateMember {
                member: member.spec.id.clone(),
            });
        }
        books.insert(member.spec.id.clone(), member.books);
    }
    let group = group::validate_group(&request.root, &specs)?;
    aggregate::check_period_coverage(&books)?;
    let mut balances = aggregate::aggregate_balances(&books)?;
    let worksheet = eliminate::build_worksheet(
        &group,
        &books,
        &request.intercompany_balances,
        &request.intercompany_sales,
    )?;
    eliminate::apply_worksheet(&mut balances, &worksheet)?;
    let economics = minority::adjusted_economics(&books, &worksheet)?;
    let minority = minority::summarize(&group, &economics)?;

    // 合并总量（Σ 全员；溢出与舍入语义与底座一致）。
    let mut net_income = AccountingAmount::ZERO;
    let mut equity = AccountingAmount::ZERO;
    let mut cash = AccountingAmount::ZERO;
    for member in economics.values() {
        net_income = net_income.add(member.net_income)?;
        equity = equity.add(member.equity_rolling)?;
    }
    for account in balances.values() {
        if account.def.is_cash {
            cash = cash.add(account.net_debit()?)?;
        }
    }
    let mut minority_ni = AccountingAmount::ZERO;
    let mut minority_equity = AccountingAmount::ZERO;
    for interest in &minority {
        minority_ni = minority_ni.add(interest.minority_net_income)?;
        minority_equity = minority_equity.add(interest.minority_equity)?;
    }
    Ok(ConsolidationOutput {
        scope: ScopeId::Consolidated(request.root),
        members: specs.into_keys().collect(),
        adjusted_balances: balances,
        worksheet,
        minority,
        consolidated_cash: cash,
        consolidated_net_income: net_income,
        net_income_to_parent: net_income.sub(minority_ni)?,
        net_income_to_minority: minority_ni,
        consolidated_equity: equity,
        equity_to_parent: equity.sub(minority_equity)?,
        minority_equity_total: minority_equity,
    })
}

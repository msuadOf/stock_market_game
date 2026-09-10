//! 少数股东损益与权益（K3，任务 12）。
//!
//! 持股比例来自固定集团关系（group.rs 精确基点）；少数份额 = 调整后成员
//! 经济量 × minority_bp（单次基点应用，半偶舍入）；归母份额 = 合并总量 −
//! Σ 少数份额（减法精确，无守恒缺口）。
//!
//! 「调整后」指**工作底稿抵销之后**的成员口径——上游未实现利润冲减卖方
//! （子公司）的收入/成本，其调整后净利与权益滚动相应减少，少数股东因此
//! 自然分担；下游冲减母公司，少数份额不受影响（金样对照断言锁定）。
//!
//! 口径说明：本游戏固定控制关系无股权投资账面计量（开局前既成控制、无
//! 并购交易），故无「长期股权投资 ↔ 子公司权益」抵销；少数权益 = 子公司
//! 调整后权益滚动 × 少数比例。权益法/变动持股不在范围（K3：仅全额合并）。

use std::collections::BTreeMap;

use crate::accounting::amount::AccountingAmount;
use crate::accounting::journal::PostingSide;
use crate::accounting::ledger::AccountElement;
use crate::accounting::Books;

use super::error::ConsolidationError;
use super::group::{MemberId, SubsidiaryOwnership, ValidatedGroup};
use super::worksheet::WorksheetEntry;

/// 单成员调整后经济量（工作底稿抵销后的净利与权益滚动）。
#[derive(Clone, Copy, Eq, PartialEq, Debug)]
pub(crate) struct MemberEconomics {
    pub net_income: AccountingAmount,
    pub equity_rolling: AccountingAmount,
}

/// 每家子公司的少数股东拆分明细。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct MinorityInterest {
    pub subsidiary: MemberId,
    /// 母公司持股（基点，精确）。
    pub parent_ownership_bp: i64,
    /// 少数股东（基点）。
    pub minority_bp: i64,
    /// 抵销后子公司净利。
    pub subsidiary_adjusted_net_income: AccountingAmount,
    /// 抵销后子公司权益滚动。
    pub subsidiary_adjusted_equity: AccountingAmount,
    /// 少数股东损益 = 调整后净利 × minority_bp（半偶舍入）。
    pub minority_net_income: AccountingAmount,
    /// 少数股东权益 = 调整后权益滚动 × minority_bp（半偶舍入）。
    pub minority_equity: AccountingAmount,
}

/// 基点 → i32：构造上 bp = held×10000/issued 且 0 < held ≤ issued（u64），
/// 故 bp ∈ (0, 10000]、minority_bp ∈ [0, 10000)，恒在 i32 值域内。
fn bp_i32(bp: i64) -> i32 {
    i32::try_from(bp).expect("basis points are bounded to [0, 10000] by construction")
}

/// 逐成员计算调整后经济量（总账口径 + 工作底稿行效应）。
///
/// 行效应按要素折算：ΔNI = Σ(费用行 Δnet_debit) − Σ(收入行 Δnet_debit)；
/// Δ权益科目 = −Σ(权益行 Δnet_debit)；Δ权益滚动 = Δ权益科目 + ΔNI。
pub(crate) fn adjusted_economics(
    members: &BTreeMap<MemberId, &Books>,
    worksheet: &[WorksheetEntry],
) -> Result<BTreeMap<MemberId, MemberEconomics>, ConsolidationError> {
    let mut result = BTreeMap::new();
    for (id, books) in members {
        let ledger = books.ledger();
        let mut ni_delta = AccountingAmount::ZERO;
        let mut equity_account_delta = AccountingAmount::ZERO;
        for line in worksheet.iter().flat_map(|entry| entry.lines.iter()) {
            if line.member != *id {
                continue;
            }
            let Some(def) = ledger.chart().get(&line.account) else {
                // 工作底稿行在生成时已校验过科目存在；此处防御内部不一致。
                return Err(ConsolidationError::UnknownIntercompanyAccount {
                    member: line.member.clone(),
                    account: line.account.clone(),
                });
            };
            let delta = match line.side {
                PostingSide::Debit => line.amount,
                PostingSide::Credit => line.amount.neg()?,
            };
            match def.element {
                // NI = (−Σ收入净借) − (Σ费用净借)：两类行的 ΔNI 都是 −Δ净借。
                AccountElement::Revenue | AccountElement::Expense => {
                    ni_delta = ni_delta.sub(delta)?;
                }
                AccountElement::Equity => equity_account_delta = equity_account_delta.sub(delta)?,
                AccountElement::Asset | AccountElement::Liability => {}
            }
        }
        let net_income = ledger.net_income()?.add(ni_delta)?;
        let equity_rolling = ledger
            .equity_rolling()?
            .add(equity_account_delta)?
            .add(ni_delta)?;
        result.insert(
            id.clone(),
            MemberEconomics {
                net_income,
                equity_rolling,
            },
        );
    }
    Ok(result)
}

/// 计算各子公司少数股东拆分（子公司顺序 = 集团校验后的 id 序）。
pub(crate) fn summarize(
    group: &ValidatedGroup,
    economics: &BTreeMap<MemberId, MemberEconomics>,
) -> Result<Vec<MinorityInterest>, ConsolidationError> {
    let mut minority = Vec::new();
    for SubsidiaryOwnership {
        id,
        parent_ownership_bp,
        minority_bp,
        ..
    } in &group.subsidiaries
    {
        let Some(member) = economics.get(id) else {
            return Err(ConsolidationError::UnknownIntercompanyMember { member: id.clone() });
        };
        minority.push(MinorityInterest {
            subsidiary: id.clone(),
            parent_ownership_bp: *parent_ownership_bp,
            minority_bp: *minority_bp,
            subsidiary_adjusted_net_income: member.net_income,
            subsidiary_adjusted_equity: member.equity_rolling,
            minority_net_income: member.net_income.apply_basis_points(bp_i32(*minority_bp))?,
            minority_equity: member
                .equity_rolling
                .apply_basis_points(bp_i32(*minority_bp))?,
        });
    }
    Ok(minority)
}

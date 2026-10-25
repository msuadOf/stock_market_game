//! 消费已冻结的重新计值事实，生成 PreNewUnitMarket 候选；不执行真实账户入账。
//!
//! 本模块不生成碎股政策、不表示普通 A 股拆缩股产品已通过制度审核。
//! 新单位成交必须在此候选之后另行处理，不能先混入原单位 registry。

use crate::account::StockCode;
use crate::calendar::CivilDate;
use crate::company::cash_dividend_tax::CashDividendTaxBook;
use crate::company::issuer_repurchase::SplitUnitContext;
use crate::company::share_registry::ShareRegistry;
use crate::company::share_split::ShareSplitBook;
use crate::orderbook::AccountId;

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct TreasuryBucketTransform {
    pub key: String,
    pub original_lot_ids: Vec<String>,
    pub shares_before: u64,
    pub shares_after: u64,
    /// initial 库存不自动归属真实回购账户；真实 plan 桶须显式关联。
    pub account: Option<AccountId>,
}

pub(crate) struct ShareTransformDayEndInput<'a> {
    pub day: CivilDate,
    pub book: &'a ShareSplitBook,
    pub registry_in_registered_units: &'a ShareRegistry,
    pub dividend_tax_books: &'a [CashDividendTaxBook],
    pub unit_chain: &'a [SplitUnitContext],
    pub treasury_buckets: &'a [TreasuryBucketTransform],
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ShareTransformAmount {
    ShareCredit { qty: u64 },
    ShareReduction { qty: u64 },
}

/// 成本累计事实保留；只有实际账户最终零仓才清除成本，不能以登记份额为零推定。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ShareTransformCostPolicy {
    PreserveUnlessActualPositionCloses,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct AccountShareTransformIntent {
    pub event_id: String,
    pub account: AccountId,
    pub stock: StockCode,
    pub source_bucket: Option<String>,
    pub registered_before: u64,
    pub converted_after: u64,
    pub amount: ShareTransformAmount,
    pub cost_policy: ShareTransformCostPolicy,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum ShareTransformCandidatePhase {
    PreNewUnitMarket,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct ShareTransformDayEndCandidate {
    pub phase: ShareTransformCandidatePhase,
    pub stock: StockCode,
    pub unit_event_id: String,
    pub registry_before_new_unit_market: ShareRegistry,
    pub dividend_tax_books_before_new_unit_market: Vec<CashDividendTaxBook>,
    pub settled_book: ShareSplitBook,
    pub account_intents: Vec<AccountShareTransformIntent>,
    pub treasury_adjustments: Vec<TreasuryBucketTransform>,
}

#[derive(Debug, thiserror::Error)]
#[error("重新计值候选 {event_id} 在 {day} 失败：{detail}")]
pub(crate) struct ShareTransformDayEndError {
    pub event_id: String,
    pub day: CivilDate,
    pub detail: String,
}

pub(crate) fn share_transform_day_end_candidate(
    input: ShareTransformDayEndInput<'_>,
) -> Result<ShareTransformDayEndCandidate, ShareTransformDayEndError> {
    Err(ShareTransformDayEndError {
        event_id: input.book.plan().event_id.clone(),
        day: input.day,
        detail: "候选尚未实现".into(),
    })
}

#[cfg(test)]
#[path = "share_transform_day_end_tests.rs"]
mod tests;

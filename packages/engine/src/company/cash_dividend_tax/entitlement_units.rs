//! 原登记现金权益在明确单位映射下的纯评税候选；尚未接入生产日终/存档。
//!
//! 来源事实由调用方提供且须由后续受控结算生产者证明。此入口不改变旧
//! `outstanding`、真实账户或生产中的个人送股能力门。

use super::*;
use serde::{Deserialize, Serialize};

/// 原权益/当前内部来源份额；不是可直接交易的碎股。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ExactShareUnits(ExactDividendTaxAmount);
impl ExactShareUnits {
    pub fn new(numerator: i128, denominator: u64) -> Result<Self, DividendTaxError> {
        ExactDividendTaxAmount::new(numerator, denominator).map(Self)
    }
    pub fn numerator(&self) -> i128 {
        self.0.numerator()
    }
    pub fn denominator(&self) -> u64 {
        self.0.denominator()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum TaxIncomeKind {
    CashDividend,
    StockBonus,
}

/// 所得身份以种类和事件共同区分，不用字符串前缀猜相位。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxEntitlementKey {
    pub kind: TaxIncomeKind,
    pub event_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxOriginSpan {
    pub lot_id: String,
    pub start: ExactShareUnits,
    pub end: ExactShareUnits,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxCurrentSpan {
    pub lot_id: String,
    pub start: ExactShareUnits,
    pub end: ExactShareUnits,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxUnitFragment {
    pub from: TaxOriginSpan,
    pub to: TaxCurrentSpan,
}

/// `operation_seq` 必须指向税账真实的单位核减或显式 CorporateAction 正向取得。
/// 同时保存逐来源区间和目标区间；两端都必须构成完整、不重叠 partition。
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxUnitTransition {
    pub event_id: String,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    pub operation_seq: u64,
    pub day: CivilDate,
    pub from_unit_event: Option<String>,
    pub before_lots: Vec<DividendTaxLot>,
    pub after_lots: Vec<DividendTaxLot>,
    pub fragments: Vec<TaxUnitFragment>,
    pub non_disposal_retirements: Vec<TaxOriginSpan>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaxUnitLineage {
    pub account: AccountId,
    pub stock: StockCode,
    pub transitions: Vec<TaxUnitTransition>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaxEntitlementConsumption {
    pub key: TaxEntitlementKey,
    pub original: TaxOriginSpan,
    pub disposition_event_id: String,
    pub disposed_on: CivilDate,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TaxEntitlementRetirement {
    pub key: TaxEntitlementKey,
    pub original: TaxOriginSpan,
    pub unit_event_id: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MappedTaxAssessment {
    pub outstanding: ExactDividendTaxAmount,
    pub consumptions: Vec<TaxEntitlementConsumption>,
    /// 保留未处置退休权益，不冒充免税、已结案或现金补偿。
    pub retirements: Vec<TaxEntitlementRetirement>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PartitionSide {
    Origin,
    Current,
}

#[derive(Debug, thiserror::Error)]
pub enum TaxLineageError {
    #[error(transparent)]
    Tax(#[from] DividendTaxError),
    #[error("税务单位映射账户/证券不符：expected {expected_account:?}/{expected_stock:?}, actual {actual_account:?}/{actual_stock:?}")]
    ContextMismatch {
        expected_account: AccountId,
        actual_account: AccountId,
        expected_stock: StockCode,
        actual_stock: StockCode,
    },
    #[error("单位事件 {event_id} 序号{operation_seq} 缺少明确来源映射")]
    MissingTransition {
        event_id: String,
        operation_seq: u64,
    },
    #[error("单位事件 {event_id} 身份重用但事实不同")]
    ConflictingTransition { event_id: String },
    #[error("单位事件 {event_id} 的 {side:?} lot {lot_id} partition 无效：{detail}")]
    InvalidPartition {
        event_id: String,
        side: PartitionSide,
        lot_id: String,
        detail: &'static str,
    },
    #[error("单位事件 {event_id} 与真实税账前后事实不一致")]
    TransitionMismatch { event_id: String },
    #[error("单位事件 {event_id} 的前驱 {actual:?} 与当前 {expected:?} 不同")]
    UnitPredecessorMismatch {
        event_id: String,
        expected: Option<String>,
        actual: Option<String>,
    },
    #[error("单位事件 {event_id} 操作序不指向明确的公司单位事实")]
    InvalidOperation {
        event_id: String,
        operation_seq: u64,
    },
    #[error("本候选不接旧送股新 lot 税基：{event_id}")]
    UnsupportedLegacyBonusBasis { event_id: String },
}

impl CashDividendTaxBook {
    /// 只读准备明确来源下的原登记现金权益评税；未接生产 `outstanding`/税款划收。
    pub fn assessment_with_unit_lineage(
        &self,
        _lineage: &TaxUnitLineage,
    ) -> Result<MappedTaxAssessment, TaxLineageError> {
        Ok(MappedTaxAssessment {
            outstanding: self.outstanding()?,
            consumptions: vec![],
            retirements: vec![],
        })
    }
}

#[cfg(test)]
mod tests;

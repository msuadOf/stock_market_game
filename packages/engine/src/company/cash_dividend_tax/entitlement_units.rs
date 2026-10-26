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
    #[serde(deserialize_with = "required_predecessor")]
    pub from_unit_event: Option<String>,
    pub before_lots: Vec<DividendTaxLot>,
    pub after_lots: Vec<DividendTaxLot>,
    pub fragments: Vec<TaxUnitFragment>,
    pub non_disposal_retirements: Vec<TaxOriginSpan>,
}

fn required_predecessor<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Option<String>, D::Error> {
    Option::<String>::deserialize(decoder)
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

// carrier 仅在重放期间存在；original 始终是登记单位，current 随转换和处置重定位。
#[derive(Clone)]
struct EntitlementCarrier {
    key: TaxEntitlementKey,
    original: TaxOriginSpan,
    current: TaxCurrentSpan,
}

impl CashDividendTaxBook {
    /// 只读准备明确来源下的原登记现金权益评税；未接生产 `outstanding`/税款划收。
    pub fn assessment_with_unit_lineage(
        &self,
        lineage: &TaxUnitLineage,
    ) -> Result<MappedTaxAssessment, TaxLineageError> {
        self.validate()?;
        if lineage.account != self.account || lineage.stock != self.stock {
            return Err(TaxLineageError::ContextMismatch {
                expected_account: self.account,
                actual_account: lineage.account,
                expected_stock: self.stock.clone(),
                actual_stock: lineage.stock.clone(),
            });
        }
        if let Some(basis) = self.stock_bonus_bases.first() {
            return Err(TaxLineageError::UnsupportedLegacyBonusBasis {
                event_id: basis.event_id.clone(),
            });
        }
        let mut transitions = std::collections::BTreeMap::new();
        let mut identities = std::collections::BTreeMap::new();
        for transition in &lineage.transitions {
            if let Some(previous) = identities.insert(&transition.event_id, transition) {
                if previous != transition {
                    return Err(TaxLineageError::ConflictingTransition {
                        event_id: transition.event_id.clone(),
                    });
                }
                continue;
            }
            let reduction = self.redenominations.iter().any(|receipt| {
                receipt.operation_seq == transition.operation_seq
                    && receipt.event_id == transition.event_id
                    && receipt.day == transition.day
            });
            let corporate_credit = self.days.iter().any(|receipt| {
                receipt.operation_seq == transition.operation_seq
                    && receipt.day == transition.day
                    && receipt.net_change > 0
                    && matches!(&receipt.acquisition, Some(lot) if lot.source ==
                        TaxAcquisitionSource::CorporateAction { event: transition.event_id.clone() })
            });
            if transition.event_id.trim().is_empty()
                || (!reduction && !corporate_credit)
                || transitions
                    .insert(transition.operation_seq, transition)
                    .is_some()
            {
                return Err(TaxLineageError::InvalidOperation {
                    event_id: transition.event_id.clone(),
                    operation_seq: transition.operation_seq,
                });
            }
        }
        let mut lots = self.opening_lots.clone();
        let mut carriers = Vec::new();
        let mut consumptions = Vec::new();
        let mut retirements = Vec::new();
        let mut predecessor = None;
        for sequence in 1..=self.operation_seq {
            let before = lots.clone();
            if let Some(day) = self.days.iter().find(|day| day.operation_seq == sequence) {
                // 旧 CorporateAction 来源未区分单位扩张与独立取得。缺受控分类时不能
                // 默认新增权益，否则1拆2后的部分卖出会按错误原单位计税。
                if !transitions.contains_key(&sequence) {
                    if let Some(DividendTaxLot {
                        source: TaxAcquisitionSource::CorporateAction { event },
                        ..
                    }) = &day.acquisition
                    {
                        return Err(TaxLineageError::MissingTransition {
                            event_id: event.clone(),
                            operation_seq: sequence,
                        });
                    }
                }
                apply_net_change(&mut lots, day.day, day.net_change, day.acquisition.as_ref())?;
                if !transitions.contains_key(&sequence) {
                    consume_day(&mut carriers, &mut consumptions, day)?;
                }
            }
            if let Some(reduction) = self
                .redenominations
                .iter()
                .find(|r| r.operation_seq == sequence)
            {
                apply_redenomination_reduction(
                    &mut lots,
                    reduction.day,
                    u64::try_from(-reduction.net_change)
                        .map_err(|_| invalid("redenomination exceeds u64"))?,
                )?;
                if !transitions.contains_key(&sequence) {
                    return Err(TaxLineageError::MissingTransition {
                        event_id: reduction.event_id.clone(),
                        operation_seq: sequence,
                    });
                }
            }
            if let Some(transition) = transitions.get(&sequence) {
                if transition.before_lots != before || transition.after_lots != lots {
                    return Err(TaxLineageError::TransitionMismatch {
                        event_id: transition.event_id.clone(),
                    });
                }
                if transition.from_unit_event != predecessor {
                    return Err(TaxLineageError::UnitPredecessorMismatch {
                        event_id: transition.event_id.clone(),
                        expected: predecessor,
                        actual: transition.from_unit_event.clone(),
                    });
                }
                validate_partition(transition, PartitionSide::Origin)?;
                validate_partition(transition, PartitionSide::Current)?;
                carriers = transport(&carriers, transition, &mut retirements)?;
                predecessor = Some(transition.event_id.clone());
            }
            if let Some(dividend) = self.dividends.iter().find(|d| d.operation_seq == sequence) {
                let key = cash_key(&dividend.event_id);
                for lot in &dividend.lots {
                    let end = ExactShareUnits::new(i128::from(lot.qty), 1)?;
                    carriers.push(EntitlementCarrier {
                        key: key.clone(),
                        original: TaxOriginSpan {
                            lot_id: lot.id.clone(),
                            start: units_zero(),
                            end: end.clone(),
                        },
                        current: TaxCurrentSpan {
                            lot_id: lot.id.clone(),
                            start: units_zero(),
                            end,
                        },
                    });
                }
            }
        }
        let assessed = self.assessed_cash_cents_with_consumptions(&consumptions)?;
        let collected = self.collections.iter().try_fold(0i128, |total, receipt| {
            total
                .checked_add(i128::from(receipt.collected.cents()))
                .ok_or_else(|| overflow("collected tax sum"))
        })?;
        Ok(MappedTaxAssessment {
            outstanding: ExactDividendTaxAmount::new(
                assessed
                    .checked_sub(collected)
                    .ok_or_else(|| overflow("mapped outstanding subtraction"))?,
                1,
            )?,
            consumptions,
            retirements,
        })
    }

    fn assessed_cash_cents_with_consumptions(
        &self,
        consumptions: &[TaxEntitlementConsumption],
    ) -> Result<i128, DividendTaxError> {
        self.assessed_cash_cents_through(
            self.settled_on,
            self.operation_seq,
            |dividend, lot, per_share, start, assessed| {
                let key = cash_key(&dividend.event_id);
                for consumption in consumptions
                    .iter()
                    .filter(|c| c.key == key && c.original.lot_id == lot.id)
                {
                    let quantity =
                        units_sub(&consumption.original.end, &consumption.original.start)?;
                    let rate = personal_cash_dividend_rate(start, consumption.disposed_on)?;
                    let tax = units_product(&[
                        ExactShareUnits(per_share.clone()),
                        quantity,
                        ExactShareUnits::new(i128::from(rate), 100)?,
                    ])?;
                    *assessed = assessed.add(&tax.0)?;
                }
                Ok(())
            },
        )
    }
}

fn cash_key(event_id: &str) -> TaxEntitlementKey {
    TaxEntitlementKey {
        kind: TaxIncomeKind::CashDividend,
        event_id: event_id.into(),
    }
}
fn units_zero() -> ExactShareUnits {
    ExactShareUnits(ExactDividendTaxAmount::zero())
}
fn units_cmp(a: &ExactShareUnits, b: &ExactShareUnits) -> std::cmp::Ordering {
    use num_bigint::BigUint;
    (BigUint::from(a.numerator() as u128) * b.denominator())
        .cmp(&(BigUint::from(b.numerator() as u128) * a.denominator()))
}
fn normalized_units(
    mut numerator: num_bigint::BigUint,
    mut denominator: num_bigint::BigUint,
) -> Result<ExactShareUnits, DividendTaxError> {
    let mut a = numerator.clone();
    let mut b = denominator.clone();
    while b != num_bigint::BigUint::from(0u8) {
        let remainder = &a % &b;
        a = b;
        b = remainder;
    }
    if a == num_bigint::BigUint::from(0u8) {
        return Err(invalid("exact share denominator must be positive"));
    }
    numerator /= &a;
    denominator /= a;
    ExactShareUnits::new(
        i128::try_from(numerator).map_err(|_| overflow("exact share numerator"))?,
        u64::try_from(denominator).map_err(|_| overflow("exact share denominator"))?,
    )
}
fn units_sub(
    a: &ExactShareUnits,
    b: &ExactShareUnits,
) -> Result<ExactShareUnits, DividendTaxError> {
    use num_bigint::BigUint;
    if units_cmp(a, b).is_lt() {
        return Err(invalid("negative exact share interval"));
    }
    normalized_units(
        BigUint::from(a.numerator() as u128) * b.denominator()
            - BigUint::from(b.numerator() as u128) * a.denominator(),
        BigUint::from(a.denominator()) * b.denominator(),
    )
}
fn units_add(
    a: &ExactShareUnits,
    b: &ExactShareUnits,
) -> Result<ExactShareUnits, DividendTaxError> {
    use num_bigint::BigUint;
    normalized_units(
        BigUint::from(a.numerator() as u128) * b.denominator()
            + BigUint::from(b.numerator() as u128) * a.denominator(),
        BigUint::from(a.denominator()) * b.denominator(),
    )
}
fn units_product(factors: &[ExactShareUnits]) -> Result<ExactShareUnits, DividendTaxError> {
    let mut n = num_bigint::BigUint::from(1u8);
    let mut d = n.clone();
    for factor in factors {
        n *= factor.numerator() as u128;
        d *= factor.denominator();
    }
    normalized_units(n, d)
}
/// 半开区间求交；无交集是正常几何结果，不是错误 fallback。
fn intersection(
    a: &ExactShareUnits,
    b: &ExactShareUnits,
    c: &ExactShareUnits,
    d: &ExactShareUnits,
) -> Option<(ExactShareUnits, ExactShareUnits)> {
    let start = if units_cmp(a, c).is_gt() { a } else { c };
    let end = if units_cmp(b, d).is_lt() { b } else { d };
    units_cmp(start, end)
        .is_lt()
        .then(|| (start.clone(), end.clone()))
}
/// 在同一冻结线性片段上反投影/正投影，不重新选择来源。
fn project(
    point: &ExactShareUnits,
    from_start: &ExactShareUnits,
    from_end: &ExactShareUnits,
    to_start: &ExactShareUnits,
    to_end: &ExactShareUnits,
) -> Result<ExactShareUnits, DividendTaxError> {
    use num_bigint::BigUint;
    let offset = units_sub(point, from_start)?;
    let from_len = units_sub(from_end, from_start)?;
    let to_len = units_sub(to_end, to_start)?;
    if from_len.numerator() == 0 {
        return Err(invalid("empty projection source"));
    }
    let scaled = normalized_units(
        BigUint::from(offset.numerator() as u128)
            * to_len.numerator() as u128
            * from_len.denominator(),
        BigUint::from(offset.denominator()) * to_len.denominator() * from_len.numerator() as u128,
    )?;
    units_add(to_start, &scaled)
}

fn validate_partition(t: &TaxUnitTransition, side: PartitionSide) -> Result<(), TaxLineageError> {
    let lots = match side {
        PartitionSide::Origin => &t.before_lots,
        PartitionSide::Current => &t.after_lots,
    };
    let mut spans = Vec::new();
    for fragment in &t.fragments {
        spans.push(match side {
            PartitionSide::Origin => (
                &fragment.from.lot_id,
                &fragment.from.start,
                &fragment.from.end,
            ),
            PartitionSide::Current => (&fragment.to.lot_id, &fragment.to.start, &fragment.to.end),
        });
    }
    if side == PartitionSide::Origin {
        for span in &t.non_disposal_retirements {
            spans.push((&span.lot_id, &span.start, &span.end));
        }
    }
    let bad = |lot_id: &str, detail| TaxLineageError::InvalidPartition {
        event_id: t.event_id.clone(),
        side,
        lot_id: lot_id.into(),
        detail,
    };
    for (id, start, end) in &spans {
        let lot = lots
            .iter()
            .find(|lot| lot.id == **id)
            .ok_or_else(|| bad(id, "引用不存在的 lot"))?;
        if !units_cmp(start, end).is_lt()
            || units_cmp(end, &ExactShareUnits::new(i128::from(lot.qty), 1)?).is_gt()
        {
            return Err(bad(id, "区间为空或超出 lot 容量"));
        }
    }
    for lot in lots {
        let mut pieces: Vec<_> = spans.iter().filter(|(id, _, _)| **id == lot.id).collect();
        pieces.sort_by(|a, b| units_cmp(a.1, b.1));
        let mut frontier = units_zero();
        for (_, start, end) in pieces {
            if **start != frontier {
                return Err(bad(&lot.id, "区间存在缺口或重叠"));
            }
            frontier = (*end).clone();
        }
        if frontier != ExactShareUnits::new(i128::from(lot.qty), 1)? {
            return Err(bad(&lot.id, "区间未完整覆盖 lot"));
        }
    }
    Ok(())
}

fn original_slice(
    carrier: &EntitlementCarrier,
    start: &ExactShareUnits,
    end: &ExactShareUnits,
) -> Result<TaxOriginSpan, DividendTaxError> {
    Ok(TaxOriginSpan {
        lot_id: carrier.original.lot_id.clone(),
        start: project(
            start,
            &carrier.current.start,
            &carrier.current.end,
            &carrier.original.start,
            &carrier.original.end,
        )?,
        end: project(
            end,
            &carrier.current.start,
            &carrier.current.end,
            &carrier.original.start,
            &carrier.original.end,
        )?,
    })
}
fn transport(
    carriers: &[EntitlementCarrier],
    t: &TaxUnitTransition,
    retired: &mut Vec<TaxEntitlementRetirement>,
) -> Result<Vec<EntitlementCarrier>, DividendTaxError> {
    let mut result = Vec::new();
    for carrier in carriers {
        for fragment in t
            .fragments
            .iter()
            .filter(|f| f.from.lot_id == carrier.current.lot_id)
        {
            if let Some((start, end)) = intersection(
                &carrier.current.start,
                &carrier.current.end,
                &fragment.from.start,
                &fragment.from.end,
            ) {
                result.push(EntitlementCarrier {
                    key: carrier.key.clone(),
                    original: original_slice(carrier, &start, &end)?,
                    current: TaxCurrentSpan {
                        lot_id: fragment.to.lot_id.clone(),
                        start: project(
                            &start,
                            &fragment.from.start,
                            &fragment.from.end,
                            &fragment.to.start,
                            &fragment.to.end,
                        )?,
                        end: project(
                            &end,
                            &fragment.from.start,
                            &fragment.from.end,
                            &fragment.to.start,
                            &fragment.to.end,
                        )?,
                    },
                });
            }
        }
        for span in t
            .non_disposal_retirements
            .iter()
            .filter(|s| s.lot_id == carrier.current.lot_id)
        {
            if let Some((start, end)) = intersection(
                &carrier.current.start,
                &carrier.current.end,
                &span.start,
                &span.end,
            ) {
                retired.push(TaxEntitlementRetirement {
                    key: carrier.key.clone(),
                    original: original_slice(carrier, &start, &end)?,
                    unit_event_id: t.event_id.clone(),
                });
            }
        }
    }
    Ok(result)
}
fn consume_day(
    carriers: &mut Vec<EntitlementCarrier>,
    consumed: &mut Vec<TaxEntitlementConsumption>,
    day: &TaxDayReceipt,
) -> Result<(), DividendTaxError> {
    for disposition in &day.dispositions {
        let boundary = ExactShareUnits::new(i128::from(disposition.lot.qty), 1)?;
        let mut remaining = Vec::new();
        for carrier in carriers.drain(..) {
            if carrier.current.lot_id != disposition.lot.id {
                remaining.push(carrier);
                continue;
            }
            if let Some((start, end)) = intersection(
                &carrier.current.start,
                &carrier.current.end,
                &units_zero(),
                &boundary,
            ) {
                consumed.push(TaxEntitlementConsumption {
                    key: carrier.key.clone(),
                    original: original_slice(&carrier, &start, &end)?,
                    disposition_event_id: day.event_id.clone(),
                    disposed_on: day.day,
                });
            }
            if units_cmp(&carrier.current.end, &boundary).is_gt() {
                let start = if units_cmp(&carrier.current.start, &boundary).is_gt() {
                    carrier.current.start.clone()
                } else {
                    boundary.clone()
                };
                remaining.push(EntitlementCarrier {
                    key: carrier.key.clone(),
                    original: original_slice(&carrier, &start, &carrier.current.end)?,
                    current: TaxCurrentSpan {
                        lot_id: carrier.current.lot_id.clone(),
                        start: units_sub(&start, &boundary)?,
                        end: units_sub(&carrier.current.end, &boundary)?,
                    },
                });
            }
        }
        *carriers = remaining;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

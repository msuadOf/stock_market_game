//! Bonus 原权益的条件事实；不代表真实发股已安装或生产税务来源已闭合。
use super::*;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalBonusTaxFacts {
    pub events: Vec<ConditionalBonusTaxEvent>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalBonusTaxEvent {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    pub after_book_operation: u64,
    pub day: CivilDate,
    pub action: ConditionalBonusTaxAction,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum ConditionalBonusTaxAction {
    RegisterBonus {
        event_id: String,
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        shares_per_existing_share_micros: u64,
        par_value_per_share: Money,
    },
    RealizeBonus {
        event_id: String,
        actual_credit: ConditionalBonusCredit,
        #[serde(deserialize_with = "required_tax_acquisition")]
        tax_acquisition: Option<u64>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConditionalBonusCredit {
    pub event_id: String,
    pub account: AccountId,
    pub stock: StockCode,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub credited_qty: u64,
}
fn required_tax_acquisition<'de, D: serde::Deserializer<'de>>(
    decoder: D,
) -> Result<Option<u64>, D::Error> {
    Option::<u64>::deserialize(decoder)
}

use std::collections::{BTreeMap, BTreeSet};
struct RegisteredBonus {
    lots: Vec<DividendTaxLot>,
    ratio: u64,
    par: Money,
    realization: Option<(CivilDate, i128)>,
}
pub(super) struct BonusReplay<'a> {
    events: Vec<&'a ConditionalBonusTaxEvent>,
    acquisitions: BTreeSet<u64>,
    registered: BTreeMap<String, RegisteredBonus>,
}
fn bad(event_id: &str, detail: &'static str) -> TaxLineageError {
    TaxLineageError::InvalidConditionalFact {
        event_id: event_id.into(),
        detail,
    }
}
impl<'a> BonusReplay<'a> {
    pub(super) fn new(
        book: &CashDividendTaxBook,
        facts: &'a ConditionalBonusTaxFacts,
    ) -> Result<Self, TaxLineageError> {
        let mut days = BTreeMap::from([(0, book.opened_on)]);
        for d in &book.days {
            days.insert(d.operation_seq, d.day);
        }
        for d in &book.redenominations {
            days.insert(d.operation_seq, d.day);
        }
        for d in &book.dividends {
            days.insert(d.operation_seq, d.registered_on);
            for p in &d.payments {
                days.insert(p.operation_seq, p.paid_on);
            }
        }
        for c in &book.collections {
            days.insert(c.operation_seq, c.day);
        }
        for b in &book.stock_bonus_bases {
            days.insert(b.operation_seq, b.credited_on);
        }
        let mut identities = BTreeMap::new();
        let mut registrations = BTreeMap::new();
        let mut acquisitions = BTreeSet::new();
        let mut events = Vec::new();
        let mut previous_prefix = 0;
        let mut previous_day = book.opened_on;
        for event in &facts.events {
            let (phase, id) = match &event.action {
                ConditionalBonusTaxAction::RegisterBonus { event_id, .. } => (0u8, event_id),
                ConditionalBonusTaxAction::RealizeBonus { event_id, .. } => (1u8, event_id),
            };
            let lower = days
                .get(&event.after_book_operation)
                .ok_or_else(|| bad(id, "未知book操作prefix"))?;
            let upper = days
                .get(&event.after_book_operation.saturating_add(1))
                .copied()
                .unwrap_or(book.settled_on);
            if id.trim().is_empty()
                || event.after_book_operation < previous_prefix
                || event.day < previous_day
                || event.day < *lower
                || event.day > upper
                || event.day > book.settled_on
            {
                return Err(bad(id, "条件事件顺序、日期或prefix无效"));
            }
            previous_prefix = event.after_book_operation;
            previous_day = event.day;
            if let Some(previous) = identities.insert((phase, id), event) {
                if previous != event {
                    return Err(bad(id, "同typed动作身份重用但事实不同"));
                }
                continue;
            }
            match &event.action {
                ConditionalBonusTaxAction::RegisterBonus {
                    shares_per_existing_share_micros,
                    par_value_per_share,
                    ..
                } => {
                    if event.day != *lower
                        || *shares_per_existing_share_micros == 0
                        || par_value_per_share.cents() <= 0
                    {
                        return Err(bad(id, "完整R日期、正比例与冻结正面值必需"));
                    }
                    registrations.insert(id, event);
                }
                ConditionalBonusTaxAction::RealizeBonus {
                    actual_credit,
                    tax_acquisition,
                    ..
                } => {
                    let registration = registrations
                        .get(id)
                        .ok_or_else(|| bad(id, "所得激活之前缺原R登记"))?;
                    if actual_credit.event_id != *id
                        || actual_credit.account != book.account
                        || actual_credit.stock != book.stock
                        || actual_credit.credited_qty == 0
                    {
                        return Err(bad(id, "实际条件发股的event、account、stock或数量不符"));
                    }
                    if let Some(sequence) = tax_acquisition {
                        let day = book
                            .days
                            .iter()
                            .find(|day| day.operation_seq == *sequence)
                            .ok_or_else(|| bad(id, "税务独立取得引用不存在"))?;
                        if *sequence <= registration.after_book_operation
                            || day.net_change <= 0
                            || !matches!(&day.acquisition,Some(lot) if lot.source==TaxAcquisitionSource::CorporateAction{event:(*id).clone()})
                            || !acquisitions.insert(*sequence)
                        {
                            return Err(bad(id, "税务独立取得分类与来源不符或重复使用"));
                        }
                    }
                }
            }
            events.push(event);
        }
        Ok(Self {
            events,
            acquisitions,
            registered: BTreeMap::new(),
        })
    }
    pub(super) fn classifies(&self, sequence: u64) -> bool {
        self.acquisitions.contains(&sequence)
    }
    pub(super) fn apply_at(
        &mut self,
        sequence: u64,
        lots: &[DividendTaxLot],
        carriers: &mut Vec<EntitlementCarrier>,
    ) -> Result<(), TaxLineageError> {
        for event in self
            .events
            .iter()
            .filter(|e| e.after_book_operation == sequence)
        {
            match &event.action {
                ConditionalBonusTaxAction::RegisterBonus {
                    event_id,
                    shares_per_existing_share_micros,
                    par_value_per_share,
                } => {
                    if lots.is_empty() {
                        return Err(bad(event_id, "完整R无原权益"));
                    }
                    let key = TaxEntitlementKey {
                        kind: TaxIncomeKind::StockBonus,
                        event_id: event_id.clone(),
                    };
                    register_carriers(&key, lots, carriers)?;
                    self.registered.insert(
                        event_id.clone(),
                        RegisteredBonus {
                            lots: lots.to_vec(),
                            ratio: *shares_per_existing_share_micros,
                            par: *par_value_per_share,
                            realization: None,
                        },
                    );
                }
                ConditionalBonusTaxAction::RealizeBonus {
                    event_id,
                    actual_credit,
                    ..
                } => {
                    let registered = self
                        .registered
                        .get_mut(event_id)
                        .ok_or_else(|| bad(event_id, "缺少已重放原R"))?;
                    let original_qty = total_quantity(&registered.lots)?;
                    let product = u128::from(original_qty) * u128::from(registered.ratio);
                    if product % 1_000_000 != 0
                        || product / 1_000_000 != u128::from(actual_credit.credited_qty)
                    {
                        return Err(TaxLineageError::UnsupportedBonusAllocation {
                            event_id: event_id.clone(),
                            original_qty,
                            ratio_micros: registered.ratio,
                            actual_qty: actual_credit.credited_qty,
                        });
                    }
                    let gross = i128::from(actual_credit.credited_qty)
                        .checked_mul(i128::from(registered.par.cents()))
                        .ok_or_else(|| overflow("actual stock bonus gross income"))?;
                    registered.realization = Some((event.day, gross));
                }
            }
        }
        Ok(())
    }
    pub(super) fn assessed(
        &self,
        consumptions: &[TaxEntitlementConsumption],
    ) -> Result<i128, DividendTaxError> {
        let mut total = 0i128;
        for (event_id, registration) in &self.registered {
            let key = TaxEntitlementKey {
                kind: TaxIncomeKind::StockBonus,
                event_id: event_id.clone(),
            };
            let cents = assess_registered_income_cents(
                &registration.lots,
                registration.realization,
                |lot, per_share, start, assessed| {
                    assess_mapped_disposals(&key, lot, per_share, start, consumptions, assessed)
                },
            )?;
            total = total
                .checked_add(cents)
                .ok_or_else(|| overflow("stock bonus assessed tax cents"))?;
        }
        Ok(total)
    }
}

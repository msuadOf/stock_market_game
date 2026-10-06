use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::share_registry::{HolderId, RegistrationSnapshot, ShareLot};
use super::CompanyId;
use crate::{account::StockCode, calendar::CivilDate};

const RATIO_DENOMINATOR: u64 = 1_000_000;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum StockDistributionKind {
    BonusShares,
    CapitalReserveConversion,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDistributionPlan {
    pub event_id: String,
    pub approval_reference: String,
    pub kind: StockDistributionKind,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub shares_per_existing_share_micros: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub approved_total_new_shares: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HolderDistribution {
    pub holder: HolderId,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub original_shares: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub whole_shares: u64,
    pub fractional_numerator: u64,
    pub original_lots: Vec<ShareLot>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub enum SourceLotAttribution {
    SourceLotAttributionPending,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StockDistributionReceipt {
    pub event_id: String,
    pub approval_reference: String,
    pub registration_event_id: String,
    pub stock: StockCode,
    pub issuer: CompanyId,
    pub registered_on: CivilDate,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub issued_shares_before: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub issuer_treasury_shares_excluded: u64,
    pub kind: StockDistributionKind,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub shares_per_existing_share_micros: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub approved_total_new_shares: u64,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    pub tie_break_seed: u64,
    pub source_lot_attribution: SourceLotAttribution,
    pub holders: Vec<HolderDistribution>,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum StockDistributionError {
    #[error("stock distribution: {detail}")]
    InvalidPlan { detail: String },
    #[error("stock distribution: approved total {approved} is outside feasible interval {minimum}..={maximum}")]
    ApprovedTotalInfeasible {
        approved: u64,
        minimum: u64,
        maximum: u64,
    },
    #[error("stock distribution: integer arithmetic overflow")]
    ArithmeticOverflow,
}

pub fn allocate_stock_distribution(
    snapshot: &RegistrationSnapshot,
    plan: &StockDistributionPlan,
    tie_break_seed: u64,
) -> Result<StockDistributionReceipt, StockDistributionError> {
    snapshot
        .validate()
        .map_err(|error| StockDistributionError::InvalidPlan {
            detail: format!("invalid registration snapshot: {error}"),
        })?;
    if plan.event_id.trim().is_empty() || plan.approval_reference.trim().is_empty() {
        return Err(invalid(
            "distribution event identity and approval reference are required",
        ));
    }
    if plan.shares_per_existing_share_micros == 0 {
        return Err(invalid("share distribution ratio must be positive"));
    }

    let mut grouped = BTreeMap::<HolderId, (u64, Vec<ShareLot>)>::new();
    for holding in snapshot.holdings() {
        let entry = grouped.entry(holding.holder.clone()).or_default();
        for lot in &holding.lots {
            entry.0 = entry
                .0
                .checked_add(lot.qty)
                .ok_or(StockDistributionError::ArithmeticOverflow)?;
            entry.1.push(lot.clone());
        }
    }

    let treasury_shares = grouped
        .get(&HolderId::IssuerTreasury)
        .map(|(shares, _)| *shares)
        .unwrap_or(0);
    if matches!(&plan.kind, StockDistributionKind::CapitalReserveConversion)
        && treasury_shares > 0
        && snapshot.issuer_repurchase_account().is_none()
    {
        return Err(invalid(
            "capital reserve conversion requires registered issuer repurchase account facts to exclude issuer treasury shares",
        ));
    }
    let issuer_treasury_shares_excluded = grouped
        .remove(&HolderId::IssuerTreasury)
        .map(|(shares, _)| shares)
        .unwrap_or(0);
    let mut holders = Vec::with_capacity(grouped.len());
    let mut base_total = 0_u64;
    let mut maximum_total = 0_u64;
    let mut eligible_shares = 0_u64;
    for (holder, (original_shares, original_lots)) in grouped {
        eligible_shares = eligible_shares
            .checked_add(original_shares)
            .ok_or(StockDistributionError::ArithmeticOverflow)?;
        let product = u128::from(original_shares)
            .checked_mul(u128::from(plan.shares_per_existing_share_micros))
            .ok_or(StockDistributionError::ArithmeticOverflow)?;
        let whole = u64::try_from(product / u128::from(RATIO_DENOMINATOR))
            .map_err(|_| StockDistributionError::ArithmeticOverflow)?;
        let remainder = u64::try_from(product % u128::from(RATIO_DENOMINATOR))
            .map_err(|_| StockDistributionError::ArithmeticOverflow)?;
        base_total = base_total
            .checked_add(whole)
            .ok_or(StockDistributionError::ArithmeticOverflow)?;
        maximum_total = maximum_total
            .checked_add(
                whole
                    .checked_add(u64::from(remainder > 0))
                    .ok_or(StockDistributionError::ArithmeticOverflow)?,
            )
            .ok_or(StockDistributionError::ArithmeticOverflow)?;
        holders.push(HolderDistribution {
            holder,
            original_shares,
            whole_shares: whole,
            fractional_numerator: remainder,
            original_lots,
        });
    }

    let aggregate_product = u128::from(eligible_shares)
        .checked_mul(u128::from(plan.shares_per_existing_share_micros))
        .ok_or(StockDistributionError::ArithmeticOverflow)?;
    let aggregate_floor = u64::try_from(aggregate_product / u128::from(RATIO_DENOMINATOR))
        .map_err(|_| StockDistributionError::ArithmeticOverflow)?;
    let aggregate_ceil = aggregate_floor
        .checked_add(u64::from(
            aggregate_product % u128::from(RATIO_DENOMINATOR) > 0,
        ))
        .ok_or(StockDistributionError::ArithmeticOverflow)?;
    let minimum = base_total.max(aggregate_floor);
    let maximum = maximum_total.min(aggregate_ceil);
    if minimum > maximum
        || plan.approved_total_new_shares < minimum
        || plan.approved_total_new_shares > maximum
    {
        return Err(StockDistributionError::ApprovedTotalInfeasible {
            approved: plan.approved_total_new_shares,
            minimum,
            maximum,
        });
    }
    let extra = plan.approved_total_new_shares - base_total;
    let mut candidates: Vec<usize> = holders
        .iter()
        .enumerate()
        .filter_map(|(index, holder)| (holder.fractional_numerator > 0).then_some(index))
        .collect();
    candidates.sort_by(|left, right| {
        holders[*right]
            .fractional_numerator
            .cmp(&holders[*left].fractional_numerator)
    });
    shuffle_equal_remainders(&mut candidates, &holders, tie_break_seed);
    for index in candidates
        .into_iter()
        .take(usize::try_from(extra).map_err(|_| StockDistributionError::ArithmeticOverflow)?)
    {
        holders[index].whole_shares = holders[index]
            .whole_shares
            .checked_add(1)
            .ok_or(StockDistributionError::ArithmeticOverflow)?;
    }

    Ok(StockDistributionReceipt {
        event_id: plan.event_id.clone(),
        approval_reference: plan.approval_reference.clone(),
        registration_event_id: snapshot.event_id().to_owned(),
        stock: snapshot.stock().clone(),
        issuer: snapshot.issuer().clone(),
        registered_on: snapshot.registered_on(),
        issued_shares_before: snapshot.issued_shares(),
        issuer_treasury_shares_excluded,
        kind: plan.kind.clone(),
        shares_per_existing_share_micros: plan.shares_per_existing_share_micros,
        approved_total_new_shares: plan.approved_total_new_shares,
        tie_break_seed,
        source_lot_attribution: SourceLotAttribution::SourceLotAttributionPending,
        holders,
    })
}

fn shuffle_equal_remainders(candidates: &mut [usize], holders: &[HolderDistribution], seed: u64) {
    let mut random = SeededRandom(seed);
    let mut start = 0;
    while start < candidates.len() {
        let remainder = holders[candidates[start]].fractional_numerator;
        let mut end = start + 1;
        while end < candidates.len() && holders[candidates[end]].fractional_numerator == remainder {
            end += 1;
        }
        for index in (start + 1..end).rev() {
            let swap = start + random.index(index - start + 1);
            candidates.swap(index, swap);
        }
        start = end;
    }
}

struct SeededRandom(u64);

impl SeededRandom {
    fn index(&mut self, upper_exclusive: usize) -> usize {
        let upper = u64::try_from(upper_exclusive)
            .expect("candidate group length fits the u64 index domain");
        let rejection_floor = upper.wrapping_neg() % upper;
        loop {
            let value = self.next_u64();
            if value >= rejection_floor {
                return usize::try_from(value % upper)
                    .expect("bounded result fits the candidate group length");
            }
        }
    }

    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut value = self.0;
        value = (value ^ (value >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        value = (value ^ (value >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        value ^ (value >> 31)
    }
}

fn invalid(detail: &str) -> StockDistributionError {
    StockDistributionError::InvalidPlan {
        detail: detail.to_owned(),
    }
}

#[cfg(test)]
#[path = "stock_distribution_tests.rs"]
mod tests;

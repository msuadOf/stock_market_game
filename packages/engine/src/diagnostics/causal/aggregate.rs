use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn seconds_between(end: CivilInstant, start: CivilInstant) -> i64 {
    end.date().days_since(start.date()) * 86_400 + i64::from(end.second_of_day())
        - i64::from(start.second_of_day())
}

impl CausalReport {
    pub fn from_facts(seed: u64, facts: &[CausalFact]) -> Result<Self, CausalError> {
        let mut orders = BTreeMap::<OrderId, OrderLifecycle>::new();
        let mut fills = BTreeSet::new();
        let mut information_delays = Vec::new();
        let mut market_volume = 0_u64;
        let mut unmatched_fills = BTreeMap::<OrderId, (u64, i128)>::new();
        for (index, fact) in facts.iter().enumerate() {
            if fact.sequence != index as u64 {
                return Err(CausalError::Sequence(fact.sequence));
            }
            match &fact.kind {
                CausalFactKind::ObservationRestart => return Err(CausalError::RestoredObservation),
                CausalFactKind::Budget {
                    available_cents,
                    allocated_cents,
                    ..
                } => {
                    let sum = allocated_cents.iter().try_fold(0_i64, |sum, value| {
                        sum.checked_add(*value).ok_or(CausalError::Overflow)
                    })?;
                    if allocated_cents.iter().any(|value| *value < 0) || sum > *available_cents {
                        return Err(CausalError::Budget(fact.sequence));
                    }
                }
                CausalFactKind::Submitted(origin) => {
                    if let Some(decision) = origin.decision {
                        if decision >= fact.sequence || !facts.iter().any(|source| source.sequence == decision && matches!(source.kind, CausalFactKind::Decision { account } if account == origin.account)) {
                            return Err(CausalError::OrderMismatch(origin.order));
                        }
                    }
                    if orders.contains_key(&origin.order) {
                        return Err(CausalError::DuplicateOrder(origin.order));
                    }
                    orders.insert(
                        origin.order,
                        OrderLifecycle {
                            origin: origin.clone(),
                            submitted_at: fact.time,
                            source_sequence: fact.sequence,
                            filled_qty: 0,
                            canceled_qty: 0,
                            aborted_qty: 0,
                            open_qty: u64::from(origin.qty),
                            terminal_reason: None,
                            lifetime_market_minutes: None,
                            lifetime_civil_seconds: None,
                            censored_reason: Some("still_open_at_observation_end"),
                            filled_value: 0,
                        },
                    );
                }
                CausalFactKind::Filled {
                    order,
                    account,
                    code,
                    qty,
                    value_before,
                    gross,
                } => {
                    let row = orders
                        .get_mut(order)
                        .ok_or(CausalError::OrderMismatch(*order))?;
                    if row.origin.account != *account || row.origin.code != *code {
                        return Err(CausalError::OrderMismatch(*order));
                    }
                    if *qty == 0
                        || !fills.insert((*order, *value_before))
                        || row.filled_value != *value_before
                        || *gross <= 0
                    {
                        return Err(CausalError::FillMismatch(*order));
                    }
                    row.filled_value = row
                        .filled_value
                        .checked_add(*gross)
                        .ok_or(CausalError::Overflow)?;
                    row.filled_qty = row
                        .filled_qty
                        .checked_add(u64::from(*qty))
                        .ok_or(CausalError::Overflow)?;
                    row.open_qty = row
                        .open_qty
                        .checked_sub(u64::from(*qty))
                        .ok_or(CausalError::Conservation(*order))?;
                    let pending = unmatched_fills.entry(*order).or_insert((0, 0));
                    pending.0 = pending
                        .0
                        .checked_add(u64::from(*qty))
                        .ok_or(CausalError::Overflow)?;
                    pending.1 = pending
                        .1
                        .checked_add(i128::from(*gross))
                        .ok_or(CausalError::Overflow)?;
                    finish(row, fact)?;
                }
                CausalFactKind::Terminated {
                    order,
                    account,
                    code,
                    qty,
                    reason,
                } => {
                    let row = orders
                        .get_mut(order)
                        .ok_or(CausalError::OrderMismatch(*order))?;
                    if row.origin.account != *account || row.origin.code != *code {
                        return Err(CausalError::OrderMismatch(*order));
                    }
                    if row.open_qty != u64::from(*qty) || row.open_qty == 0 {
                        return Err(CausalError::Conservation(*order));
                    }
                    match reason {
                        Termination::Aborted => row.aborted_qty = u64::from(*qty),
                        Termination::Voluntary
                        | Termination::Reprice
                        | Termination::Expired
                        | Termination::DayEnd
                        | Termination::MarketRemainder => row.canceled_qty = u64::from(*qty),
                    }
                    row.open_qty = 0;
                    row.terminal_reason = Some(*reason);
                    finish(row, fact)?;
                }
                CausalFactKind::Acquisition {
                    published,
                    acquired,
                    ..
                } => {
                    let seconds = seconds_between(*acquired, *published);
                    if seconds < 0 {
                        return Err(CausalError::Time(fact.sequence));
                    }
                    information_delays.push((fact.sequence, seconds));
                }
                CausalFactKind::Execution {
                    qty,
                    maker,
                    taker,
                    code,
                    price_cents,
                    side,
                    ..
                } => {
                    if *qty == 0 || *price_cents <= 0 || maker == taker {
                        return Err(CausalError::ExecutionMismatch);
                    }
                    let maker_row = orders
                        .get(maker)
                        .ok_or(CausalError::OrderMismatch(*maker))?;
                    let taker_row = orders
                        .get(taker)
                        .ok_or(CausalError::OrderMismatch(*taker))?;
                    if maker_row.origin.side == taker_row.origin.side
                        || side.is_some_and(|side| side != taker_row.origin.side)
                    {
                        return Err(CausalError::ExecutionMismatch);
                    }
                    for id in [maker, taker] {
                        if orders.get(id).is_none_or(|row| row.origin.code != *code) {
                            return Err(CausalError::OrderMismatch(*id));
                        }
                        let pending = unmatched_fills
                            .get_mut(id)
                            .ok_or(CausalError::ExecutionMismatch)?;
                        pending.0 = pending
                            .0
                            .checked_sub(u64::from(*qty))
                            .ok_or(CausalError::ExecutionMismatch)?;
                        pending.1 -= i128::from(*price_cents) * i128::from(*qty);
                    }
                    market_volume = market_volume
                        .checked_add(u64::from(*qty))
                        .ok_or(CausalError::Overflow)?;
                }
                CausalFactKind::Quote(_) | CausalFactKind::Decision { .. } => {}
            }
        }
        if unmatched_fills.values().any(|pending| *pending != (0, 0)) {
            return Err(CausalError::ExecutionMismatch);
        }
        let sum = |project: fn(&OrderLifecycle) -> u64| {
            orders.values().try_fold(0_u64, |total, row| {
                total.checked_add(project(row)).ok_or(CausalError::Overflow)
            })
        };
        let submitted_qty = sum(|row| u64::from(row.origin.qty))?;
        let filled_qty = sum(|row| row.filled_qty)?;
        let canceled_qty = sum(|row| row.canceled_qty)?;
        let aborted_qty = sum(|row| row.aborted_qty)?;
        let open_qty = sum(|row| row.open_qty)?;
        let accounted = filled_qty
            .checked_add(canceled_qty)
            .and_then(|value| value.checked_add(aborted_qty))
            .and_then(|value| value.checked_add(open_qty))
            .ok_or(CausalError::Overflow)?;
        if accounted != submitted_qty {
            return Err(CausalError::ExecutionMismatch);
        }
        if market_volume.checked_mul(2).ok_or(CausalError::Overflow)? != filled_qty {
            return Err(CausalError::ExecutionMismatch);
        }
        let (direction_persistence, impacts, recoveries) = microstructure::analyze(facts)?;
        Ok(Self {
            seed,
            submitted_qty,
            filled_qty,
            canceled_qty,
            aborted_qty,
            open_qty,
            filled_submitted_ratio: (submitted_qty > 0)
                .then(|| filled_qty as f64 / submitted_qty as f64),
            ratio_absent_reason: (submitted_qty == 0).then_some("no_submissions"),
            orders: orders.into_values().collect(),
            information_delays,
            direction_persistence,
            direction_absent_reason: direction_persistence
                .is_none()
                .then_some("fewer_than_two_active_fills_per_stock"),
            impacts,
            recoveries,
            observation_end: facts.last().map(|fact| fact.time),
        })
    }
}

fn finish(row: &mut OrderLifecycle, fact: &CausalFact) -> Result<(), CausalError> {
    if row.open_qty == 0 {
        row.lifetime_market_minutes = Some(
            fact.time
                .market_minute
                .checked_sub(row.submitted_at.market_minute)
                .ok_or(CausalError::Time(fact.sequence))?,
        );
        let seconds = seconds_between(fact.time.civil, row.submitted_at.civil);
        if seconds < 0 {
            return Err(CausalError::Time(fact.sequence));
        }
        row.lifetime_civil_seconds = Some(seconds);
        row.censored_reason = None;
    }
    Ok(())
}

use super::{PlayerWorkingOrder, ProtocolError, TickBatch, TickFrame};
use crate::session::StepFatal;
use crate::{AccountId, AccountSnap, GameSession, Side, TradingPhase};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PlayerOrderDelta {
    pub reset: bool,
    pub upserts: Vec<PlayerWorkingOrder>,
    #[serde(with = "safe_order_ids")]
    #[ts(type = "Array<number>")]
    pub removed: Vec<u64>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RuntimeDelta {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq_from: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq_to: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub tick: u64,
    pub day: u32,
    pub phase: TradingPhase,
    pub accounts: BTreeMap<AccountId, AccountSnap>,
    pub working_orders: PlayerOrderDelta,
}

#[derive(Clone)]
pub struct PublicRuntimeState {
    tick: u64,
    seq: u64,
    day: u32,
    phase: TradingPhase,
    accounts: BTreeMap<AccountId, AccountSnap>,
    working_orders: BTreeMap<u64, PlayerWorkingOrder>,
}

impl RuntimeDelta {
    pub fn validate(&self) -> Result<(), ProtocolError> {
        if self.seq_to < self.seq_from {
            return Err(ProtocolError::RuntimeDeltaMismatch);
        }
        validate_accounts(&self.accounts)?;
        let mut ids = BTreeSet::new();
        for order in &self.working_orders.upserts {
            validate_order(order)?;
            if !ids.insert(order.id) {
                return Err(ProtocolError::RuntimeDeltaMismatch);
            }
        }
        if self.working_orders.reset && !self.working_orders.removed.is_empty() {
            return Err(ProtocolError::RuntimeDeltaMismatch);
        }
        for id in &self.working_orders.removed {
            if !ids.insert(*id) {
                return Err(ProtocolError::RuntimeDeltaMismatch);
            }
        }
        Ok(())
    }
}

fn validate_accounts(accounts: &BTreeMap<AccountId, AccountSnap>) -> Result<(), ProtocolError> {
    for (id, account) in accounts {
        if *id != AccountId(0)
            || account.cash.cents() < 0
            || account.reserved_cash.cents() < 0
            || account.reserved_cash > account.cash
            || account.positions.values().any(|position| {
                position.t1_locked > position.qty
                    || position.invested_cents < 0
                    || position.recovered_cents < 0
            })
            || account.reserved_sell_qty.iter().any(|(code, reserved)| {
                account.positions.get(code).is_none_or(|position| {
                    *reserved > position.qty.saturating_sub(position.t1_locked)
                })
            })
        {
            return Err(ProtocolError::RuntimeDeltaMismatch);
        }
    }
    Ok(())
}

fn validate_order(order: &PlayerWorkingOrder) -> Result<(), ProtocolError> {
    let frozen = match order.side {
        Side::Buy => "cash",
        Side::Sell => "shares",
    };
    if order.code.0.is_empty()
        || order.price.cents() <= 0
        || order.remaining_qty == 0
        || !matches!(order.venue.as_str(), "auction" | "continuous")
        || order.frozen != frozen
    {
        return Err(ProtocolError::RuntimeDeltaMismatch);
    }
    Ok(())
}

fn same_account(left: &AccountSnap, right: &AccountSnap) -> bool {
    left.cash == right.cash
        && left.reserved_cash == right.reserved_cash
        && left.reserved_sell_qty == right.reserved_sell_qty
        && left.positions.len() == right.positions.len()
        && left.positions.iter().all(|(code, position)| {
            right.positions.get(code).is_some_and(|other| {
                position.qty == other.qty
                    && position.t1_locked == other.t1_locked
                    && position.invested_cents == other.invested_cents
                    && position.recovered_cents == other.recovered_cents
            })
        })
}

fn same_order(left: &PlayerWorkingOrder, right: &PlayerWorkingOrder) -> bool {
    left.id == right.id
        && left.code == right.code
        && left.side == right.side
        && left.price == right.price
        && left.remaining_qty == right.remaining_qty
        && left.venue == right.venue
        && left.frozen == right.frozen
}

fn fatal(error: ProtocolError) -> StepFatal {
    StepFatal::InvariantViolation {
        location: "public runtime delta publication".into(),
        description: error.to_string(),
    }
}

impl GameSession {
    pub fn public_runtime_state(&self) -> Result<PublicRuntimeState, StepFatal> {
        self.require_healthy()?;
        let snapshot = self.runtime_snapshot();
        validate_accounts(&snapshot.accounts).map_err(fatal)?;
        if !snapshot.accounts.contains_key(&AccountId(0)) {
            return Err(fatal(ProtocolError::RuntimeDeltaMismatch));
        }
        let mut working_orders = BTreeMap::new();
        for order in self.player_working_orders() {
            validate_order(&order).map_err(fatal)?;
            if working_orders.insert(order.id, order).is_some() {
                return Err(fatal(ProtocolError::RuntimeDeltaMismatch));
            }
        }
        Ok(PublicRuntimeState {
            tick: snapshot.tick,
            seq: snapshot.seq,
            day: snapshot.day,
            phase: snapshot.phase,
            accounts: snapshot.accounts,
            working_orders,
        })
    }

    pub fn tick_batch_delta(
        &self,
        frames: Vec<TickFrame>,
        previous: Option<&PublicRuntimeState>,
    ) -> Result<(TickBatch, PublicRuntimeState), StepFatal> {
        let first = frames
            .first()
            .ok_or_else(|| fatal(ProtocolError::EmptyBatch))?;
        let current = self.public_runtime_state()?;
        if previous.is_some_and(|before| {
            before.seq != first.seq_from || before.tick.checked_add(1) != Some(first.tick)
        }) {
            return Err(fatal(ProtocolError::RuntimeDeltaMismatch));
        }
        let accounts = current
            .accounts
            .iter()
            .filter(|(id, account)| {
                previous.is_none_or(|before| {
                    before
                        .accounts
                        .get(*id)
                        .is_none_or(|other| !same_account(account, other))
                })
            })
            .map(|(id, account)| (*id, account.clone()))
            .collect();
        let upserts = current
            .working_orders
            .iter()
            .filter(|(id, order)| {
                previous.is_none_or(|before| {
                    before
                        .working_orders
                        .get(*id)
                        .is_none_or(|other| !same_order(order, other))
                })
            })
            .map(|(_, order)| order.clone())
            .collect();
        let removed = match previous {
            Some(before) => before
                .working_orders
                .keys()
                .filter(|id| !current.working_orders.contains_key(*id))
                .copied()
                .collect(),
            None => Vec::new(),
        };
        let batch = TickBatch {
            runtime_snapshot: None,
            runtime_delta: Some(RuntimeDelta {
                seq_from: first.seq_from,
                seq_to: current.seq,
                tick: current.tick,
                day: current.day,
                phase: current.phase,
                accounts,
                working_orders: PlayerOrderDelta {
                    reset: previous.is_none(),
                    upserts,
                    removed,
                },
            }),
            frames,
        };
        batch.validate().map_err(fatal)?;
        Ok((batch, current))
    }
}

mod safe_order_ids {
    use serde::{Deserialize, Serialize};

    const MAX: u64 = 9_007_199_254_740_991;

    pub fn serialize<S: serde::Serializer>(ids: &[u64], serializer: S) -> Result<S::Ok, S::Error> {
        if ids.iter().any(|id| *id > MAX) {
            return Err(serde::ser::Error::custom(
                "removed order ID exceeds JavaScript safe range",
            ));
        }
        ids.serialize(serializer)
    }

    pub fn deserialize<'de, D: serde::Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<u64>, D::Error> {
        let ids = Vec::<u64>::deserialize(deserializer)?;
        if ids.iter().any(|id| *id > MAX) {
            return Err(serde::de::Error::custom(
                "removed order ID exceeds JavaScript safe range",
            ));
        }
        Ok(ids)
    }
}

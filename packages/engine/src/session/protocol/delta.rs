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
    pub removed: Vec<OwnerScopedRemovedOrder>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct OwnerScopedRemovedOrder {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub id: u64,
    pub owner: AccountId,
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

#[cfg(test)]
impl PublicRuntimeState {
    pub(in crate::session::protocol) fn test_projection(&self) -> serde_json::Value {
        serde_json::to_value((
            self.tick,
            self.seq,
            self.day,
            self.phase,
            &self.accounts,
            &self.working_orders,
        ))
        .unwrap()
    }
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
        for order in &self.working_orders.removed {
            if order.id > crate::orderbook::js_safe_u64::MAX || !ids.insert(order.id) {
                return Err(ProtocolError::RuntimeDeltaMismatch);
            }
        }
        Ok(())
    }
}

fn validate_accounts(accounts: &BTreeMap<AccountId, AccountSnap>) -> Result<(), ProtocolError> {
    for account in accounts.values() {
        if account.cash.cents() < 0
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
        && left.owner == right.owner
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
        for order in self.state.accounts.values().filter(|account| account.kind() == crate::AccountKind::Player).flat_map(|account| self.account_working_orders(account.id())) {
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
                .iter()
                .filter(|(id, _)| !current.working_orders.contains_key(*id))
                .map(|(id, order)| OwnerScopedRemovedOrder { id: *id, owner: order.owner })
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

#[cfg(test)]
mod multiplayer_validation_tests {
    use super::*;

    #[test]
    fn runtime_delta_accepts_nonzero_player_accounts_and_keeps_financial_guards() {
        let valid = AccountSnap { cash: crate::Money::from_cents(100000), positions: BTreeMap::new(), reserved_cash: crate::Money::from_cents(1000), reserved_sell_qty: BTreeMap::new() };
        let delta = RuntimeDelta {
            seq_from: 0,
            seq_to: 1,
            tick: 1,
            day: 0,
            phase: TradingPhase::Continuous,
            accounts: BTreeMap::from([(AccountId(0), valid.clone()), (AccountId(2), valid)]),
            working_orders: PlayerOrderDelta { reset: true, upserts: Vec::new(), removed: Vec::new() },
        };
        delta.validate().unwrap();
        let mut negative = delta.clone();
        negative.accounts.get_mut(&AccountId(2)).unwrap().cash = crate::Money::from_cents(-1);
        assert!(matches!(negative.validate(), Err(ProtocolError::RuntimeDeltaMismatch)));
        let mut excessive = delta.clone();
        excessive.accounts.get_mut(&AccountId(2)).unwrap().reserved_cash = crate::Money::from_cents(100001);
        assert!(matches!(excessive.validate(), Err(ProtocolError::RuntimeDeltaMismatch)));
        let mut missing_shares = delta;
        missing_shares.accounts.get_mut(&AccountId(2)).unwrap().reserved_sell_qty.insert(crate::StockCode("600888".into()), 100);
        assert!(matches!(missing_shares.validate(), Err(ProtocolError::RuntimeDeltaMismatch)));
    }

    #[test]
    fn removed_order_identity_requires_strict_owner_string_and_safe_order_id() {
        let removal = OwnerScopedRemovedOrder { id: crate::orderbook::js_safe_u64::MAX, owner: AccountId(u64::MAX) };
        let encoded = serde_json::to_value(&removal).unwrap();
        assert_eq!(encoded, serde_json::json!({ "id": 9007199254740991_u64, "owner": "18446744073709551615" }));
        assert_eq!(serde_json::from_value::<OwnerScopedRemovedOrder>(encoded).unwrap().owner, removal.owner);
        for invalid in [
            serde_json::json!(1),
            serde_json::json!({ "id": 1, "owner": 1 }),
            serde_json::json!({ "id": 1, "owner": "01" }),
            serde_json::json!({ "id": 1 }),
            serde_json::json!({ "id": 9007199254740992_u64, "owner": "1" }),
            serde_json::json!({ "id": 1, "owner": "1", "account": "2" }),
        ] {
            assert!(serde_json::from_value::<OwnerScopedRemovedOrder>(invalid).is_err());
        }
    }
}

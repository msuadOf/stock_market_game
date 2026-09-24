use super::{GameSession, StepFatal, TickShadowPlan};
use crate::session::account_book::AccountBook;
use crate::{AccountId, Money, StockCode};
use rayon::prelude::*;
use std::collections::BTreeMap;

/// Post-P0 resources observed by P2 and reserved by P3. Each account is prepared once.
#[derive(Clone)]
pub struct DecisionResourceSnapshot {
    accounts: AccountBook,
    configured_stocks: BTreeMap<StockCode, Money>,
    reserved_cash: BTreeMap<AccountId, Money>,
    reserved_sell: BTreeMap<AccountId, BTreeMap<StockCode, u32>>,
}

impl std::fmt::Debug for DecisionResourceSnapshot {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("DecisionResourceSnapshot")
            .field("account_count", &self.accounts.len())
            .field("configured_stocks", &self.configured_stocks)
            .field("reserved_cash", &self.reserved_cash)
            .field("reserved_sell", &self.reserved_sell)
            .finish()
    }
}

impl PartialEq for DecisionResourceSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.configured_stocks == other.configured_stocks
            && self.reserved_cash == other.reserved_cash
            && self.reserved_sell == other.reserved_sell
            && self.accounts.len() == other.accounts.len()
            && self.accounts.iter().zip(other.accounts.iter()).all(
                |((left_id, left), (right_id, right))| {
                    left_id == right_id
                        && left.cash == right.cash
                        && left
                            .positions
                            .iter()
                            .filter(|(code, _)| self.configured_stocks.contains_key(*code))
                            .map(|(code, position)| {
                                (
                                    code,
                                    position.qty,
                                    position.sellable(),
                                    position.cost_price(),
                                )
                            })
                            .eq(right
                                .positions
                                .iter()
                                .filter(|(code, _)| other.configured_stocks.contains_key(*code))
                                .map(|(code, position)| {
                                    (
                                        code,
                                        position.qty,
                                        position.sellable(),
                                        position.cost_price(),
                                    )
                                }))
                },
            )
    }
}

impl Eq for DecisionResourceSnapshot {}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct DecisionPositionResources {
    total_qty: u32,
    sellable_before_reservation: u32,
    available_sell_qty: u32,
    cost_price: Option<Money>,
}

const EMPTY_POSITION: DecisionPositionResources = DecisionPositionResources {
    total_qty: 0,
    sellable_before_reservation: 0,
    available_sell_qty: 0,
    cost_price: None,
};

pub(super) fn plan_allocation(shadow: &mut TickShadowPlan) -> Result<(), StepFatal> {
    let resources = shadow
        .state
        .execute(|session| DecisionResourceSnapshot::seal(session))?;
    shadow.decision_resources = Some(resources);
    Ok(())
}

impl DecisionResourceSnapshot {
    pub(super) fn contains_account(&self, account: AccountId) -> bool {
        self.accounts.contains_key(&account)
    }

    pub(super) fn seal(session: &GameSession) -> Result<Self, StepFatal> {
        let configured_stocks = session
            .setup
            .stocks
            .iter()
            .map(|stock| {
                let price = session
                    .markets
                    .get(&stock.code)
                    .ok_or_else(|| invariant("configured stock has no market"))?
                    .last_price();
                Ok((stock.code.clone(), price))
            })
            .collect::<Result<BTreeMap<_, _>, StepFatal>>()?;

        let mut reserved_cash = BTreeMap::<AccountId, Money>::new();
        let mut reserved_sell = BTreeMap::<AccountId, BTreeMap<StockCode, u32>>::new();
        for (key, envelope) in session.envelope_ledger.iter() {
            let live = envelope.live();
            let cash = reserved_cash.entry(key.account).or_insert(Money::ZERO);
            *cash = cash.add(live.cash).map_err(allocation_money)?;
            let shares = reserved_sell
                .entry(key.account)
                .or_default()
                .entry(key.stock.clone())
                .or_insert(0);
            *shares = shares
                .checked_add(live.shares)
                .ok_or_else(|| allocation_invariant("live sell reservation overflow"))?;
        }

        session
            .accounts
            .par_iter()
            .try_for_each(|(account_id, account)| {
                (|| {
                    let reserved = reserved_cash
                        .get(account_id)
                        .copied()
                        .unwrap_or(Money::ZERO);
                    let available_cash = account.cash.sub(reserved).map_err(allocation_money)?;
                    if available_cash.cents() < 0 {
                        return Err(allocation_invariant(
                            "live buy reservation exceeds account cash",
                        ));
                    }

                    let mut equity = account.cash;
                    let reserved_for_account = reserved_sell.get(account_id);
                    for (stock, position) in &account.positions {
                        let Some(price) = configured_stocks.get(stock) else {
                            continue;
                        };
                        let total_qty = position.qty;
                        let sellable_before_reservation = account.sellable_qty(stock);
                        let reserved_shares = reserved_for_account
                            .and_then(|shares| shares.get(stock))
                            .copied()
                            .unwrap_or(0);
                        let available_sell_qty = sellable_before_reservation
                            .checked_sub(reserved_shares)
                            .ok_or_else(|| {
                                allocation_invariant(
                                    "live sell reservation exceeds sellable shares",
                                )
                            })?;
                        equity = equity
                            .add(price.mul_shares(total_qty).map_err(money)?)
                            .map_err(money)?;
                        let _ = available_sell_qty;
                        let _ = account.cost_price(stock);
                    }
                    if reserved_for_account.is_some_and(|shares| {
                        shares.iter().any(|(stock, qty)| {
                            *qty > 0
                                && configured_stocks.contains_key(stock)
                                && !account.positions.contains_key(stock)
                        })
                    }) {
                        return Err(allocation_invariant(
                            "live sell reservation exceeds sellable shares",
                        ));
                    }
                    let _ = equity;
                    Ok(())
                })()
            })?;
        if reserved_cash
            .keys()
            .any(|account| !session.accounts.contains_key(account))
        {
            return Err(allocation_invariant(
                "envelope ledger references an unknown account",
            ));
        }
        if reserved_sell.iter().any(|(account, stocks)| {
            !session.accounts.contains_key(account)
                || stocks
                    .keys()
                    .any(|stock| !configured_stocks.contains_key(stock))
        }) {
            return Err(allocation_invariant(
                "envelope ledger references an unknown account or stock",
            ));
        }
        Ok(Self {
            accounts: session.accounts.clone(),
            configured_stocks,
            reserved_cash,
            reserved_sell,
        })
    }

    pub fn raw_cash(&self, account: AccountId) -> Result<Money, StepFatal> {
        Ok(self.account(account)?.cash)
    }

    pub fn available_cash(&self, account: AccountId) -> Result<Money, StepFatal> {
        self.account(account)?
            .cash
            .sub(*self.reserved_cash.get(&account).unwrap_or(&Money::ZERO))
            .map_err(allocation_money)
    }

    pub fn reserved_cash(&self, account: AccountId) -> Result<Money, StepFatal> {
        self.raw_cash(account)?
            .sub(self.available_cash(account)?)
            .map_err(money)
    }

    pub fn equity(&self, account: AccountId) -> Result<Money, StepFatal> {
        let account = self.account(account)?;
        account
            .positions
            .iter()
            .try_fold(account.cash, |equity, (code, position)| {
                let Some(price) = self.configured_stocks.get(code) else {
                    return Ok(equity);
                };
                equity
                    .add(price.mul_shares(position.qty).map_err(money)?)
                    .map_err(money)
            })
    }

    pub fn total_held_qty(&self, account: AccountId, stock: &StockCode) -> Result<u32, StepFatal> {
        Ok(self.position(account, stock)?.total_qty)
    }

    pub fn sellable_before_reservation(
        &self,
        account: AccountId,
        stock: &StockCode,
    ) -> Result<u32, StepFatal> {
        Ok(self.position(account, stock)?.sellable_before_reservation)
    }

    pub fn available_sell_qty(
        &self,
        account: AccountId,
        stock: &StockCode,
    ) -> Result<u32, StepFatal> {
        Ok(self.position(account, stock)?.available_sell_qty)
    }

    pub fn reserved_sell_qty(
        &self,
        account: AccountId,
        stock: &StockCode,
    ) -> Result<u32, StepFatal> {
        self.sellable_before_reservation(account, stock)?
            .checked_sub(self.available_sell_qty(account, stock)?)
            .ok_or_else(|| invariant("available sell quantity exceeds sellable quantity"))
    }

    pub fn cost_price(
        &self,
        account: AccountId,
        stock: &StockCode,
    ) -> Result<Option<Money>, StepFatal> {
        Ok(self.position(account, stock)?.cost_price)
    }

    fn account(&self, account: AccountId) -> Result<&crate::Account, StepFatal> {
        self.accounts
            .get(&account)
            .ok_or_else(|| invariant("unknown decision resource account"))
    }

    fn position(
        &self,
        account_id: AccountId,
        stock: &StockCode,
    ) -> Result<DecisionPositionResources, StepFatal> {
        let account = self.account(account_id)?;
        if !self.configured_stocks.contains_key(stock) {
            return Err(invariant("unknown decision resource stock"));
        }
        let Some(position) = account.positions.get(stock) else {
            return Ok(EMPTY_POSITION);
        };
        let sellable_before_reservation = account.sellable_qty(stock);
        let reserved_shares = self
            .reserved_sell
            .get(&account_id)
            .and_then(|shares| shares.get(stock))
            .copied()
            .unwrap_or(0);
        let available_sell_qty = sellable_before_reservation
            .checked_sub(reserved_shares)
            .ok_or_else(|| allocation_invariant("live sell reservation exceeds sellable shares"))?;
        Ok(DecisionPositionResources {
            total_qty: position.qty,
            sellable_before_reservation,
            available_sell_qty,
            cost_price: account.cost_price(stock),
        })
    }
}

fn allocation_money(error: crate::MoneyError) -> StepFatal {
    StepFatal::InvariantViolation {
        description: error.to_string(),
        location: "pipeline::decision_resources::allocation".to_owned(),
    }
}

fn allocation_invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::decision_resources::allocation".to_owned(),
    }
}

fn money(error: crate::MoneyError) -> StepFatal {
    StepFatal::InvariantViolation {
        description: error.to_string(),
        location: "pipeline::decision_resources".to_owned(),
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::decision_resources".to_owned(),
    }
}

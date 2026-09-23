use super::{AllocationSnapshot, GameSession, StepFatal};
use crate::{AccountId, Money, StockCode};
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DecisionResourceSnapshot {
    allocation: AllocationSnapshot,
    accounts: BTreeMap<AccountId, DecisionAccountResources>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DecisionAccountResources {
    raw_cash: Money,
    equity: Money,
    positions: BTreeMap<StockCode, DecisionPositionResources>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct DecisionPositionResources {
    total_qty: u32,
    sellable_before_reservation: u32,
    cost_price: Option<Money>,
}

impl DecisionResourceSnapshot {
    pub(super) fn seal(
        session: &GameSession,
        allocation: AllocationSnapshot,
    ) -> Result<Self, StepFatal> {
        let mut accounts = BTreeMap::new();
        for (account_id, account) in &session.accounts {
            let mut equity = account.cash;
            let mut positions = BTreeMap::new();
            for stock in &session.setup.stocks {
                let position = account.positions.get(&stock.code);
                let total_qty = position.map_or(0, |position| position.qty);
                let sellable_before_reservation = account.sellable_qty(&stock.code);
                let price = session
                    .markets
                    .get(&stock.code)
                    .ok_or_else(|| invariant("configured stock has no market"))?
                    .last_price();
                equity = equity
                    .add(price.mul_shares(total_qty).map_err(money)?)
                    .map_err(money)?;
                positions.insert(
                    stock.code.clone(),
                    DecisionPositionResources {
                        total_qty,
                        sellable_before_reservation,
                        cost_price: account.cost_price(&stock.code),
                    },
                );
            }
            accounts.insert(
                *account_id,
                DecisionAccountResources {
                    raw_cash: account.cash,
                    equity,
                    positions,
                },
            );
        }
        Ok(Self {
            allocation,
            accounts,
        })
    }

    /// Confirms that the immutable P1 snapshot still belongs to the same post-P0 resource state.
    /// The sealed snapshot remains authoritative; this comparison only rejects a mismatched or
    /// stale owner before P3 can allocate from it.
    pub(super) fn validate_source_session(
        &self,
        session: &mut GameSession,
    ) -> Result<(), StepFatal> {
        let observed_allocation = session.seal_allocation_snapshot()?;
        let observed = Self::seal(session, observed_allocation)?;
        if &observed != self {
            return Err(StepFatal::InvariantViolation {
                description: "P1 decision resource snapshot does not match its post-P0 session"
                    .to_owned(),
                location: "pipeline::decision_resources::validate_source_session".to_owned(),
            });
        }
        Ok(())
    }

    pub fn raw_cash(&self, account: AccountId) -> Result<Money, StepFatal> {
        Ok(self.account(account)?.raw_cash)
    }

    pub fn available_cash(&self, account: AccountId) -> Result<Money, StepFatal> {
        self.allocation.available_cash(account)
    }

    pub fn reserved_cash(&self, account: AccountId) -> Result<Money, StepFatal> {
        self.raw_cash(account)?
            .sub(self.available_cash(account)?)
            .map_err(money)
    }

    pub fn equity(&self, account: AccountId) -> Result<Money, StepFatal> {
        Ok(self.account(account)?.equity)
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
        self.allocation.available_sell_qty(account, stock)
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

    #[cfg(test)]
    pub(super) fn allocation(&self) -> &AllocationSnapshot {
        &self.allocation
    }

    fn account(&self, account: AccountId) -> Result<&DecisionAccountResources, StepFatal> {
        self.accounts
            .get(&account)
            .ok_or_else(|| invariant("unknown decision resource account"))
    }

    fn position(
        &self,
        account: AccountId,
        stock: &StockCode,
    ) -> Result<&DecisionPositionResources, StepFatal> {
        self.account(account)?
            .positions
            .get(stock)
            .ok_or_else(|| invariant("unknown decision resource stock"))
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

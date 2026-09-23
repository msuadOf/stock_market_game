use super::*;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AllocationSnapshot {
    accounts: BTreeMap<crate::AccountId, AllocationResources>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct AllocationResources {
    cash: crate::Money,
    sellable: BTreeMap<crate::StockCode, u32>,
}

impl AllocationSnapshot {
    pub fn available_cash(&self, account: crate::AccountId) -> Result<crate::Money, StepFatal> {
        self.accounts
            .get(&account)
            .map(|resources| resources.cash)
            .ok_or_else(|| invariant("unknown allocation account"))
    }

    pub fn available_sell_qty(
        &self,
        account: crate::AccountId,
        stock: &crate::StockCode,
    ) -> Result<u32, StepFatal> {
        self.accounts
            .get(&account)
            .ok_or_else(|| invariant("unknown allocation account"))?
            .sellable
            .get(stock)
            .copied()
            .ok_or_else(|| invariant("unknown allocation stock"))
    }
}

pub(super) fn plan_allocation(
    input: &PhaseInput<'_>,
    _expiry: ExpiryOutput,
    shadow: &mut TickShadowPlan,
) -> Result<AllocationOutput, StepFatal> {
    input.session.require_healthy()?;
    if shadow.state.is_non_authoritative_test_strategy() {
        shadow.tokens.push(PhaseOutput {
            phase: TickPhase::SealAllocationSnapshot,
        });
        return Ok(AllocationOutput);
    }
    let allocation = shadow
        .state
        .execute(GameSession::seal_allocation_snapshot)?;
    let snapshot = shadow
        .state
        .execute(|session| DecisionResourceSnapshot::seal(session, allocation))?;
    shadow.decision_resources = Some(std::sync::Arc::new(snapshot));
    shadow.tokens.push(PhaseOutput {
        phase: TickPhase::SealAllocationSnapshot,
    });
    Ok(AllocationOutput)
}

impl GameSession {
    fn seal_allocation_snapshot(&mut self) -> Result<AllocationSnapshot, StepFatal> {
        let configured_stocks: BTreeMap<_, _> = self
            .setup
            .stocks
            .iter()
            .map(|stock| (stock.code.clone(), ()))
            .collect();
        let mut reserved_cash: BTreeMap<crate::AccountId, crate::Money> = BTreeMap::new();
        let mut reserved_sell: BTreeMap<(crate::AccountId, crate::StockCode), u32> =
            BTreeMap::new();
        for (key, envelope) in self.envelope_ledger.iter() {
            let live = envelope.live();
            let cash = reserved_cash
                .entry(key.account)
                .or_insert(crate::Money::ZERO);
            *cash = cash.add(live.cash).map_err(money)?;
            let shares = reserved_sell
                .entry((key.account, key.stock.clone()))
                .or_insert(0);
            *shares = shares
                .checked_add(live.shares)
                .ok_or_else(|| invariant("live sell reservation overflow"))?;
        }
        let mut accounts = BTreeMap::new();
        for (account_id, account) in &self.accounts {
            let reserved = reserved_cash
                .remove(account_id)
                .unwrap_or(crate::Money::ZERO);
            let cash = account.cash.sub(reserved).map_err(money)?;
            if cash.cents() < 0 {
                return Err(invariant("live buy reservation exceeds account cash"));
            }
            let mut sellable = BTreeMap::new();
            for stock in configured_stocks.keys() {
                let reserved_shares = reserved_sell
                    .remove(&(*account_id, stock.clone()))
                    .unwrap_or(0);
                let available = account
                    .sellable_qty(stock)
                    .checked_sub(reserved_shares)
                    .ok_or_else(|| invariant("live sell reservation exceeds sellable shares"))?;
                sellable.insert(stock.clone(), available);
            }
            accounts.insert(*account_id, AllocationResources { cash, sellable });
        }
        if !reserved_cash.is_empty() {
            return Err(invariant("envelope ledger references an unknown account"));
        }
        if !reserved_sell.is_empty() {
            return Err(invariant(
                "envelope ledger references an unknown account or stock",
            ));
        }
        Ok(AllocationSnapshot { accounts })
    }
}

fn money(error: crate::MoneyError) -> StepFatal {
    StepFatal::InvariantViolation {
        description: error.to_string(),
        location: "pipeline::p1_allocation".to_owned(),
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p1_allocation".to_owned(),
    }
}

use super::*;
use crate::diagnostics::causal::{
    CausalError, CausalFact, CausalFactKind, CausalReport, FactTime, OrderOrigin, Quote,
    Termination,
};

impl GameSession {
    pub fn causal_facts(&self) -> &[CausalFact] {
        &self.causal.facts
    }

    pub fn causal_diagnostics(&self) -> Result<CausalReport, CausalError> {
        CausalReport::from_facts(self.seed, &self.causal.facts)
    }

    fn causal_time(&self) -> FactTime {
        let day_tick = self.tick - u64::from(self.day) * self.setup.ticks_per_day;
        let continuous =
            self.setup.ticks_per_day - self.setup.auction_ticks - self.setup.closing_auction_ticks;
        let elapsed = day_tick
            .saturating_sub(self.setup.auction_ticks)
            .min(continuous);
        let market_minute = u64::from(self.day) * 240 + elapsed * 240 / continuous;
        FactTime {
            phase: self.phase(),
            market_minute,
            civil: self.observation_civil_instant(),
        }
    }

    pub(super) fn causal_record(&mut self, kind: CausalFactKind) {
        self.causal.record(self.causal_time(), kind);
    }

    pub(super) fn causal_quote(&self, code: &StockCode) -> Quote {
        let market = &self.markets[code];
        Quote {
            code: code.clone(),
            bid_cents: market.best_bid().map(|price| price.cents()),
            ask_cents: market.best_ask().map(|price| price.cents()),
            bid_depth: market.bid_depth().iter().map(|(_, qty)| qty).sum(),
            ask_depth: market.ask_depth().iter().map(|(_, qty)| qty).sum(),
        }
    }

    pub(super) fn causal_snapshot(&mut self, code: &StockCode) {
        self.causal_record(CausalFactKind::Quote(self.causal_quote(code)));
    }

    pub(super) fn causal_submitted(&mut self, order: &Order, code: &StockCode) {
        let quote = self.causal_quote(code);
        self.causal_submitted_with_quote(order.owner, order.id, code, order.side, order.qty, quote);
    }

    pub(super) fn causal_submitted_with_quote(
        &mut self,
        account: AccountId,
        order: OrderId,
        code: &StockCode,
        side: crate::Side,
        qty: u32,
        quote: Quote,
    ) {
        self.causal_record(CausalFactKind::Quote(quote));
        let plan = self
            .parent_orders
            .get(&account)
            .and_then(|plans| plans.get(code))
            .filter(|plan| plan.side == side)
            .and_then(|plan| plan.linked_plan_id);
        self.causal_record(CausalFactKind::Submitted(OrderOrigin {
            order,
            account,
            code: code.clone(),
            company: self.company_registry.issuer_of(code).cloned(),
            plan,
            decision: self.causal.decision.get(&account).copied(),
            side,
            qty,
        }));
    }

    pub(super) fn causal_terminated(
        &mut self,
        order: (AccountId, OrderId, u32),
        code: &StockCode,
        reason: Termination,
    ) {
        self.causal_record(CausalFactKind::Terminated {
            account: order.0,
            order: order.1,
            qty: order.2,
            code: code.clone(),
            reason,
        });
    }
}

use super::*;
use crate::diagnostics::causal::{
    CausalError, CausalFact, CausalFactKind, CausalReport, FactTime, OrderOrigin, Quote,
    Termination,
};

impl GameSession {
    pub fn causal_facts(&self) -> &[CausalFact] {
        self.state.causal.facts()
    }

    pub fn causal_diagnostics(&self) -> Result<CausalReport, CausalError> {
        CausalReport::from_facts(self.state.seed, self.state.causal.facts())
    }

    pub(super) fn causal_time(&self) -> FactTime {
        let day_tick = self.state.tick - u64::from(self.state.day) * self.state.setup.ticks_per_day;
        let continuous = self.state.setup.ticks_per_day
            - self.state.setup.auction_ticks
            - self.state.setup.closing_auction_ticks;
        let elapsed = day_tick
            .saturating_sub(self.state.setup.auction_ticks)
            .min(continuous);
        let market_minute = u64::from(self.state.day) * 240 + elapsed * 240 / continuous;
        FactTime {
            phase: self.phase(),
            market_minute,
            civil: self.observation_civil_instant(),
        }
    }

    pub(super) fn causal_record(&mut self, kind: CausalFactKind) {
        self.causal_record_at(self.causal_time(), kind);
    }

    pub(super) fn causal_record_at(&mut self, time: FactTime, kind: CausalFactKind) {
        self.state.causal.record(time, kind);
    }

    pub(super) fn causal_quote(&self, code: &StockCode) -> Quote {
        let market = &self.state.markets[code];
        Quote {
            code: code.clone(),
            bid_cents: market.best_bid().map(|price| price.cents()),
            ask_cents: market.best_ask().map(|price| price.cents()),
            bid_depth: market.bid_depth().iter().map(|(_, qty)| qty).sum(),
            ask_depth: market.ask_depth().iter().map(|(_, qty)| qty).sum(),
        }
    }

    pub(super) fn causal_snapshot(&mut self, code: &StockCode) {
        self.causal_snapshot_at(self.causal_time(), code);
    }

    pub(super) fn causal_snapshot_at(&mut self, time: FactTime, code: &StockCode) {
        self.causal_record_at(time, CausalFactKind::Quote(self.causal_quote(code)));
    }

    #[cfg(test)]
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
            .state
            .parent_orders
            .get(&account)
            .and_then(|plans| plans.get(code))
            .filter(|plan| plan.side() == side)
            .and_then(|plan| plan.linked_plan_id());
        self.causal_record(CausalFactKind::Submitted(OrderOrigin {
            order,
            account,
            code: code.clone(),
            company: self.state.company_system.issuers().issuer_of(code).cloned(),
            plan,
            decision: self.state.causal.decision_for(account),
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
        self.causal_terminated_at(self.causal_time(), order, code, reason);
    }

    pub(super) fn causal_terminated_at(
        &mut self,
        time: FactTime,
        order: (AccountId, OrderId, u32),
        code: &StockCode,
        reason: Termination,
    ) {
        self.causal_record_at(
            time,
            CausalFactKind::Terminated {
                account: order.0,
                order: order.1,
                qty: order.2,
                code: code.clone(),
                reason,
            },
        );
    }
}

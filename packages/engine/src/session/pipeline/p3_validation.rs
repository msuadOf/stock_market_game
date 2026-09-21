use super::{
    DecisionResourceSnapshot, Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, FeeComponents,
    P2CandidateBatch, P2CandidateKey, ResVec, StepFatal,
};
use crate::{
    AccountId, GameConfig, Intent, Money, OrderId, RejectionReason, SecurityCategory, Side,
    StockCode, MAX_OPEN_ORDERS, MAX_OPEN_ORDERS_PER_ACCOUNT,
};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P3OpenOrderLimits {
    pub global: usize,
    pub per_account: usize,
}

impl P3OpenOrderLimits {
    pub const PRODUCTION: Self = Self {
        global: MAX_OPEN_ORDERS,
        per_account: MAX_OPEN_ORDERS_PER_ACCOUNT,
    };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct P3StockValidation {
    category: SecurityCategory,
    market_buy_protective_price: Money,
    market_sell_protective_price: Money,
}

impl P3StockValidation {
    pub const fn new(
        category: SecurityCategory,
        market_buy_protective_price: Money,
        market_sell_protective_price: Money,
    ) -> Self {
        Self {
            category,
            market_buy_protective_price,
            market_sell_protective_price,
        }
    }

    const fn protective_price(self, side: Side) -> Money {
        match side {
            Side::Buy => self.market_buy_protective_price,
            Side::Sell => self.market_sell_protective_price,
        }
    }
}

/// Immutable tick-start facts needed by P3. It deliberately contains no session reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct P3ValidationContext {
    stocks: BTreeMap<StockCode, P3StockValidation>,
    global_open_orders: usize,
    account_open_orders: BTreeMap<AccountId, usize>,
    limits: P3OpenOrderLimits,
}

impl P3ValidationContext {
    pub fn new(
        stocks: impl IntoIterator<Item = (StockCode, P3StockValidation)>,
        global_open_orders: usize,
        account_open_orders: impl IntoIterator<Item = (AccountId, usize)>,
        limits: P3OpenOrderLimits,
    ) -> Result<Self, StepFatal> {
        let mut stock_map = BTreeMap::new();
        for (code, validation) in stocks {
            if stock_map.insert(code, validation).is_some() {
                return Err(invariant("duplicate stock in P3 validation context"));
            }
        }
        let mut account_map = BTreeMap::new();
        for (account, count) in account_open_orders {
            if account_map.insert(account, count).is_some() {
                return Err(invariant("duplicate account in P3 validation context"));
            }
        }
        Ok(Self {
            stocks: stock_map,
            global_open_orders,
            account_open_orders: account_map,
            limits,
        })
    }

    fn stock(&self, code: &StockCode) -> Option<P3StockValidation> {
        self.stocks.get(code).copied()
    }
}

#[derive(Clone, Debug)]
pub struct P2P3Handoff {
    candidates: P2CandidateBatch,
    resources: DecisionResourceSnapshot,
    ledger: EnvelopeLedger,
    next_order_id: u64,
    config: GameConfig,
    context: P3ValidationContext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum P3PlaceKind {
    Limit,
    Market,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvelopeDraft {
    candidate_key: P2CandidateKey,
    sealed_index: u64,
    envelope_key: EnvelopeKey,
    kind: P3PlaceKind,
    limit: Money,
    qty: u32,
    required: ResVec,
}

#[derive(Clone, Debug, PartialEq)]
pub enum P3CandidateResult {
    Accepted {
        key: P2CandidateKey,
        sealed_index: u64,
    },
    Rejected {
        key: P2CandidateKey,
        sealed_index: u64,
        reason: RejectionReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum P3ValidatedOperation {
    Place(EnvelopeDraft),
    Cancel {
        candidate_key: P2CandidateKey,
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct P3ValidationOutput {
    results: Vec<P3CandidateResult>,
    operations: Vec<P3ValidatedOperation>,
    drafts: Vec<EnvelopeDraft>,
    identities: Vec<P3CandidateIdentity>,
    next_order_id_after: u64,
    next_sealed_index_after: u64,
}

#[derive(Clone, Debug)]
pub(super) struct P3ValidationState {
    resources: Arc<DecisionResourceSnapshot>,
    config: GameConfig,
    context: Arc<P3ValidationContext>,
    budgets: BTreeMap<AccountId, AccountBudget>,
    open_orders: OpenOrderBudget,
    next_sealed_index: u64,
    processed_count: u64,
    open_order_feedback: BTreeSet<(P2CandidateKey, u64)>,
    output: P3ValidationOutput,
    #[cfg(test)]
    pub(super) last_round_account_shards: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct P3CandidateIdentity {
    key: P2CandidateKey,
    sealed_index: u64,
    allocated_order_id: Option<OrderId>,
}

#[derive(Clone, Debug)]
pub(super) struct P3ValidatedStep {
    pub(super) result: P3CandidateResult,
    pub(super) operation: Option<P3ValidatedOperation>,
    identity: P3CandidateIdentity,
}

#[derive(Clone, Debug)]
enum P3PreparedStep {
    Place {
        place: UnkeyedEnvelopeDraft,
        required: ResVec,
    },
    Cancel {
        key: P2CandidateKey,
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
    },
    Rejected {
        key: P2CandidateKey,
        sealed_index: u64,
        reason: RejectionReason,
    },
}

impl P2P3Handoff {
    pub fn new_with_context(
        candidates: P2CandidateBatch,
        resources: DecisionResourceSnapshot,
        ledger: EnvelopeLedger,
        next_order_id: u64,
        config: GameConfig,
        context: P3ValidationContext,
    ) -> Result<Self, StepFatal> {
        ledger.validate_conservation()?;
        Ok(Self {
            candidates,
            resources,
            ledger,
            next_order_id,
            config,
            context,
        })
    }

    /// Transitional unit-test seam for older P2 composition tests. Production wiring must pass
    /// an explicit immutable `P3ValidationContext` through `new_with_context`.
    #[cfg(test)]
    pub fn new(
        candidates: P2CandidateBatch,
        resources: DecisionResourceSnapshot,
        ledger: EnvelopeLedger,
        next_order_id: u64,
        config: GameConfig,
    ) -> Result<Self, StepFatal> {
        let mut stocks = BTreeMap::new();
        let mut accounts = BTreeMap::new();
        for candidate in candidates.candidates() {
            accounts.entry(candidate.owner()).or_insert(0);
            let (code, price) = match candidate.intent() {
                Intent::PlaceLimit { code, price, .. } => (code, *price),
                Intent::PlaceMarket { code, .. } | Intent::Cancel { code, .. } => {
                    (code, Money::ZERO)
                }
            };
            stocks.entry(code.clone()).or_insert(P3StockValidation::new(
                SecurityCategory::MainBoard,
                price,
                price,
            ));
        }
        let context = P3ValidationContext::new(stocks, 0, accounts, P3OpenOrderLimits::PRODUCTION)?;
        Self::new_with_context(
            candidates,
            resources,
            ledger,
            next_order_id,
            config,
            context,
        )
    }

    pub fn validate(&self) -> Result<P3ValidationOutput, StepFatal> {
        let mut state = P3ValidationState::new(
            self.resources.clone(),
            self.next_order_id,
            0,
            self.config.clone(),
            self.context.clone(),
        )?;
        state.consume_round(self.candidates.candidates())?;
        Ok(state.into_output())
    }

    pub fn ledger(&self) -> &EnvelopeLedger {
        &self.ledger
    }
}

impl P3ValidationState {
    pub(super) fn new(
        resources: DecisionResourceSnapshot,
        next_order_id: u64,
        next_sealed_index: u64,
        config: GameConfig,
        context: P3ValidationContext,
    ) -> Result<Self, StepFatal> {
        let mut budgets = BTreeMap::new();
        for account in context.account_open_orders.keys().copied() {
            budgets.insert(
                account,
                AccountBudget {
                    cash: resources.available_cash(account)?,
                    sellable: BTreeMap::new(),
                },
            );
        }
        Ok(Self {
            resources: Arc::new(resources),
            config,
            open_orders: OpenOrderBudget::new(&context),
            context: Arc::new(context),
            budgets,
            next_sealed_index,
            processed_count: 0,
            open_order_feedback: BTreeSet::new(),
            output: P3ValidationOutput {
                results: Vec::new(),
                operations: Vec::new(),
                drafts: Vec::new(),
                identities: Vec::new(),
                next_order_id_after: next_order_id,
                next_sealed_index_after: next_sealed_index,
            },
            #[cfg(test)]
            last_round_account_shards: 0,
        })
    }

    /// Performs P3's two passes for one canonical ready round. The first pass validates
    /// every candidate against the tick-persistent account budgets and provisional open-order
    /// counts. The second pass allocates OrderIds only to accepted Place operations.
    /// Callers transact this state by cloning it before invoking the method.
    pub(super) fn consume_round(
        &mut self,
        candidates: &[super::P2Candidate],
    ) -> Result<Vec<P3ValidatedStep>, StepFatal> {
        let prepared = self.prepare_account_round(candidates)?;

        let place_count = prepared
            .iter()
            .filter(|step| matches!(step, P3PreparedStep::Place { .. }))
            .count();
        let place_count = u64::try_from(place_count)
            .map_err(|_| invariant("P3 accepted Place count does not fit in u64"))?;
        let next_order_id_after = self
            .output
            .next_order_id_after
            .checked_add(place_count)
            .ok_or_else(|| invariant("P3 order ID allocation overflow"))?;

        let mut next_order_id = self.output.next_order_id_after;
        let mut steps = Vec::with_capacity(prepared.len());
        for prepared in prepared {
            let step = match prepared {
                P3PreparedStep::Place { place, required } => {
                    let order_id = OrderId(next_order_id);
                    next_order_id = next_order_id
                        .checked_add(1)
                        .ok_or_else(|| invariant("P3 order ID allocation overflow"))?;
                    accepted_place(place, required, order_id)
                }
                P3PreparedStep::Cancel {
                    key,
                    sealed_index,
                    account,
                    code,
                    order_id,
                } => accepted_cancel(key, sealed_index, account, code, order_id),
                P3PreparedStep::Rejected {
                    key,
                    sealed_index,
                    reason,
                } => rejected(key, sealed_index, reason),
            };
            self.commit_step(step.clone());
            steps.push(step);
        }
        if next_order_id != next_order_id_after {
            return Err(invariant(
                "P3 two-pass OrderId allocation disagrees with its prefix sum",
            ));
        }
        self.output.next_order_id_after = next_order_id_after;
        self.output.next_sealed_index_after = self.next_sealed_index;
        Ok(steps)
    }

    /// Limit-order slots are the only cross-account P3 constraint. A round whose maximum
    /// possible slot demand fits can validate independent accounts in parallel. A caller that
    /// submits a globally competing round still gets canonical arbitration, before ID allocation.
    fn prepare_account_round(
        &mut self,
        candidates: &[super::P2Candidate],
    ) -> Result<Vec<P3PreparedStep>, StepFatal> {
        let limit_count = candidates
            .iter()
            .filter(|candidate| matches!(candidate.intent(), Intent::PlaceLimit { .. }))
            .count();
        if limit_count
            > self
                .open_orders
                .limits
                .global
                .saturating_sub(self.open_orders.global)
        {
            #[cfg(test)]
            {
                self.last_round_account_shards = 0;
            }
            return candidates
                .iter()
                .map(|candidate| self.prepare(candidate))
                .collect();
        }

        let count = u64::try_from(candidates.len())
            .map_err(|_| invariant("P3 candidate count does not fit in u64"))?;
        let next_sealed_index = self
            .next_sealed_index
            .checked_add(count)
            .ok_or_else(|| invariant("P3 sealed candidate index overflow"))?;
        let processed_count = self
            .processed_count
            .checked_add(count)
            .ok_or_else(|| invariant("P3 processed candidate count overflow"))?;
        let mut grouped = BTreeMap::<AccountId, Vec<(usize, &super::P2Candidate)>>::new();
        for (index, candidate) in candidates.iter().enumerate() {
            grouped
                .entry(candidate.owner())
                .or_default()
                .push((index, candidate));
        }
        let results = grouped
            .into_iter()
            .collect::<Vec<_>>()
            .into_par_iter()
            .map(|(account, candidates)| {
                // The immutable decision/context snapshots are shared. Each worker owns only
                // its account's cash, stock budgets and order count; it never clones tick output.
                let mut worker = Self {
                    resources: Arc::clone(&self.resources),
                    config: self.config.clone(),
                    context: Arc::clone(&self.context),
                    budgets: self
                        .budgets
                        .get(&account)
                        .cloned()
                        .map(|budget| (account, budget))
                        .into_iter()
                        .collect(),
                    open_orders: OpenOrderBudget {
                        global: self.open_orders.global,
                        by_account: self
                            .open_orders
                            .by_account
                            .get(&account)
                            .copied()
                            .map(|count| (account, count))
                            .into_iter()
                            .collect(),
                        limits: self.open_orders.limits,
                    },
                    next_sealed_index: self.next_sealed_index,
                    processed_count: 0,
                    open_order_feedback: BTreeSet::new(),
                    output: P3ValidationOutput {
                        results: Vec::new(),
                        operations: Vec::new(),
                        drafts: Vec::new(),
                        identities: Vec::new(),
                        next_order_id_after: self.output.next_order_id_after,
                        next_sealed_index_after: self.next_sealed_index,
                    },
                    #[cfg(test)]
                    last_round_account_shards: 0,
                };
                let mut prepared = Vec::with_capacity(candidates.len());
                for (index, candidate) in candidates {
                    // The whole range was checked above; account traversal never allocates IDs.
                    worker.next_sealed_index = self.next_sealed_index + index as u64;
                    let step = worker.prepare(candidate);
                    let failed = step.is_err();
                    prepared.push((index, step));
                    if failed {
                        break;
                    }
                }
                (worker, prepared)
            })
            .collect::<Vec<_>>();
        #[cfg(test)]
        {
            self.last_round_account_shards = results.len();
        }
        let mut prepared = Vec::with_capacity(candidates.len());
        for (worker, steps) in results {
            self.budgets.extend(worker.budgets);
            self.open_orders
                .by_account
                .extend(worker.open_orders.by_account);
            prepared.extend(steps);
        }
        prepared.sort_by_key(|(index, _)| *index);
        let prepared = prepared
            .into_iter()
            .map(|(_, step)| step)
            .collect::<Result<Vec<_>, _>>()?;
        let accepted_limits = prepared.iter().filter(|step| {
            matches!(step, P3PreparedStep::Place { place, .. } if place.kind == P3PlaceKind::Limit)
        }).count();
        self.open_orders.global = self
            .open_orders
            .global
            .checked_add(accepted_limits)
            .ok_or_else(|| invariant("global open-order count overflow"))?;
        self.next_sealed_index = next_sealed_index;
        self.processed_count = processed_count;
        Ok(prepared)
    }

    /// Conservative ready prefix: no candidate in it can need a P4 slot release to pass P3.
    /// A full first slot still permits one candidate so rejection/cancellation can make progress.
    pub(super) fn ready_round_len(
        &self,
        candidates: &[super::P2Candidate],
    ) -> Result<usize, StepFatal> {
        let mut slots = self.open_orders.clone();
        for (index, candidate) in candidates.iter().enumerate() {
            if matches!(candidate.intent(), Intent::PlaceLimit { .. }) {
                if !slots.can_accept_limit(candidate.owner())? {
                    return Ok(index.max(1));
                }
                slots.accept_limit(candidate.owner())?;
            }
        }
        Ok(candidates.len())
    }

    fn prepare(&mut self, candidate: &super::P2Candidate) -> Result<P3PreparedStep, StepFatal> {
        let sealed_index = self.next_sealed_index;
        let next_sealed_index = sealed_index
            .checked_add(1)
            .ok_or_else(|| invariant("P3 sealed candidate index overflow"))?;
        let next_processed_count = self
            .processed_count
            .checked_add(1)
            .ok_or_else(|| invariant("P3 processed candidate count overflow"))?;
        let key = candidate.key().clone();
        let prepared = match candidate.intent() {
            Intent::Cancel { code, id } => P3PreparedStep::Cancel {
                key,
                sealed_index,
                account: candidate.owner(),
                code: code.clone(),
                order_id: *id,
            },
            Intent::PlaceLimit {
                code,
                side,
                price,
                qty,
            } => self.prepare_place(
                candidate,
                key,
                sealed_index,
                code,
                *side,
                P3PlaceKind::Limit,
                *price,
                *qty,
            )?,
            Intent::PlaceMarket { code, side, qty } => {
                let Some(stock) = self.context.stock(code) else {
                    self.next_sealed_index = next_sealed_index;
                    self.processed_count = next_processed_count;
                    return Ok(P3PreparedStep::Rejected {
                        key,
                        sealed_index,
                        reason: RejectionReason::UnknownStock,
                    });
                };
                self.prepare_place(
                    candidate,
                    key,
                    sealed_index,
                    code,
                    *side,
                    P3PlaceKind::Market,
                    stock.protective_price(*side),
                    *qty,
                )?
            }
        };
        self.next_sealed_index = next_sealed_index;
        self.processed_count = next_processed_count;
        Ok(prepared)
    }

    fn prepare_place(
        &mut self,
        candidate: &super::P2Candidate,
        key: P2CandidateKey,
        sealed_index: u64,
        code: &StockCode,
        side: Side,
        kind: P3PlaceKind,
        limit: Money,
        qty: u32,
    ) -> Result<P3PreparedStep, StepFatal> {
        let Some(stock) = self.context.stock(code) else {
            return Ok(P3PreparedStep::Rejected {
                key,
                sealed_index,
                reason: RejectionReason::UnknownStock,
            });
        };
        let place = UnkeyedEnvelopeDraft {
            candidate_key: key.clone(),
            sealed_index,
            account: candidate.owner(),
            code: code.clone(),
            side,
            kind,
            limit,
            qty,
        };
        self.ensure_account_budget(candidate.owner())?;
        if side == Side::Sell {
            self.ensure_sellable_budget(candidate.owner(), code)?;
        }
        let prepared = match self.validate_place(stock, &place)? {
            Ok(prepared) => prepared,
            Err(reason) => {
                return Ok(P3PreparedStep::Rejected {
                    key,
                    sealed_index,
                    reason,
                });
            }
        };
        if kind == P3PlaceKind::Limit {
            self.open_orders.accept_limit(candidate.owner())?;
        }
        self.apply_budget_update(place.account, prepared.budget_update);
        Ok(P3PreparedStep::Place {
            place,
            required: prepared.required,
        })
    }

    fn ensure_account_budget(&mut self, account: AccountId) -> Result<(), StepFatal> {
        if !self.budgets.contains_key(&account) {
            self.budgets.insert(
                account,
                AccountBudget {
                    cash: self.resources.available_cash(account)?,
                    sellable: BTreeMap::new(),
                },
            );
        }
        Ok(())
    }

    fn ensure_sellable_budget(
        &mut self,
        account: AccountId,
        code: &StockCode,
    ) -> Result<(), StepFatal> {
        let budget = self
            .budgets
            .get_mut(&account)
            .ok_or_else(|| invariant("P3 validation context is missing an account budget"))?;
        if !budget.sellable.contains_key(code) {
            let available = self.resources.available_sell_qty(account, code)?;
            budget.sellable.insert(code.clone(), available);
        }
        Ok(())
    }

    fn validate_place(
        &self,
        stock: P3StockValidation,
        place: &UnkeyedEnvelopeDraft,
    ) -> Result<Result<PreparedReservation, RejectionReason>, StepFatal> {
        if place.qty == 0 || place.qty > stock.category.max_order_qty(place.is_market()) {
            return Ok(Err(RejectionReason::InvalidQuantity));
        }
        let existing_budget = self
            .budgets
            .get(&place.account)
            .ok_or_else(|| invariant("P3 validation context is missing an account budget"))?;
        let current_cash = existing_budget.cash;
        if place.side == Side::Buy && !place.qty.is_multiple_of(self.config.lot_size) {
            return Ok(Err(RejectionReason::InvalidQuantity));
        }
        let mut available_sell = None;
        if place.side == Side::Sell {
            let available = existing_budget
                .sellable
                .get(&place.code)
                .copied()
                .ok_or_else(|| invariant("P3 validation context is missing a stock budget"))?;
            if place.qty > available {
                return Ok(Err(RejectionReason::InsufficientShares));
            }
            if !place.qty.is_multiple_of(self.config.lot_size)
                && place.qty % self.config.lot_size != available % self.config.lot_size
            {
                return Ok(Err(RejectionReason::InvalidQuantity));
            }
            available_sell = Some(available);
        }
        if place.kind == P3PlaceKind::Limit && !self.open_orders.can_accept_limit(place.account)? {
            return Ok(Err(RejectionReason::ResourceLimitExceeded));
        }
        match place.side {
            Side::Buy => {
                let required = crate::session::buy_order_reservation(
                    &self.config,
                    place.limit,
                    place.qty,
                    Money::ZERO,
                )
                .map_err(money)?;
                if required > current_cash {
                    return Ok(Err(RejectionReason::InsufficientCash));
                }
                Ok(Ok(PreparedReservation {
                    required: ResVec::new(required, 0),
                    budget_update: BudgetUpdate::Cash {
                        cash_after: current_cash.sub(required).map_err(money)?,
                    },
                }))
            }
            Side::Sell => Ok(Ok(PreparedReservation {
                required: ResVec::new(Money::ZERO, place.qty),
                budget_update: BudgetUpdate::Shares {
                    code: place.code.clone(),
                    shares_after: available_sell
                        .ok_or_else(|| invariant("sell validation omitted available shares"))?
                        .checked_sub(place.qty)
                        .ok_or_else(|| invariant("sell reservation underflow"))?,
                },
            })),
        }
    }

    fn apply_budget_update(&mut self, account: AccountId, update: BudgetUpdate) {
        let budget = self
            .budgets
            .get_mut(&account)
            .expect("P3 context budgets were exhaustively initialized");
        match update {
            BudgetUpdate::Cash { cash_after } => {
                budget.cash = cash_after;
            }
            BudgetUpdate::Shares { code, shares_after } => {
                budget.sellable.insert(code, shares_after);
            }
        }
    }

    fn commit_step(&mut self, step: P3ValidatedStep) {
        self.output.identities.push(step.identity);
        self.output.results.push(step.result);
        if let Some(operation) = step.operation {
            if let P3ValidatedOperation::Place(draft) = &operation {
                self.output.drafts.push(draft.clone());
            }
            self.output.operations.push(operation);
        }
    }

    pub(super) fn apply_open_order_feedback(
        &mut self,
        candidate_key: &P2CandidateKey,
        sealed_index: u64,
        actual_deltas: impl IntoIterator<Item = (AccountId, i64)>,
    ) -> Result<(), StepFatal> {
        let feedback_key = (candidate_key.clone(), sealed_index);
        if self.open_order_feedback.contains(&feedback_key) {
            return Err(invariant(
                "P3 open-order feedback was consumed more than once",
            ));
        }
        let operation_index = self
            .output
            .operations
            .binary_search_by_key(&sealed_index, P3ValidatedOperation::sealed_index)
            .map_err(|_| invariant("P3 open-order feedback has no accepted operation"))?;
        let operation = &self.output.operations[operation_index];
        if operation.candidate_key() != candidate_key {
            return Err(invariant(
                "P3 open-order feedback has no accepted operation",
            ));
        }

        let mut actual_by_account = BTreeMap::<AccountId, i64>::new();
        for (account, delta) in actual_deltas {
            if actual_by_account.insert(account, delta).is_some() {
                return Err(invariant(
                    "P3 open-order feedback contains a duplicate account delta",
                ));
            }
        }
        validate_open_order_feedback(operation, &actual_by_account)?;

        let mut correction = actual_by_account;
        if let P3ValidatedOperation::Place(draft) = operation {
            if draft.kind() == P3PlaceKind::Limit {
                let owner_delta = correction.entry(draft.owner()).or_default();
                *owner_delta = owner_delta
                    .checked_sub(1)
                    .ok_or_else(|| invariant("P3 open-order feedback delta underflow"))?;
            }
        }
        correction.retain(|_, delta| *delta != 0);
        self.open_orders.apply_correction(&correction)?;
        self.open_order_feedback.insert(feedback_key);
        Ok(())
    }

    pub(super) const fn output(&self) -> &P3ValidationOutput {
        &self.output
    }

    pub(super) const fn sealed_count(&self) -> u64 {
        self.processed_count
    }

    pub(super) const fn next_sealed_index(&self) -> u64 {
        self.next_sealed_index
    }

    pub(super) fn remaining_cash(&self) -> BTreeMap<AccountId, Money> {
        self.budgets
            .iter()
            .map(|(account, budget)| (*account, budget.cash))
            .collect()
    }

    pub(super) fn remaining_sellable(&self) -> BTreeMap<(AccountId, StockCode), u32> {
        self.budgets
            .iter()
            .flat_map(|(account, budget)| {
                budget
                    .sellable
                    .iter()
                    .map(|(code, qty)| ((*account, code.clone()), *qty))
            })
            .collect()
    }

    pub(super) const fn global_open_orders(&self) -> usize {
        self.open_orders.global
    }

    pub(super) fn account_open_orders(&self) -> BTreeMap<AccountId, usize> {
        self.open_orders.by_account.clone()
    }

    pub(super) fn feedback_count(&self) -> usize {
        self.open_order_feedback.len()
    }

    pub(super) fn into_output(self) -> P3ValidationOutput {
        self.output
    }
}

fn rejected(key: P2CandidateKey, sealed_index: u64, reason: RejectionReason) -> P3ValidatedStep {
    let identity = P3CandidateIdentity {
        key: key.clone(),
        sealed_index,
        allocated_order_id: None,
    };
    P3ValidatedStep {
        result: P3CandidateResult::Rejected {
            key,
            sealed_index,
            reason,
        },
        operation: None,
        identity,
    }
}

fn accepted_place(
    place: UnkeyedEnvelopeDraft,
    required: ResVec,
    order_id: OrderId,
) -> P3ValidatedStep {
    let key = place.candidate_key.clone();
    let sealed_index = place.sealed_index;
    let identity = P3CandidateIdentity {
        key: key.clone(),
        sealed_index,
        allocated_order_id: Some(order_id),
    };
    P3ValidatedStep {
        result: P3CandidateResult::Accepted { key, sealed_index },
        operation: Some(P3ValidatedOperation::Place(
            place.with_order_id(order_id, required),
        )),
        identity,
    }
}

fn accepted_cancel(
    key: P2CandidateKey,
    sealed_index: u64,
    account: AccountId,
    code: StockCode,
    order_id: OrderId,
) -> P3ValidatedStep {
    let identity = P3CandidateIdentity {
        key: key.clone(),
        sealed_index,
        allocated_order_id: None,
    };
    P3ValidatedStep {
        result: P3CandidateResult::Accepted {
            key: key.clone(),
            sealed_index,
        },
        operation: Some(P3ValidatedOperation::Cancel {
            candidate_key: key,
            sealed_index,
            account,
            code,
            order_id,
        }),
        identity,
    }
}

impl P3ValidatedStep {
    pub(super) const fn candidate_key(&self) -> &P2CandidateKey {
        &self.identity.key
    }

    pub(super) const fn sealed_index(&self) -> u64 {
        self.identity.sealed_index
    }

    pub(super) const fn allocated_order_id(&self) -> Option<OrderId> {
        self.identity.allocated_order_id
    }
}

impl EnvelopeDraft {
    pub const fn candidate_key(&self) -> &P2CandidateKey {
        &self.candidate_key
    }

    pub const fn sealed_index(&self) -> u64 {
        self.sealed_index
    }

    pub const fn owner(&self) -> AccountId {
        self.envelope_key.account
    }

    pub const fn code(&self) -> &StockCode {
        &self.envelope_key.stock
    }

    pub const fn side(&self) -> Side {
        self.envelope_key.side
    }

    pub const fn kind(&self) -> P3PlaceKind {
        self.kind
    }

    pub const fn limit(&self) -> Money {
        self.limit
    }

    pub const fn qty(&self) -> u32 {
        self.qty
    }

    pub const fn order_id(&self) -> OrderId {
        self.envelope_key.order
    }

    pub const fn order(&self) -> OrderId {
        self.envelope_key.order
    }

    pub const fn key(&self) -> &EnvelopeKey {
        &self.envelope_key
    }

    pub const fn required(&self) -> ResVec {
        self.required
    }

    pub fn materialize_envelope(&self) -> Envelope {
        Envelope::p3_created(
            self.envelope_key.clone(),
            self.required.cash,
            self.required.shares,
            EnvelopeAudit {
                limit: self.limit,
                remaining_qty: self.qty,
                filled_qty: 0,
                filled_value: Money::ZERO,
                nominal: FeeComponents::ZERO,
                charged: FeeComponents::ZERO,
            },
        )
    }
}

impl P3ValidatedOperation {
    pub const fn candidate_key(&self) -> &P2CandidateKey {
        match self {
            Self::Place(draft) => draft.candidate_key(),
            Self::Cancel { candidate_key, .. } => candidate_key,
        }
    }

    pub const fn sealed_index(&self) -> u64 {
        match self {
            Self::Place(draft) => draft.sealed_index(),
            Self::Cancel { sealed_index, .. } => *sealed_index,
        }
    }
}

impl P3CandidateResult {
    pub const fn key(&self) -> &P2CandidateKey {
        match self {
            Self::Accepted { key, .. } | Self::Rejected { key, .. } => key,
        }
    }

    pub const fn sealed_index(&self) -> u64 {
        match self {
            Self::Accepted { sealed_index, .. } | Self::Rejected { sealed_index, .. } => {
                *sealed_index
            }
        }
    }
}

impl P3ValidationOutput {
    pub fn accepted(&self) -> impl Iterator<Item = &P2CandidateKey> {
        self.results.iter().filter_map(|result| match result {
            P3CandidateResult::Accepted { key, .. } => Some(key),
            P3CandidateResult::Rejected { .. } => None,
        })
    }

    pub fn rejected(&self) -> impl Iterator<Item = (&P2CandidateKey, &RejectionReason)> {
        self.results.iter().filter_map(|result| match result {
            P3CandidateResult::Accepted { .. } => None,
            P3CandidateResult::Rejected { key, reason, .. } => Some((key, reason)),
        })
    }

    pub fn results(&self) -> &[P3CandidateResult] {
        &self.results
    }

    pub fn operations(&self) -> &[P3ValidatedOperation] {
        &self.operations
    }

    pub fn drafts(&self) -> &[EnvelopeDraft] {
        &self.drafts
    }

    pub const fn next_order_id_after(&self) -> u64 {
        self.next_order_id_after
    }

    pub const fn next_sealed_index_after(&self) -> u64 {
        self.next_sealed_index_after
    }

    pub fn identities(
        &self,
    ) -> impl ExactSizeIterator<Item = (&P2CandidateKey, u64, Option<OrderId>)> {
        self.identities.iter().map(|identity| {
            (
                &identity.key,
                identity.sealed_index,
                identity.allocated_order_id,
            )
        })
    }
}

#[derive(Clone, Debug)]
struct UnkeyedEnvelopeDraft {
    candidate_key: P2CandidateKey,
    sealed_index: u64,
    account: AccountId,
    code: StockCode,
    side: Side,
    kind: P3PlaceKind,
    limit: Money,
    qty: u32,
}

impl UnkeyedEnvelopeDraft {
    const fn is_market(&self) -> bool {
        matches!(self.kind, P3PlaceKind::Market)
    }

    fn with_order_id(self, order_id: OrderId, required: ResVec) -> EnvelopeDraft {
        EnvelopeDraft {
            candidate_key: self.candidate_key,
            sealed_index: self.sealed_index,
            envelope_key: EnvelopeKey {
                account: self.account,
                stock: self.code,
                order: order_id,
                side: self.side,
            },
            kind: self.kind,
            limit: self.limit,
            qty: self.qty,
            required,
        }
    }
}

#[derive(Clone, Debug)]
struct AccountBudget {
    cash: Money,
    sellable: BTreeMap<StockCode, u32>,
}

struct PreparedReservation {
    required: ResVec,
    budget_update: BudgetUpdate,
}

enum BudgetUpdate {
    Cash { cash_after: Money },
    Shares { code: StockCode, shares_after: u32 },
}

#[derive(Clone, Debug)]
struct OpenOrderBudget {
    global: usize,
    by_account: BTreeMap<AccountId, usize>,
    limits: P3OpenOrderLimits,
}

impl OpenOrderBudget {
    fn new(context: &P3ValidationContext) -> Self {
        Self {
            global: context.global_open_orders,
            by_account: context.account_open_orders.clone(),
            limits: context.limits,
        }
    }

    fn can_accept_limit(&self, account: AccountId) -> Result<bool, StepFatal> {
        let account_count =
            self.by_account.get(&account).copied().ok_or_else(|| {
                invariant("P3 validation context is missing an account open count")
            })?;
        Ok(self.global < self.limits.global && account_count < self.limits.per_account)
    }

    fn accept_limit(&mut self, account: AccountId) -> Result<(), StepFatal> {
        let next_global = self
            .global
            .checked_add(1)
            .ok_or_else(|| invariant("global open-order count overflow"))?;
        let account_count =
            self.by_account.get(&account).copied().ok_or_else(|| {
                invariant("P3 validation context is missing an account open count")
            })?;
        let next_account = account_count
            .checked_add(1)
            .ok_or_else(|| invariant("account open-order count overflow"))?;
        self.global = next_global;
        self.by_account.insert(account, next_account);
        Ok(())
    }

    fn apply_correction(&mut self, correction: &BTreeMap<AccountId, i64>) -> Result<(), StepFatal> {
        let mut next_by_account = self.by_account.clone();
        let mut global_delta = 0_i64;
        for (account, delta) in correction {
            let current = next_by_account
                .get(account)
                .copied()
                .ok_or_else(|| invariant("P3 open-order feedback refers to an unknown account"))?;
            let next = apply_count_delta(current, *delta, "account open-order feedback")?;
            next_by_account.insert(*account, next);
            global_delta = global_delta
                .checked_add(*delta)
                .ok_or_else(|| invariant("global open-order feedback delta overflow"))?;
        }
        let next_global =
            apply_count_delta(self.global, global_delta, "global open-order feedback")?;
        if next_global > self.limits.global
            || next_by_account
                .values()
                .any(|count| *count > self.limits.per_account)
        {
            return Err(invariant(
                "P3 open-order feedback exceeds a configured order limit",
            ));
        }
        self.global = next_global;
        self.by_account = next_by_account;
        Ok(())
    }
}

fn validate_open_order_feedback(
    operation: &P3ValidatedOperation,
    actual_by_account: &BTreeMap<AccountId, i64>,
) -> Result<(), StepFatal> {
    let total = actual_by_account.values().try_fold(0_i64, |total, delta| {
        total
            .checked_add(*delta)
            .ok_or_else(|| invariant("P3 open-order feedback total overflow"))
    })?;
    match operation {
        P3ValidatedOperation::Place(draft) => {
            if actual_by_account
                .iter()
                .any(|(account, delta)| *delta > 0 && *account != draft.owner())
            {
                return Err(invariant(
                    "P3 Place feedback opens an order for a different account",
                ));
            }
            let has_impossible_positive_delta = match draft.kind() {
                P3PlaceKind::Limit => {
                    total > 1 || actual_by_account.values().any(|delta| *delta > 1)
                }
                P3PlaceKind::Market => actual_by_account.values().any(|delta| *delta > 0),
            };
            if has_impossible_positive_delta {
                return Err(invariant(
                    "P3 Place feedback contains an impossible positive order delta",
                ));
            }
        }
        P3ValidatedOperation::Cancel { account, .. } => {
            if actual_by_account
                .iter()
                .any(|(delta_account, _)| delta_account != account)
                || !matches!(total, -1 | 0)
            {
                return Err(invariant(
                    "P3 Cancel feedback must be zero or close its owner's order",
                ));
            }
        }
    }
    Ok(())
}

fn apply_count_delta(value: usize, delta: i64, label: &str) -> Result<usize, StepFatal> {
    if delta >= 0 {
        let delta = usize::try_from(delta)
            .map_err(|_| invariant(&format!("{label} does not fit in usize")))?;
        value
            .checked_add(delta)
            .ok_or_else(|| invariant(&format!("{label} overflow")))
    } else {
        let magnitude = delta
            .checked_abs()
            .and_then(|magnitude| usize::try_from(magnitude).ok())
            .ok_or_else(|| invariant(&format!("{label} magnitude does not fit in usize")))?;
        value
            .checked_sub(magnitude)
            .ok_or_else(|| invariant(&format!("{label} underflow")))
    }
}

fn money(error: crate::MoneyError) -> StepFatal {
    StepFatal::InvariantViolation {
        description: error.to_string(),
        location: "pipeline::p3_validation".to_owned(),
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p3_validation".to_owned(),
    }
}

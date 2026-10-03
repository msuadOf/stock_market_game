use super::{
    DecisionResourceSnapshot, Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, FeeComponents,
    IntentCandidateBatch, IntentCandidateKey, ResVec, StepFatal,
};
use crate::{
    AccountId, GameConfig, Intent, LimitPrice, Money, OrderId, RejectionReason, SecurityCategory,
    Side, StockCode,
};
use rayon::prelude::*;
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StockValidation {
    category: SecurityCategory,
    market_buy_protective_price: Money,
    market_sell_protective_price: Money,
}

impl StockValidation {
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

    const fn extreme_price(self, highest: bool) -> Money {
        if highest {
            self.market_buy_protective_price
        } else {
            self.market_sell_protective_price
        }
    }
}

/// Immutable tick-start facts needed by P3. It deliberately contains no session reference.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AccountValidationContext {
    stocks: BTreeMap<StockCode, StockValidation>,
}

impl AccountValidationContext {
    pub fn new(
        stocks: impl IntoIterator<Item = (StockCode, StockValidation)>,
    ) -> Result<Self, StepFatal> {
        let mut stock_map = BTreeMap::new();
        for (code, validation) in stocks {
            if stock_map.insert(code, validation).is_some() {
                return Err(invariant("duplicate stock in P3 validation context"));
            }
        }
        Ok(Self { stocks: stock_map })
    }

    fn stock(&self, code: &StockCode) -> Option<StockValidation> {
        self.stocks.get(code).copied()
    }
}

#[derive(Clone, Debug)]
pub struct CandidateValidationInput {
    candidates: IntentCandidateBatch,
    resources: DecisionResourceSnapshot,
    ledger: EnvelopeLedger,
    next_order_id: u64,
    config: GameConfig,
    context: AccountValidationContext,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PlaceKind {
    Limit,
    Market,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EnvelopeDraft {
    candidate_key: IntentCandidateKey,
    sealed_index: u64,
    envelope_key: EnvelopeKey,
    kind: PlaceKind,
    requested_price: Option<LimitPrice>,
    limit: Money,
    qty: u32,
    required: ResVec,
}

#[derive(Clone, Debug, PartialEq)]
pub enum CandidateValidationResult {
    Accepted {
        key: IntentCandidateKey,
        sealed_index: u64,
    },
    Rejected {
        key: IntentCandidateKey,
        sealed_index: u64,
        reason: RejectionReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ValidatedOperation {
    Place(EnvelopeDraft),
    Cancel {
        candidate_key: IntentCandidateKey,
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct AccountValidationOutput {
    results: Vec<CandidateValidationResult>,
    operations: Vec<ValidatedOperation>,
    drafts: Vec<EnvelopeDraft>,
    identities: Vec<ValidatedCandidateIdentity>,
    next_order_id_after: u64,
    next_sealed_index_after: u64,
}

#[derive(Clone, Debug)]
pub(super) struct AccountValidationState {
    resources: Arc<DecisionResourceSnapshot>,
    config: GameConfig,
    context: Arc<AccountValidationContext>,
    budgets: BTreeMap<AccountId, AccountBudget>,
    next_sealed_index: u64,
    processed_count: u64,
    output: AccountValidationOutput,
    #[cfg(test)]
    pub(super) last_round_account_shards: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct ValidatedCandidateIdentity {
    key: IntentCandidateKey,
    sealed_index: u64,
    allocated_order_id: Option<OrderId>,
}

#[derive(Clone, Debug)]
pub(super) struct ValidatedStep {
    pub(super) result: CandidateValidationResult,
    pub(super) operation: Option<ValidatedOperation>,
    identity: ValidatedCandidateIdentity,
}

#[derive(Clone, Debug)]
enum PreparedValidationStep {
    Place {
        place: UnkeyedEnvelopeDraft,
        required: ResVec,
    },
    Cancel {
        key: IntentCandidateKey,
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
    },
    Rejected {
        key: IntentCandidateKey,
        sealed_index: u64,
        reason: RejectionReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
enum AccountResourceLane {
    Cash(AccountId),
    Shares(AccountId, StockCode),
    Cancel(AccountId, StockCode, OrderId),
}

impl AccountResourceLane {
    fn account(&self) -> AccountId {
        match self {
            Self::Cash(account) | Self::Shares(account, _) | Self::Cancel(account, _, _) => {
                *account
            }
        }
    }
}

impl CandidateValidationInput {
    pub fn new_with_context(
        candidates: IntentCandidateBatch,
        resources: DecisionResourceSnapshot,
        ledger: EnvelopeLedger,
        next_order_id: u64,
        config: GameConfig,
        context: AccountValidationContext,
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

    pub fn validate(&self) -> Result<AccountValidationOutput, StepFatal> {
        let mut state = AccountValidationState::new(
            self.resources.clone(),
            self.next_order_id,
            0,
            self.config.clone(),
            self.context.clone(),
        );
        state.consume_round(self.candidates.candidates())?;
        Ok(state.into_output())
    }

    pub fn ledger(&self) -> &EnvelopeLedger {
        &self.ledger
    }
}

impl AccountValidationState {
    pub(super) fn new(
        resources: DecisionResourceSnapshot,
        next_order_id: u64,
        next_sealed_index: u64,
        config: GameConfig,
        context: AccountValidationContext,
    ) -> Self {
        Self {
            resources: Arc::new(resources),
            config,
            context: Arc::new(context),
            budgets: BTreeMap::new(),
            next_sealed_index,
            processed_count: 0,
            output: AccountValidationOutput {
                results: Vec::new(),
                operations: Vec::new(),
                drafts: Vec::new(),
                identities: Vec::new(),
                next_order_id_after: next_order_id,
                next_sealed_index_after: next_sealed_index,
            },
            #[cfg(test)]
            last_round_account_shards: 0,
        }
    }

    /// Performs P3's two passes for one ready round without copying previous rounds'
    /// results. A failed round never changes the cumulative validator.
    pub(super) fn consume_round(
        &mut self,
        candidates: &[super::IntentCandidate],
    ) -> Result<Vec<ValidatedStep>, StepFatal> {
        let (round, steps) = self.prepare_round(candidates)?;
        self.commit_round(round);
        Ok(steps)
    }

    /// Only accounts touched by this round need private budgets. Previous candidate results,
    /// drafts and identities stay in the cumulative state until the round succeeds.
    pub(super) fn prepare_round(
        &self,
        candidates: &[super::IntentCandidate],
    ) -> Result<(Self, Vec<ValidatedStep>), StepFatal> {
        let accounts = candidates
            .iter()
            .map(super::IntentCandidate::owner)
            .collect::<BTreeSet<_>>();
        let mut round = Self {
            resources: Arc::clone(&self.resources),
            config: self.config.clone(),
            context: Arc::clone(&self.context),
            budgets: accounts
                .iter()
                .filter_map(|account| {
                    self.budgets
                        .get(account)
                        .cloned()
                        .map(|budget| (*account, budget))
                })
                .collect(),
            next_sealed_index: self.next_sealed_index,
            processed_count: self.processed_count,
            output: AccountValidationOutput {
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
        let steps = round.consume_round_in_place(candidates)?;
        Ok((round, steps))
    }

    /// No fallible work remains here: install the validated account patches and append this
    /// round's new facts in the order supplied by the ready round.
    pub(super) fn commit_round(&mut self, round: Self) {
        self.budgets.extend(round.budgets);
        self.next_sealed_index = round.next_sealed_index;
        self.processed_count = round.processed_count;
        self.output.results.extend(round.output.results);
        self.output.operations.extend(round.output.operations);
        self.output.drafts.extend(round.output.drafts);
        self.output.identities.extend(round.output.identities);
        self.output.next_order_id_after = round.output.next_order_id_after;
        self.output.next_sealed_index_after = round.output.next_sealed_index_after;
        #[cfg(test)]
        {
            self.last_round_account_shards = round.last_round_account_shards;
        }
    }

    fn consume_round_in_place(
        &mut self,
        candidates: &[super::IntentCandidate],
    ) -> Result<Vec<ValidatedStep>, StepFatal> {
        let prepared = self.prepare_account_round(candidates)?;

        let place_count = prepared
            .iter()
            .filter(|step| matches!(step, PreparedValidationStep::Place { .. }))
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
                PreparedValidationStep::Place { place, required } => {
                    let order_id = OrderId(next_order_id);
                    next_order_id = next_order_id
                        .checked_add(1)
                        .ok_or_else(|| invariant("P3 order ID allocation overflow"))?;
                    accepted_place(place, required, order_id)
                }
                PreparedValidationStep::Cancel {
                    key,
                    sealed_index,
                    account,
                    code,
                    order_id,
                } => accepted_cancel(key, sealed_index, account, code, order_id),
                PreparedValidationStep::Rejected {
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

    /// Requests sharing cash or the same sellable shares keep their account-local arrival
    /// order. Independent resource lanes validate concurrently.
    fn prepare_account_round(
        &mut self,
        candidates: &[super::IntentCandidate],
    ) -> Result<Vec<PreparedValidationStep>, StepFatal> {
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
        let mut grouped =
            BTreeMap::<AccountResourceLane, Vec<(usize, &super::IntentCandidate)>>::new();
        for (index, candidate) in candidates.iter().enumerate() {
            let account = candidate.owner();
            let lane = match candidate.intent() {
                Intent::PlaceLimit {
                    code,
                    side: Side::Sell,
                    ..
                }
                | Intent::PlaceMarket {
                    code,
                    side: Side::Sell,
                    ..
                } => AccountResourceLane::Shares(account, code.clone()),
                Intent::Cancel { code, id } => {
                    AccountResourceLane::Cancel(account, code.clone(), *id)
                }
                _ => AccountResourceLane::Cash(account),
            };
            grouped.entry(lane).or_default().push((index, candidate));
        }
        let work = grouped.into_iter().collect::<Vec<_>>();
        #[cfg(any(test, feature = "verification-harness"))]
        let work = {
            let mut work = work;
            super::executor_perturbation::reorder(
                super::ExecutorBoundary::AccountValidationShards,
                &mut work,
                |(lane, entries)| (format!("{lane:?}"), entries.len()),
            );
            work
        };
        let results = work
            .par_iter()
            .map(|(lane, entries)| {
                let mut worker = self.account_worker(lane.account());
                let mut prepared = Vec::with_capacity(entries.len());
                for (index, candidate) in entries {
                    worker.next_sealed_index = self.next_sealed_index + *index as u64;
                    let step = worker.prepare(candidate);
                    let failed = step.is_err();
                    prepared.push((*index, step));
                    if failed {
                        break;
                    }
                }
                ((lane.clone(), worker), prepared)
            })
            .collect::<Vec<_>>();
        #[cfg(any(test, feature = "verification-harness"))]
        let results = {
            let mut results = results;
            super::executor_perturbation::reorder(
                super::ExecutorBoundary::AccountValidationWorkerResults,
                &mut results,
                |((lane, _), prepared)| (format!("{lane:?}"), prepared.len()),
            );
            results
        };
        #[cfg(test)]
        {
            self.last_round_account_shards = results.len();
        }
        let mut prepared = (0..candidates.len()).map(|_| None).collect::<Vec<_>>();
        let mut first_error: Option<(usize, StepFatal)> = None;
        for ((lane, worker), steps) in results {
            let account = lane.account();
            if let Some(budget) = worker.budgets.get(&account) {
                match lane {
                    AccountResourceLane::Cash(_) => {
                        self.budgets
                            .entry(account)
                            .or_insert_with(|| budget.clone())
                            .adopt_cash_lane(budget);
                    }
                    AccountResourceLane::Shares(_, code) => {
                        if let Some(shares) = budget.sellable.get(&code) {
                            self.budgets
                                .entry(account)
                                .or_insert_with(|| budget.clone())
                                .adopt_shares_lane(code, *shares);
                        }
                    }
                    AccountResourceLane::Cancel(_, _, _) => {}
                }
            }
            for (index, step) in steps {
                match step {
                    Ok(step) => {
                        if prepared[index].replace(step).is_some() {
                            return Err(invariant("P3 candidate was prepared twice"));
                        }
                    }
                    Err(error) if first_error.as_ref().is_none_or(|(first, _)| index < *first) => {
                        first_error = Some((index, error));
                    }
                    Err(_) => {}
                }
            }
        }
        if let Some((_, error)) = first_error {
            return Err(error);
        }
        self.next_sealed_index = next_sealed_index;
        self.processed_count = processed_count;
        prepared
            .into_iter()
            .map(|step| step.ok_or_else(|| invariant("P3 candidate result is missing")))
            .collect()
    }

    fn account_worker(&self, account: AccountId) -> Self {
        Self {
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
            next_sealed_index: self.next_sealed_index,
            processed_count: 0,
            output: AccountValidationOutput {
                results: Vec::new(),
                operations: Vec::new(),
                drafts: Vec::new(),
                identities: Vec::new(),
                next_order_id_after: self.output.next_order_id_after,
                next_sealed_index_after: self.next_sealed_index,
            },
            #[cfg(test)]
            last_round_account_shards: 0,
        }
    }

    fn prepare(
        &mut self,
        candidate: &super::IntentCandidate,
    ) -> Result<PreparedValidationStep, StepFatal> {
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
            Intent::Cancel { code, id } => PreparedValidationStep::Cancel {
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
            } => {
                let Some(stock) = self.context.stock(code) else {
                    self.next_sealed_index = next_sealed_index;
                    self.processed_count = next_processed_count;
                    return Ok(PreparedValidationStep::Rejected {
                        key,
                        sealed_index,
                        reason: RejectionReason::UnknownStock,
                    });
                };
                self.prepare_place(PlaceRequest {
                    candidate,
                    key,
                    sealed_index,
                    code,
                    side: *side,
                    kind: PlaceKind::Limit,
                    requested_price: match price {
                        LimitPrice::Fixed(_) => None,
                        symbolic => Some(*symbolic),
                    },
                    limit: match price {
                        LimitPrice::Fixed(price) => *price,
                        LimitPrice::Highest => stock.extreme_price(true),
                        LimitPrice::Lowest => stock.extreme_price(false),
                    },
                    qty: *qty,
                })?
            }
            Intent::PlaceMarket { code, side, qty } => {
                let Some(stock) = self.context.stock(code) else {
                    self.next_sealed_index = next_sealed_index;
                    self.processed_count = next_processed_count;
                    return Ok(PreparedValidationStep::Rejected {
                        key,
                        sealed_index,
                        reason: RejectionReason::UnknownStock,
                    });
                };
                self.prepare_place(PlaceRequest {
                    candidate,
                    key,
                    sealed_index,
                    code,
                    side: *side,
                    kind: PlaceKind::Market,
                    requested_price: None,
                    limit: stock.protective_price(*side),
                    qty: *qty,
                })?
            }
        };
        self.next_sealed_index = next_sealed_index;
        self.processed_count = next_processed_count;
        Ok(prepared)
    }

    fn prepare_place(
        &mut self,
        request: PlaceRequest<'_>,
    ) -> Result<PreparedValidationStep, StepFatal> {
        let PlaceRequest {
            candidate,
            key,
            sealed_index,
            code,
            side,
            kind,
            requested_price,
            limit,
            qty,
        } = request;
        let Some(stock) = self.context.stock(code) else {
            return Ok(PreparedValidationStep::Rejected {
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
            requested_price,
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
                return Ok(PreparedValidationStep::Rejected {
                    key,
                    sealed_index,
                    reason,
                });
            }
        };
        self.apply_budget_update(place.account, prepared.budget_update);
        Ok(PreparedValidationStep::Place {
            place,
            required: prepared.required,
        })
    }

    fn ensure_account_budget(&mut self, account: AccountId) -> Result<(), StepFatal> {
        if !self.budgets.contains_key(&account) {
            self.budgets.insert(
                account,
                AccountBudget::ensure_from_snapshot(account, &self.resources)?,
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
        budget.ensure_sellable_from_snapshot(account, code, &self.resources)
    }

    fn validate_place(
        &self,
        stock: StockValidation,
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
        budget.apply_update(update);
    }

    fn commit_step(&mut self, step: ValidatedStep) {
        self.output.identities.push(step.identity);
        self.output.results.push(step.result);
        if let Some(operation) = step.operation {
            if let ValidatedOperation::Place(draft) = &operation {
                self.output.drafts.push(draft.clone());
            }
            self.output.operations.push(operation);
        }
    }

    pub(super) const fn output(&self) -> &AccountValidationOutput {
        &self.output
    }

    pub(super) fn resources(&self) -> Arc<DecisionResourceSnapshot> {
        Arc::clone(&self.resources)
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

    pub(super) fn into_output(self) -> AccountValidationOutput {
        self.output
    }
}

struct PlaceRequest<'candidate> {
    candidate: &'candidate super::IntentCandidate,
    key: IntentCandidateKey,
    sealed_index: u64,
    code: &'candidate StockCode,
    side: Side,
    kind: PlaceKind,
    requested_price: Option<LimitPrice>,
    limit: Money,
    qty: u32,
}

fn rejected(key: IntentCandidateKey, sealed_index: u64, reason: RejectionReason) -> ValidatedStep {
    let identity = ValidatedCandidateIdentity {
        key: key.clone(),
        sealed_index,
        allocated_order_id: None,
    };
    ValidatedStep {
        result: CandidateValidationResult::Rejected {
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
) -> ValidatedStep {
    let key = place.candidate_key.clone();
    let sealed_index = place.sealed_index;
    let identity = ValidatedCandidateIdentity {
        key: key.clone(),
        sealed_index,
        allocated_order_id: Some(order_id),
    };
    ValidatedStep {
        result: CandidateValidationResult::Accepted { key, sealed_index },
        operation: Some(ValidatedOperation::Place(
            place.with_order_id(order_id, required),
        )),
        identity,
    }
}

fn accepted_cancel(
    key: IntentCandidateKey,
    sealed_index: u64,
    account: AccountId,
    code: StockCode,
    order_id: OrderId,
) -> ValidatedStep {
    let identity = ValidatedCandidateIdentity {
        key: key.clone(),
        sealed_index,
        allocated_order_id: None,
    };
    ValidatedStep {
        result: CandidateValidationResult::Accepted {
            key: key.clone(),
            sealed_index,
        },
        operation: Some(ValidatedOperation::Cancel {
            candidate_key: key,
            sealed_index,
            account,
            code,
            order_id,
        }),
        identity,
    }
}

impl ValidatedStep {
    pub(super) const fn candidate_key(&self) -> &IntentCandidateKey {
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
    pub const fn candidate_key(&self) -> &IntentCandidateKey {
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

    pub const fn kind(&self) -> PlaceKind {
        self.kind
    }

    pub const fn requested_price(&self) -> Option<LimitPrice> {
        self.requested_price
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
        Envelope::created_at_validation_with_pending_price(
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
            self.requested_price,
        )
    }
}

impl ValidatedOperation {
    pub const fn candidate_key(&self) -> &IntentCandidateKey {
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

impl CandidateValidationResult {
    pub const fn key(&self) -> &IntentCandidateKey {
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

impl AccountValidationOutput {
    pub fn accepted(&self) -> impl Iterator<Item = &IntentCandidateKey> {
        self.results.iter().filter_map(|result| match result {
            CandidateValidationResult::Accepted { key, .. } => Some(key),
            CandidateValidationResult::Rejected { .. } => None,
        })
    }

    pub fn rejected(&self) -> impl Iterator<Item = (&IntentCandidateKey, &RejectionReason)> {
        self.results.iter().filter_map(|result| match result {
            CandidateValidationResult::Accepted { .. } => None,
            CandidateValidationResult::Rejected { key, reason, .. } => Some((key, reason)),
        })
    }

    pub fn results(&self) -> &[CandidateValidationResult] {
        &self.results
    }

    pub fn operations(&self) -> &[ValidatedOperation] {
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
    ) -> impl ExactSizeIterator<Item = (&IntentCandidateKey, u64, Option<OrderId>)> {
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
    candidate_key: IntentCandidateKey,
    sealed_index: u64,
    account: AccountId,
    code: StockCode,
    side: Side,
    kind: PlaceKind,
    requested_price: Option<LimitPrice>,
    limit: Money,
    qty: u32,
}

impl UnkeyedEnvelopeDraft {
    const fn is_market(&self) -> bool {
        matches!(self.kind, PlaceKind::Market)
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
            requested_price: self.requested_price,
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

impl AccountBudget {
    fn ensure_from_snapshot(
        account: AccountId,
        resources: &DecisionResourceSnapshot,
    ) -> Result<Self, StepFatal> {
        Ok(Self {
            cash: resources.available_cash(account)?,
            sellable: BTreeMap::new(),
        })
    }

    fn ensure_sellable_from_snapshot(
        &mut self,
        account: AccountId,
        code: &StockCode,
        resources: &DecisionResourceSnapshot,
    ) -> Result<(), StepFatal> {
        if !self.sellable.contains_key(code) {
            let available = resources.available_sell_qty(account, code)?;
            self.sellable.insert(code.clone(), available);
        }
        Ok(())
    }

    fn apply_update(&mut self, update: BudgetUpdate) {
        match update {
            BudgetUpdate::Cash { cash_after } => self.cash = cash_after,
            BudgetUpdate::Shares { code, shares_after } => {
                self.sellable.insert(code, shares_after);
            }
        }
    }

    fn adopt_cash_lane(&mut self, lane: &Self) {
        self.cash = lane.cash;
    }

    fn adopt_shares_lane(&mut self, code: StockCode, shares: u32) {
        self.sellable.insert(code, shares);
    }
}

struct PreparedReservation {
    required: ResVec,
    budget_update: BudgetUpdate,
}

enum BudgetUpdate {
    Cash { cash_after: Money },
    Shares { code: StockCode, shares_after: u32 },
}

fn money(error: crate::MoneyError) -> StepFatal {
    StepFatal::InvariantViolation {
        description: error.to_string(),
        location: "pipeline::account_validation".to_owned(),
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::account_validation".to_owned(),
    }
}

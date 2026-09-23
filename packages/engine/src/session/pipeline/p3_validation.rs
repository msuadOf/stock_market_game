use super::{
    DecisionResourceSnapshot, Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, FeeComponents,
    P2CandidateBatch, P2CandidateKey, ResVec, StepFatal,
};
use crate::{
    AccountId, GameConfig, Intent, Money, OrderId, RejectionReason, SecurityCategory, Side,
    StockCode, MAX_OPEN_ORDERS, MAX_OPEN_ORDERS_PER_ACCOUNT,
};
use std::collections::BTreeMap;

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
    next_order_id_after: u64,
}

#[derive(Clone, Debug)]
pub(super) struct P3ValidationState {
    resources: DecisionResourceSnapshot,
    config: GameConfig,
    context: P3ValidationContext,
    budgets: BTreeMap<AccountId, AccountBudget>,
    open_orders: OpenOrderBudget,
    next_sealed_index: u64,
    output: P3ValidationOutput,
}

#[derive(Clone, Debug)]
pub(super) struct P3ValidatedStep {
    pub(super) result: P3CandidateResult,
    pub(super) operation: Option<P3ValidatedOperation>,
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
            let category = if code.0.starts_with("300") || code.0.starts_with("301") {
                SecurityCategory::ChiNext
            } else {
                SecurityCategory::MainBoard
            };
            stocks
                .entry(code.clone())
                .or_insert(P3StockValidation::new(category, price, price));
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
            self.config.clone(),
            self.context.clone(),
        );
        for candidate in self.candidates.candidates() {
            state.consume(candidate)?;
        }
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
        config: GameConfig,
        context: P3ValidationContext,
    ) -> Self {
        Self {
            resources,
            config,
            open_orders: OpenOrderBudget::new(&context),
            context,
            budgets: BTreeMap::new(),
            next_sealed_index: 0,
            output: P3ValidationOutput {
                results: Vec::new(),
                operations: Vec::new(),
                drafts: Vec::new(),
                next_order_id_after: next_order_id,
            },
        }
    }

    pub(super) fn consume(
        &mut self,
        candidate: &super::P2Candidate,
    ) -> Result<P3ValidatedStep, StepFatal> {
        let sealed_index = self.next_sealed_index;
        let next_sealed_index = sealed_index
            .checked_add(1)
            .ok_or_else(|| invariant("P3 sealed candidate index overflow"))?;
        let key = candidate.key().clone();
        let step = match candidate.intent() {
            Intent::Cancel { code, id } => P3ValidatedStep {
                result: P3CandidateResult::Accepted {
                    key: key.clone(),
                    sealed_index,
                },
                operation: Some(P3ValidatedOperation::Cancel {
                    candidate_key: key,
                    sealed_index,
                    account: candidate.owner(),
                    code: code.clone(),
                    order_id: *id,
                }),
            },
            Intent::PlaceLimit {
                code,
                side,
                price,
                qty,
            } => self.consume_place(
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
                    let step = rejected(key, sealed_index, RejectionReason::UnknownStock);
                    self.commit_step(step.clone(), next_sealed_index);
                    return Ok(step);
                };
                self.consume_place(
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
        self.commit_step(step.clone(), next_sealed_index);
        Ok(step)
    }

    fn consume_place(
        &mut self,
        candidate: &super::P2Candidate,
        key: P2CandidateKey,
        sealed_index: u64,
        code: &StockCode,
        side: Side,
        kind: P3PlaceKind,
        limit: Money,
        qty: u32,
    ) -> Result<P3ValidatedStep, StepFatal> {
        let Some(stock) = self.context.stock(code) else {
            return Ok(rejected(key, sealed_index, RejectionReason::UnknownStock));
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
        let prepared = match self.validate_place(stock, &place)? {
            Ok(prepared) => prepared,
            Err(reason) => return Ok(rejected(key, sealed_index, reason)),
        };
        let order_id = OrderId(self.output.next_order_id_after);
        let next_order_id_after = self
            .output
            .next_order_id_after
            .checked_add(1)
            .ok_or_else(|| invariant("P3 order ID allocation overflow"))?;
        if kind == P3PlaceKind::Limit {
            self.open_orders.accept_limit(candidate.owner())?;
        }
        self.apply_budget_update(place.account, prepared.budget_update);
        self.output.next_order_id_after = next_order_id_after;
        let draft = place.with_order_id(order_id, prepared.required);
        Ok(P3ValidatedStep {
            result: P3CandidateResult::Accepted { key, sealed_index },
            operation: Some(P3ValidatedOperation::Place(draft)),
        })
    }

    fn validate_place(
        &self,
        stock: P3StockValidation,
        place: &UnkeyedEnvelopeDraft,
    ) -> Result<Result<PreparedReservation, RejectionReason>, StepFatal> {
        if place.qty == 0 || place.qty > stock.category.max_order_qty(place.is_market()) {
            return Ok(Err(RejectionReason::InvalidQuantity));
        }
        let existing_budget = self.budgets.get(&place.account);
        let current_cash = match existing_budget {
            Some(budget) => budget.cash,
            None => self.resources.available_cash(place.account)?,
        };
        if place.side == Side::Buy && !place.qty.is_multiple_of(self.config.lot_size) {
            return Ok(Err(RejectionReason::InvalidQuantity));
        }
        let mut available_sell = None;
        if place.side == Side::Sell {
            let available = match existing_budget
                .and_then(|budget| budget.sellable.get(&place.code).copied())
            {
                Some(available) => available,
                None => self
                    .resources
                    .available_sell_qty(place.account, &place.code)?,
            };
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
                    initial_cash: current_cash,
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
        match update {
            BudgetUpdate::Cash { cash_after } => {
                self.budgets
                    .entry(account)
                    .and_modify(|budget| budget.cash = cash_after)
                    .or_insert_with(|| AccountBudget {
                        cash: cash_after,
                        sellable: BTreeMap::new(),
                    });
            }
            BudgetUpdate::Shares {
                initial_cash,
                code,
                shares_after,
            } => {
                self.budgets
                    .entry(account)
                    .or_insert_with(|| AccountBudget {
                        cash: initial_cash,
                        sellable: BTreeMap::new(),
                    })
                    .sellable
                    .insert(code, shares_after);
            }
        }
    }

    fn commit_step(&mut self, step: P3ValidatedStep, next_sealed_index: u64) {
        self.output.results.push(step.result);
        if let Some(operation) = step.operation {
            if let P3ValidatedOperation::Place(draft) = &operation {
                self.output.drafts.push(draft.clone());
            }
            self.output.operations.push(operation);
        }
        self.next_sealed_index = next_sealed_index;
    }

    pub(super) const fn output(&self) -> &P3ValidationOutput {
        &self.output
    }

    pub(super) const fn sealed_count(&self) -> u64 {
        self.next_sealed_index
    }

    pub(super) fn into_output(self) -> P3ValidationOutput {
        self.output
    }
}

fn rejected(key: P2CandidateKey, sealed_index: u64, reason: RejectionReason) -> P3ValidatedStep {
    P3ValidatedStep {
        result: P3CandidateResult::Rejected {
            key,
            sealed_index,
            reason,
        },
        operation: None,
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
    Cash {
        cash_after: Money,
    },
    Shares {
        initial_cash: Money,
        code: StockCode,
        shares_after: u32,
    },
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

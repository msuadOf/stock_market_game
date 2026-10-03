use super::{
    transition::{BuyFillInput, FillTransition, SellFillInput},
    Envelope, EnvelopeAudit, EnvelopeKey, EnvelopeLedger, EnvelopeOrigin, EnvelopeReceipt,
    FeeComponents, JournalRank, PlaceKind, ReceiptDelta, ReceiptKind, ReceiptLocalKey,
    ReceiptSource, ReceiptTransition, ResVec, StepFatal, ValidatedOperation,
};
use crate::market::MarketDelta;
use crate::{
    AccountId, GameConfig, Market, MarketError, Money, OrderId, RejectionReason, Side, StockCode,
    Trade, TradingPhase,
};
use std::collections::{BTreeMap, BTreeSet};

#[path = "incremental_continuous_stock_shadow.rs"]
mod incremental_continuous_stock_shadow;

pub(super) use incremental_continuous_stock_shadow::{
    ContinuousExecutionRound, IncrementalContinuousStockCoordinator,
    IncrementalContinuousStockFinish,
};

#[derive(Clone, Debug, PartialEq)]
pub(super) struct ContinuousEnvelopeSnapshot {
    pub(super) envelope: Envelope,
    pub(super) audit: EnvelopeAudit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ContinuousCancelOperation {
    pub(super) sealed_index: u64,
    pub(super) account: AccountId,
    pub(super) code: StockCode,
    pub(super) order_id: OrderId,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ContinuousCancelRejection {
    UnknownStock,
    OrderNotFound,
    OrderAlreadyFilled,
    NotOrderOwner,
    AuctionOrderNotCancelable,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum ContinuousCancelFact {
    Canceled {
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        side: Side,
        remaining_qty: u32,
    },
    Rejected {
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        reason: ContinuousCancelRejection,
    },
}

#[derive(Clone, Debug)]
#[cfg(test)]
pub(super) struct ContinuousCancelInput {
    pub(super) market: Market,
    pub(super) envelopes: Vec<ContinuousEnvelopeSnapshot>,
    pub(super) operation: ContinuousCancelOperation,
}

#[derive(Debug)]
#[cfg(test)]
pub(super) struct ContinuousCancelOutput {
    pub(super) market: Market,
    pub(super) receipt: Option<EnvelopeReceipt>,
    pub(super) terminal_key: Option<EnvelopeKey>,
    pub(super) fact: ContinuousCancelFact,
}

struct ContinuousCancelEffect {
    receipt: Option<EnvelopeReceipt>,
    terminal_key: Option<EnvelopeKey>,
    fact: ContinuousCancelFact,
}

#[derive(Clone, Debug)]
pub(super) struct ContinuousStockInput {
    pub(super) phase: TradingPhase,
    pub(super) market: Market,
    pub(super) envelopes: Vec<ContinuousEnvelopeSnapshot>,
    pub(super) operations: Vec<ValidatedOperation>,
    pub(super) config: GameConfig,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ContinuousPlaceFact {
    Resting {
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        side: Side,
        price: Money,
        remaining_qty: u32,
    },
    Filled {
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        side: Side,
        filled_qty: u32,
    },
    Rejected {
        sealed_index: u64,
        account: AccountId,
        code: StockCode,
        order_id: OrderId,
        reason: RejectionReason,
    },
}

/// 每个 P3 Accepted 操作对应唯一 typed P4 结果，供 continuation 按身份关联。
/// 不得从 Projection 事件重建：即时全成没有 `OrderAccepted`，P4 Rejected Place 仍拥有预分配 OrderId。
#[derive(Clone, Debug, PartialEq)]
pub(super) struct ContinuousExecutionFact {
    pub(super) candidate_key: super::IntentCandidateKey,
    pub(super) sealed_index: u64,
    pub(super) allocated_order_id: Option<OrderId>,
    pub(super) outcome: ContinuousExecutionOutcome,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum ContinuousExecutionOutcome {
    Place {
        fact: ContinuousPlaceFact,
        original_qty: u32,
    },
    Cancel(ContinuousCancelFact),
}

impl ContinuousExecutionFact {
    pub(super) const fn candidate_key(&self) -> &super::IntentCandidateKey {
        &self.candidate_key
    }

    #[cfg(test)]
    pub(super) const fn sealed_index(&self) -> u64 {
        self.sealed_index
    }

    pub(super) const fn allocated_order_id(&self) -> Option<OrderId> {
        self.allocated_order_id
    }

    pub(super) const fn outcome(&self) -> &ContinuousExecutionOutcome {
        &self.outcome
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct ContinuousTradeFact {
    pub(super) stock: StockCode,
    pub(super) triggering_sealed_index: u64,
    pub(super) stock_local_trade_event_index: u64,
    pub(super) trade: Trade,
}

#[derive(Debug)]
pub(super) struct ContinuousStockOutput {
    pub(super) market: Market,
    // 保留全部 P3 Place draft，即使 P4 拒绝或终结；下游先创建账本行、应用 receipts，
    // 再移除 terminal_keys。提前过滤终结 draft 会破坏资源守恒。
    pub(super) created_envelopes: Vec<Envelope>,
    pub(super) receipts: Vec<EnvelopeReceipt>,
    pub(super) terminal_keys: Vec<EnvelopeKey>,
    pub(super) trades: Vec<ContinuousTradeFact>,
    pub(super) place_facts: Vec<ContinuousPlaceFact>,
    pub(super) cancel_facts: Vec<ContinuousCancelFact>,
}

#[derive(Debug)]
pub(super) struct ContinuousStockStepOutput {
    pub(super) output: ContinuousStockOutput,
    pub(super) execution_facts: Vec<ContinuousExecutionFact>,
    pub(super) next_trade_event_index: u64,
    pub(super) ledger: EnvelopeLedger,
    pub(super) acceptance_quotes: BTreeMap<u64, ContinuousAcceptanceQuote>,
    pub(super) market_delta: MarketDelta,
    #[cfg(feature = "simulation-diagnostics")]
    pub(super) operation_quotes: BTreeMap<u64, ContinuousOperationQuotes>,
}

/// 限价单成为 resting 时的精确操作后行情；同股后续操作仍可能改变报价或令其全成。
#[derive(Clone, Debug)]
pub(super) struct ContinuousAcceptanceQuote {
    pub(super) order: crate::Order,
    pub(super) last_price: Money,
    pub(super) best_bid: Option<Money>,
    pub(super) best_ask: Option<Money>,
}

/// 已分配 OrderId 的 P4 Place 或成功撤单前后的精确订单簿观察。
/// `ContinuousAcceptanceQuote` 仅用于 NPC 工作单的操作后核对；诊断还需操作前快照，
/// 包括即时全成的市价单。worker 拒单未修改订单簿，因此 before/after 相同。
#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(feature = "simulation-diagnostics")]
pub(super) struct ContinuousQuoteSnapshot {
    pub(super) bid_cents: Option<i64>,
    pub(super) ask_cents: Option<i64>,
    pub(super) bid_depth: u64,
    pub(super) ask_depth: u64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
#[cfg(feature = "simulation-diagnostics")]
pub(super) struct ContinuousOperationQuotes {
    pub(super) before: ContinuousQuoteSnapshot,
    pub(super) after: ContinuousQuoteSnapshot,
}

#[cfg(test)]
pub(super) fn process_continuous_stock(
    input: ContinuousStockInput,
) -> Result<ContinuousStockOutput, StepFatal> {
    Ok(process_continuous_stock_step(input, 0)?.output)
}

pub(super) fn process_continuous_stock_step(
    input: ContinuousStockInput,
    next_trade_event_index: u64,
) -> Result<ContinuousStockStepOutput, StepFatal> {
    process_continuous_stock_step_inner(input, next_trade_event_index, None)
}

pub(super) fn process_continuous_stock_step_with_ledger(
    input: ContinuousStockInput,
    next_trade_event_index: u64,
    ledger: EnvelopeLedger,
) -> Result<ContinuousStockStepOutput, StepFatal> {
    process_continuous_stock_step_inner(input, next_trade_event_index, Some(ledger))
}

fn process_continuous_stock_step_inner(
    input: ContinuousStockInput,
    next_trade_event_index: u64,
    prior_ledger: Option<EnvelopeLedger>,
) -> Result<ContinuousStockStepOutput, StepFatal> {
    ContinuousStockRoundProcessor::new(input, next_trade_event_index, prior_ledger)?.run()
}

/// 单股票单轮的可丢弃候选状态；跨轮最终校验仍由 consuming finish 负责。
struct ContinuousStockRoundProcessor {
    phase: TradingPhase,
    config: GameConfig,
    operations: Vec<ValidatedOperation>,
    private_round: bool,
    before_last_price: Money,
    before_last_close: Money,
    before_next_seq: u64,
    original_orders: BTreeMap<OrderId, Option<crate::Order>>,
    output: ContinuousStockOutput,
    ledger: EnvelopeLedger,
    execution_facts: Vec<ContinuousExecutionFact>,
    acceptance_quotes: BTreeMap<u64, ContinuousAcceptanceQuote>,
    next_trade_event_index: u64,
    #[cfg(feature = "simulation-diagnostics")]
    operation_quotes: BTreeMap<u64, ContinuousOperationQuotes>,
}

impl ContinuousStockRoundProcessor {
    fn new(
        input: ContinuousStockInput,
        next_trade_event_index: u64,
        prior_ledger: Option<EnvelopeLedger>,
    ) -> Result<Self, StepFatal> {
        let private_round = prior_ledger.is_some();
        let before_last_price = input.market.last_price();
        let before_last_close = input.market.last_close();
        let before_next_seq = input.market.book_next_sequence();
        validate_operation_identities(&input.operations)?;
        if prior_ledger.is_some() {
            if !input.envelopes.is_empty() {
                return Err(invariant(
                    "private stock round must use its owned ledger instead of supplied snapshots",
                ));
            }
        } else {
            validate_initial_snapshots(&input.market, &input.envelopes)?;
        }

        let created_envelopes: Vec<_> = input
            .operations
            .iter()
            .filter_map(|operation| match operation {
                ValidatedOperation::Place(draft) => Some(draft.materialize_envelope()),
                ValidatedOperation::Cancel { .. } => None,
            })
            .collect();
        for envelope in &created_envelopes {
            envelope.validate()?;
        }
        let ledger = if let Some(mut ledger) = prior_ledger {
            ledger.insert_created_for_stock_round(created_envelopes.iter().cloned())?;
            ledger
        } else {
            let ledger_envelopes = input
                .envelopes
                .iter()
                .map(|snapshot| snapshot.envelope.clone())
                .chain(created_envelopes.iter().cloned());
            EnvelopeLedger::new(0, ledger_envelopes)?
        };
        let output = ContinuousStockOutput {
            market: input.market,
            created_envelopes,
            receipts: Vec::new(),
            terminal_keys: Vec::new(),
            trades: Vec::new(),
            place_facts: Vec::new(),
            cancel_facts: Vec::new(),
        };

        Ok(Self {
            phase: input.phase,
            config: input.config,
            operations: input.operations,
            private_round,
            before_last_price,
            before_last_close,
            before_next_seq,
            original_orders: BTreeMap::new(),
            output,
            ledger,
            execution_facts: Vec::new(),
            acceptance_quotes: BTreeMap::new(),
            next_trade_event_index,
            #[cfg(feature = "simulation-diagnostics")]
            operation_quotes: BTreeMap::new(),
        })
    }

    fn run(mut self) -> Result<ContinuousStockStepOutput, StepFatal> {
        for operation in std::mem::take(&mut self.operations) {
            #[cfg(feature = "simulation-diagnostics")]
            let quote_before = quote_snapshot(&self.output.market)?;
            match operation {
                ValidatedOperation::Cancel {
                    candidate_key,
                    sealed_index,
                    account,
                    code,
                    order_id,
                } => {
                    if self.phase == TradingPhase::PreOpen {
                        let fact = ContinuousCancelFact::Rejected {
                            sealed_index,
                            account,
                            code,
                            order_id,
                            reason: ContinuousCancelRejection::AuctionOrderNotCancelable,
                        };
                        self.output.cancel_facts.push(fact.clone());
                        self.execution_facts.push(ContinuousExecutionFact {
                            candidate_key,
                            sealed_index,
                            allocated_order_id: None,
                            outcome: ContinuousExecutionOutcome::Cancel(fact),
                        });
                        continue;
                    }
                    if self.phase != TradingPhase::Continuous {
                        return Err(invariant(
                            "continuous cancellation was routed outside continuous trading",
                        ));
                    }
                    let prior_order = self.output.market.resting_order_by_id(order_id).cloned();
                    let cancel = cancel_continuous_order_from_private_ledger(
                        &mut self.output.market,
                        &self.ledger,
                        ContinuousCancelOperation {
                            sealed_index,
                            account,
                            code,
                            order_id,
                        },
                    )?;
                    if let Some(receipt) = cancel.receipt {
                        let terminal = cancel.terminal_key.ok_or_else(|| {
                            invariant("cancel receipt has no terminal envelope key")
                        })?;
                        self.apply_receipts(
                            std::slice::from_ref(&receipt),
                            std::slice::from_ref(&terminal),
                        )?;
                        self.output.receipts.push(receipt);
                        self.output.terminal_keys.push(terminal);
                    } else if cancel.terminal_key.is_some() {
                        return Err(invariant("rejected cancellation exposes a terminal key"));
                    }
                    let fact = cancel.fact;
                    if matches!(fact, ContinuousCancelFact::Canceled { .. }) {
                        let prior_order = prior_order.ok_or_else(|| {
                            invariant("successful cancellation had no prior resting order")
                        })?;
                        self.original_orders
                            .entry(order_id)
                            .or_insert(Some(prior_order));
                    }
                    #[cfg(feature = "simulation-diagnostics")]
                    if matches!(fact, ContinuousCancelFact::Canceled { .. }) {
                        insert_operation_quotes(
                            &mut self.operation_quotes,
                            sealed_index,
                            quote_before,
                            quote_snapshot(&self.output.market)?,
                        )?;
                    }
                    self.output.cancel_facts.push(fact.clone());
                    self.execution_facts.push(ContinuousExecutionFact {
                        candidate_key,
                        sealed_index,
                        allocated_order_id: None,
                        outcome: ContinuousExecutionOutcome::Cancel(fact),
                    });
                }
                ValidatedOperation::Place(draft) => {
                    if let Some(reason) = place_phase_rejection(self.phase, draft.kind())? {
                        self.reject_place(&draft, reason)?;
                        #[cfg(feature = "simulation-diagnostics")]
                        insert_unchanged_operation_quote(
                            &mut self.operation_quotes,
                            draft.sealed_index(),
                            quote_before,
                        )?;
                        continue;
                    }
                    if draft.code() != self.output.market.code() {
                        self.reject_place(&draft, RejectionReason::UnknownStock)?;
                        #[cfg(feature = "simulation-diagnostics")]
                        insert_unchanged_operation_quote(
                            &mut self.operation_quotes,
                            draft.sealed_index(),
                            quote_before,
                        )?;
                        continue;
                    }
                    let (price, resolution) = super::price_resolution::resolve_draft(
                        &draft,
                        self.ledger.get(draft.key())?,
                        &self.output.market,
                        &self.config,
                        self.phase == TradingPhase::Continuous && self.config.price_cage_enabled,
                    )?;
                    if self.config.price_cage_enabled && draft.kind() == PlaceKind::Limit {
                        let bound = self
                            .output
                            .market
                            .continuous_limit_bound(draft.side())
                            .map_err(|error| invariant(&error.to_string()))?;
                        let outside = match draft.side() {
                            Side::Buy => price > bound,
                            Side::Sell => price < bound,
                        };
                        if outside {
                            self.reject_place(&draft, RejectionReason::PriceCageExceeded)?;
                            #[cfg(feature = "simulation-diagnostics")]
                            insert_unchanged_operation_quote(
                                &mut self.operation_quotes,
                                draft.sealed_index(),
                                quote_before,
                            )?;
                            continue;
                        }
                    }

                    // LimitExceeded 在修改订单簿前检查；其他错误丢弃整个 private tick candidate，
                    // 因此无须为每张 incoming 委托复制不断增长的订单簿。
                    let result = match self.output.market.place_recording(crate::Order {
                        id: draft.order_id(),
                        side: draft.side(),
                        price,
                        qty: draft.qty(),
                        original_qty: draft.qty(),
                        filled_qty: 0,
                        filled_value: Money::ZERO,
                        owner: draft.owner(),
                        seq: 0,
                    }) {
                        Ok(result) => result,
                        Err(MarketError::LimitExceeded { .. }) => {
                            self.reject_place(&draft, RejectionReason::LimitExceeded)?;
                            #[cfg(feature = "simulation-diagnostics")]
                            insert_unchanged_operation_quote(
                                &mut self.operation_quotes,
                                draft.sealed_index(),
                                quote_before,
                            )?;
                            continue;
                        }
                        Err(error) => return Err(invariant(&error.to_string())),
                    };
                    for maker in result.maker_before {
                        self.original_orders.entry(maker.id).or_insert(Some(maker));
                    }
                    self.original_orders.entry(draft.order_id()).or_insert(None);
                    let trades = result.trades;
                    let resting = result.resting;
                    if draft.kind() == PlaceKind::Market && resting.is_some() {
                        self.output
                            .market
                            .cancel(draft.order_id())
                            .map_err(|error| invariant(&error.to_string()))?;
                    }

                    let (mut receipts, states, mut ordinals) =
                        self.fill_receipts(&draft, &trades, resolution.as_ref())?;
                    if let Some(receipt) = resolution {
                        receipts.insert(0, receipt);
                    }
                    let mut terminals = terminal_fill_keys(&receipts);
                    if draft.kind() == PlaceKind::Market {
                        let incoming = states
                            .get(draft.key())
                            .ok_or_else(|| invariant("market order has no post-fill envelope"))?;
                        if incoming.live() != ResVec::ZERO {
                            let ordinal = take_ordinal(&mut ordinals, draft.key())?;
                            receipts.push(terminal_receipt(
                                draft.sealed_index(),
                                incoming,
                                ReceiptKind::Release,
                                ordinal,
                            )?);
                        }
                        terminals.insert(draft.key().clone());
                    }
                    let terminal_keys: Vec<_> = terminals.into_iter().collect();
                    self.apply_receipts(&receipts, &terminal_keys)?;
                    self.output.receipts.extend(receipts);
                    self.output.terminal_keys.extend(terminal_keys);
                    append_trade_facts(
                        draft.code(),
                        draft.sealed_index(),
                        &trades,
                        &mut self.next_trade_event_index,
                        &mut self.output.trades,
                    )?;
                    #[cfg(feature = "simulation-diagnostics")]
                    insert_operation_quotes(
                        &mut self.operation_quotes,
                        draft.sealed_index(),
                        quote_before,
                        quote_snapshot(&self.output.market)?,
                    )?;

                    if draft.kind() == PlaceKind::Limit {
                        if let Some(resting) = resting {
                            let quote = ContinuousAcceptanceQuote {
                                order: resting.clone(),
                                last_price: self.output.market.last_price(),
                                best_bid: self.output.market.best_bid(),
                                best_ask: self.output.market.best_ask(),
                            };
                            if self
                                .acceptance_quotes
                                .insert(draft.sealed_index(), quote)
                                .is_some()
                            {
                                return Err(invariant("duplicate sealed acceptance quote"));
                            }
                            self.output.place_facts.push(ContinuousPlaceFact::Resting {
                                sealed_index: draft.sealed_index(),
                                account: draft.owner(),
                                code: draft.code().clone(),
                                order_id: draft.order_id(),
                                side: draft.side(),
                                price,
                                remaining_qty: resting.qty,
                            });
                            self.record_place_fact(&draft)?;
                            continue;
                        }
                    }
                    let filled_qty = trades.iter().try_fold(0_u32, |total, trade| {
                        total
                            .checked_add(trade.qty)
                            .ok_or_else(|| invariant("incoming filled quantity overflow"))
                    })?;
                    self.output.place_facts.push(ContinuousPlaceFact::Filled {
                        sealed_index: draft.sealed_index(),
                        account: draft.owner(),
                        code: draft.code().clone(),
                        order_id: draft.order_id(),
                        side: draft.side(),
                        filled_qty,
                    });
                    self.record_place_fact(&draft)?;
                }
            }
        }
        validate_account_fact_identities(&self.output.place_facts, &self.output.cancel_facts)?;
        validate_execution_facts(&self.execution_facts)?;
        if !self.private_round {
            // 初始 shadow 与独立 worker 在这里拒绝错误来源证据；后续轮次校验每次转换，
            // 全部轮次完成后再统一核对订单簿与 ledger。
            self.ledger.validate_complete_evidence()?;
            validate_private_market_ledger(&self.output.market, &self.ledger, &self.config)?;
        }
        let market_delta = self
            .output
            .market
            .changed_orders_since(
                self.before_last_price,
                self.before_last_close,
                self.before_next_seq,
                self.original_orders,
            )
            .map_err(|error| invariant(&error.to_string()))?;

        Ok(ContinuousStockStepOutput {
            output: self.output,
            execution_facts: self.execution_facts,
            next_trade_event_index: self.next_trade_event_index,
            ledger: self.ledger,
            acceptance_quotes: self.acceptance_quotes,
            market_delta,
            #[cfg(feature = "simulation-diagnostics")]
            operation_quotes: self.operation_quotes,
        })
    }
}

#[cfg(feature = "simulation-diagnostics")]
fn quote_snapshot(market: &Market) -> Result<ContinuousQuoteSnapshot, StepFatal> {
    let bid_depth = market
        .bid_depth()
        .into_iter()
        .try_fold(0_u64, |total, (_, qty)| {
            total
                .checked_add(qty)
                .ok_or_else(|| invariant("continuous bid depth overflow"))
        })?;
    let ask_depth = market
        .ask_depth()
        .into_iter()
        .try_fold(0_u64, |total, (_, qty)| {
            total
                .checked_add(qty)
                .ok_or_else(|| invariant("continuous ask depth overflow"))
        })?;
    Ok(ContinuousQuoteSnapshot {
        bid_cents: market.best_bid().map(|price| price.cents()),
        ask_cents: market.best_ask().map(|price| price.cents()),
        bid_depth,
        ask_depth,
    })
}

#[cfg(feature = "simulation-diagnostics")]
fn insert_operation_quotes(
    quotes: &mut BTreeMap<u64, ContinuousOperationQuotes>,
    sealed_index: u64,
    before: ContinuousQuoteSnapshot,
    after: ContinuousQuoteSnapshot,
) -> Result<(), StepFatal> {
    if quotes
        .insert(sealed_index, ContinuousOperationQuotes { before, after })
        .is_some()
    {
        return Err(invariant("duplicate sealed operation quote"));
    }
    Ok(())
}

#[cfg(feature = "simulation-diagnostics")]
fn insert_unchanged_operation_quote(
    quotes: &mut BTreeMap<u64, ContinuousOperationQuotes>,
    sealed_index: u64,
    snapshot: ContinuousQuoteSnapshot,
) -> Result<(), StepFatal> {
    insert_operation_quotes(quotes, sealed_index, snapshot.clone(), snapshot)
}

pub(super) fn append_trade_facts(
    stock: &StockCode,
    triggering_sealed_index: u64,
    trades: &[Trade],
    next_index: &mut u64,
    output: &mut Vec<ContinuousTradeFact>,
) -> Result<(), StepFatal> {
    let count = u64::try_from(trades.len())
        .map_err(|_| invariant("trade fact count exceeds u64 identity space"))?;
    let next_after = next_index
        .checked_add(count)
        .ok_or_else(|| invariant("stock-local trade event index overflow"))?;
    let mut staged = Vec::with_capacity(trades.len());
    let mut cursor = *next_index;
    for trade in trades {
        staged.push(ContinuousTradeFact {
            stock: stock.clone(),
            triggering_sealed_index,
            stock_local_trade_event_index: cursor,
            trade: trade.clone(),
        });
        cursor = cursor
            .checked_add(1)
            .ok_or_else(|| invariant("stock-local trade event index overflow"))?;
    }
    if cursor != next_after {
        return Err(invariant("stock-local trade event cursor drift"));
    }
    output.extend(staged);
    *next_index = next_after;
    Ok(())
}

fn validate_account_fact_identities(
    places: &[ContinuousPlaceFact],
    cancels: &[ContinuousCancelFact],
) -> Result<(), StepFatal> {
    let mut seen = BTreeSet::new();
    for (account, sealed_index) in places
        .iter()
        .map(|fact| match fact {
            ContinuousPlaceFact::Resting {
                account,
                sealed_index,
                ..
            }
            | ContinuousPlaceFact::Filled {
                account,
                sealed_index,
                ..
            }
            | ContinuousPlaceFact::Rejected {
                account,
                sealed_index,
                ..
            } => (*account, *sealed_index),
        })
        .chain(cancels.iter().map(|fact| match fact {
            ContinuousCancelFact::Canceled {
                account,
                sealed_index,
                ..
            }
            | ContinuousCancelFact::Rejected {
                account,
                sealed_index,
                ..
            } => (*account, *sealed_index),
        }))
    {
        if !seen.insert((account, sealed_index)) {
            return Err(invariant(
                "one account operation emitted more than one sealed fact identity",
            ));
        }
    }
    Ok(())
}

pub(in crate::session::pipeline) fn validate_execution_facts(
    facts: &[ContinuousExecutionFact],
) -> Result<(), StepFatal> {
    let mut candidate_keys = BTreeSet::new();
    let mut sealed_indices = BTreeSet::new();
    for fact in facts {
        if !candidate_keys.insert(fact.candidate_key.clone()) {
            return Err(invariant(
                "one candidate emitted more than one continuous execution fact",
            ));
        }
        if !sealed_indices.insert(fact.sealed_index) {
            return Err(invariant(
                "one sealed identity emitted more than one continuous execution fact",
            ));
        }
        match &fact.outcome {
            ContinuousExecutionOutcome::Place { fact: place, .. } => {
                let (sealed_index, order_id) = match place {
                    ContinuousPlaceFact::Resting {
                        sealed_index,
                        order_id,
                        ..
                    }
                    | ContinuousPlaceFact::Filled {
                        sealed_index,
                        order_id,
                        ..
                    }
                    | ContinuousPlaceFact::Rejected {
                        sealed_index,
                        order_id,
                        ..
                    } => (*sealed_index, *order_id),
                };
                if sealed_index != fact.sealed_index || fact.allocated_order_id != Some(order_id) {
                    return Err(invariant(
                        "place execution fact identity disagrees with its typed outcome",
                    ));
                }
            }
            ContinuousExecutionOutcome::Cancel(cancel) => {
                let sealed_index = match cancel {
                    ContinuousCancelFact::Canceled { sealed_index, .. }
                    | ContinuousCancelFact::Rejected { sealed_index, .. } => *sealed_index,
                };
                if sealed_index != fact.sealed_index || fact.allocated_order_id.is_some() {
                    return Err(invariant(
                        "cancel execution fact identity disagrees with its typed outcome",
                    ));
                }
            }
        }
    }
    Ok(())
}

fn validate_operation_identities(operations: &[ValidatedOperation]) -> Result<(), StepFatal> {
    let mut seen_candidates = BTreeSet::new();
    let mut seen_sealed = BTreeSet::new();
    for operation in operations {
        if !seen_candidates.insert(operation.candidate_key())
            || !seen_sealed.insert(operation.sealed_index())
        {
            return Err(invariant(
                "continuous stock operation identity was repeated",
            ));
        }
    }
    Ok(())
}

fn validate_initial_snapshots(
    market: &Market,
    snapshots: &[ContinuousEnvelopeSnapshot],
) -> Result<(), StepFatal> {
    let orders = market.resting_orders();
    let mut by_order = BTreeMap::new();
    for snapshot in snapshots {
        snapshot.envelope.validate()?;
        if snapshot.envelope.origin() != EnvelopeOrigin::TickStart {
            return Err(invariant(
                "post-P0 continuous input contains a same-tick envelope",
            ));
        }
        if snapshot.envelope.key().stock != *market.code() {
            return Err(invariant("continuous envelope belongs to another stock"));
        }
        let key = snapshot.envelope.key().order;
        if by_order.insert(key, snapshot).is_some() {
            return Err(invariant("duplicate continuous envelope snapshot"));
        }
    }
    for order in &orders {
        let snapshot = by_order
            .remove(&order.id)
            .ok_or_else(|| invariant("resting order has no continuous envelope snapshot"))?;
        validate_snapshot(snapshot, order)?;
    }
    if !by_order.is_empty() {
        return Err(invariant(
            "continuous envelope snapshot has no resting order",
        ));
    }
    Ok(())
}

pub(super) fn validate_private_market_ledger(
    market: &Market,
    ledger: &EnvelopeLedger,
    config: &GameConfig,
) -> Result<(), StepFatal> {
    let mut matched = 0_usize;
    for order in market.resting_order_refs() {
        let key = EnvelopeKey {
            account: order.owner,
            stock: market.code().clone(),
            order: order.id,
            side: order.side,
        };
        let envelope = ledger
            .get(&key)
            .map_err(|_| invariant("stock round left a resting order without a live envelope"))?;
        let audit = envelope.audit();
        if envelope.pending_price().is_some()
            || audit.limit != order.price
            || audit.remaining_qty != order.qty
            || audit.filled_qty != order.filled_qty
            || audit.filled_value != order.filled_value
        {
            return Err(invariant(
                "stock round left an order and its live envelope out of sync",
            ));
        }
        match order.side {
            Side::Buy => {
                let expected = crate::session::buy_order_reservation(
                    config,
                    order.price,
                    order.qty,
                    order.filled_value,
                )
                .map_err(|error| invariant(&error.to_string()))?;
                if envelope.live() != ResVec::new(expected, 0) {
                    return Err(invariant(
                        "resting buy envelope disagrees with its exact reservation",
                    ));
                }
            }
            Side::Sell if envelope.live() != ResVec::new(Money::ZERO, order.qty) => {
                return Err(invariant(
                    "resting sell envelope disagrees with its quantity",
                ));
            }
            Side::Sell => {}
        }
        matched = matched
            .checked_add(1)
            .ok_or_else(|| invariant("stock round live order count overflow"))?;
    }
    if matched != ledger.iter().count() {
        return Err(invariant(
            "stock round left a live envelope without a resting order",
        ));
    }
    Ok(())
}

fn place_phase_rejection(
    phase: TradingPhase,
    kind: PlaceKind,
) -> Result<Option<RejectionReason>, StepFatal> {
    match phase {
        TradingPhase::Continuous => Ok(None),
        TradingPhase::PreOpen => Ok(Some(RejectionReason::AuctionOrderEntryClosed)),
        TradingPhase::CallAuction | TradingPhase::ClosingAuction if kind == PlaceKind::Market => {
            Ok(Some(RejectionReason::AuctionLimitOrderRequired))
        }
        TradingPhase::CallAuction | TradingPhase::ClosingAuction => Err(invariant(
            "auction limit order was routed to the continuous stock worker",
        )),
    }
}

impl ContinuousStockRoundProcessor {
    fn reject_place(
        &mut self,
        draft: &super::EnvelopeDraft,
        reason: RejectionReason,
    ) -> Result<(), StepFatal> {
        let envelope = self.ledger.get(draft.key())?.clone();
        let ordinal =
            u64::from(draft.requested_price().is_some() && envelope.pending_price().is_none());
        let receipt = terminal_receipt(
            draft.sealed_index(),
            &envelope,
            ReceiptKind::Reject,
            ordinal,
        )?;
        let terminal = draft.key().clone();
        self.apply_receipts(
            std::slice::from_ref(&receipt),
            std::slice::from_ref(&terminal),
        )?;
        self.output.receipts.push(receipt);
        self.output.terminal_keys.push(terminal);
        self.output.place_facts.push(ContinuousPlaceFact::Rejected {
            sealed_index: draft.sealed_index(),
            account: draft.owner(),
            code: draft.code().clone(),
            order_id: draft.order_id(),
            reason,
        });
        self.record_place_fact(draft)?;
        Ok(())
    }

    fn record_place_fact(&mut self, draft: &super::EnvelopeDraft) -> Result<(), StepFatal> {
        let fact = self
            .output
            .place_facts
            .last()
            .ok_or_else(|| invariant("place operation produced no typed place fact"))?
            .clone();
        self.execution_facts.push(ContinuousExecutionFact {
            candidate_key: draft.candidate_key().clone(),
            sealed_index: draft.sealed_index(),
            allocated_order_id: Some(draft.order_id()),
            outcome: ContinuousExecutionOutcome::Place {
                fact,
                original_qty: draft.qty(),
            },
        });
        Ok(())
    }

    fn apply_receipts(
        &mut self,
        receipts: &[EnvelopeReceipt],
        terminal_keys: &[EnvelopeKey],
    ) -> Result<(), StepFatal> {
        let mut validation = receipts.to_vec();
        self.ledger
            .apply_private_for_stock_round(&mut validation, terminal_keys)
    }

    fn fill_receipts(
        &self,
        draft: &super::EnvelopeDraft,
        trades: &[Trade],
        resolution: Option<&EnvelopeReceipt>,
    ) -> Result<FillReceiptProjection, StepFatal> {
        ContinuousFillReceiptProjection::capture(draft, &self.ledger, &self.config, resolution)?
            .run(trades)
    }
}

type FillReceiptProjection = (
    Vec<EnvelopeReceipt>,
    BTreeMap<EnvelopeKey, Envelope>,
    BTreeMap<EnvelopeKey, u64>,
);

#[cfg(test)]
pub(super) fn fill_receipts(
    draft: &super::EnvelopeDraft,
    trades: &[Trade],
    ledger: &EnvelopeLedger,
    config: &GameConfig,
    resolution: Option<&EnvelopeReceipt>,
) -> Result<FillReceiptProjection, StepFatal> {
    ContinuousFillReceiptProjection::capture(draft, ledger, config, resolution)?.run(trades)
}

/// 单 Place 的收据和审计副本；成功后仍交由 round ledger 校验与采纳。
struct ContinuousFillReceiptProjection<'a> {
    draft: &'a super::EnvelopeDraft,
    ledger: &'a EnvelopeLedger,
    config: &'a GameConfig,
    states: BTreeMap<EnvelopeKey, Envelope>,
    ordinals: BTreeMap<EnvelopeKey, u64>,
    receipts: Vec<EnvelopeReceipt>,
}

impl<'a> ContinuousFillReceiptProjection<'a> {
    fn capture(
        draft: &'a super::EnvelopeDraft,
        ledger: &'a EnvelopeLedger,
        config: &'a GameConfig,
        resolution: Option<&EnvelopeReceipt>,
    ) -> Result<Self, StepFatal> {
        // 市价单释放余量需要 incoming 的成交后状态；仅加载实际成交的 maker。
        let mut incoming = ledger.get(draft.key())?.clone();
        if let Some(receipt) = resolution {
            let audit_after = super::ledger_validation::next_audit(receipt, incoming.audit())?;
            incoming.apply(receipt.delta, audit_after, true)?;
        }
        let states = BTreeMap::from([(draft.key().clone(), incoming)]);
        let mut ordinals = BTreeMap::new();
        if draft.requested_price().is_some() {
            ordinals.insert(draft.key().clone(), 1);
        }

        Ok(Self {
            draft,
            ledger,
            config,
            states,
            ordinals,
            receipts: Vec::new(),
        })
    }

    fn run(mut self, trades: &[Trade]) -> Result<FillReceiptProjection, StepFatal> {
        self.receipts.reserve(trades.len().saturating_mul(2));
        for trade in trades {
            let gross = trade
                .price
                .mul_shares(trade.qty)
                .map_err(|error| invariant(&error.to_string()))?;
            let (buyer, buyer_before, seller, seller_before) = match self.draft.side() {
                Side::Buy => (
                    self.draft.key().clone(),
                    trade.taker_filled_value_before,
                    EnvelopeKey {
                        account: trade.maker,
                        stock: self.draft.code().clone(),
                        order: trade.maker_order_id,
                        side: Side::Sell,
                    },
                    trade.maker_filled_value_before,
                ),
                Side::Sell => (
                    EnvelopeKey {
                        account: trade.maker,
                        stock: self.draft.code().clone(),
                        order: trade.maker_order_id,
                        side: Side::Buy,
                    },
                    trade.maker_filled_value_before,
                    self.draft.key().clone(),
                    trade.taker_filled_value_before,
                ),
            };
            let receipt = self.record_fill(buyer, buyer_before, trade.qty, gross)?;
            self.receipts.push(receipt);
            let receipt = self.record_fill(seller, seller_before, trade.qty, gross)?;
            self.receipts.push(receipt);
        }

        Ok((self.receipts, self.states, self.ordinals))
    }

    fn record_fill(
        &mut self,
        key: EnvelopeKey,
        filled_value_before: Money,
        fill_qty: u32,
        gross: Money,
    ) -> Result<EnvelopeReceipt, StepFatal> {
        if !self.states.contains_key(&key) {
            self.states
                .insert(key.clone(), self.ledger.get(&key)?.clone());
        }
        let envelope = self
            .states
            .get_mut(&key)
            .ok_or_else(|| invariant("trade participant has no live envelope"))?;
        let audit = envelope.audit();
        if audit.filled_value != filled_value_before {
            return Err(invariant(
                "trade filled-value history disagrees with envelope audit",
            ));
        }
        let remaining_qty_after = audit
            .remaining_qty
            .checked_sub(fill_qty)
            .ok_or_else(|| invariant("trade fill exceeds envelope remaining quantity"))?;
        let value_after = audit
            .filled_value
            .add(gross)
            .map_err(|error| invariant(&error.to_string()))?;
        let transition = match key.side {
            Side::Buy => FillTransition::buy(BuyFillInput {
                config: self.config,
                limit: audit.limit,
                fill_qty,
                remaining_qty_after,
                filled_value_before: audit.filled_value,
                gross_delta: gross,
                live_before: envelope.live(),
            })?,
            Side::Sell => FillTransition::sell(SellFillInput {
                config: self.config,
                fill_qty,
                remaining_qty_after,
                filled_value_before: audit.filled_value,
                gross_delta: gross,
                nominal_before: audit.nominal,
                charged_before: audit.charged,
            })?,
        };
        let ordinal = take_ordinal(&mut self.ordinals, &key)?;
        let receipt = EnvelopeReceipt {
            index: 0,
            local_key: ReceiptLocalKey::new(
                JournalRank::SealedBatch,
                ReceiptSource::SealedIntent(self.draft.sealed_index()),
                ReceiptTransition {
                    envelope: key.clone(),
                    ordinal,
                },
            )?,
            envelope: key,
            kind: ReceiptKind::Fill,
            qty_before: audit.remaining_qty,
            qty_after: remaining_qty_after,
            value_before: audit.filled_value,
            value_after,
            delta: transition.delta,
            nominal: transition.nominal,
            charged: transition.charged,
            charged_before: audit.charged,
            charged_after: transition.charged_after,
            deliver_qty: transition.deliver_qty,
            deliver_cash: transition.deliver_cash,
        };
        let filled_qty = audit
            .filled_qty
            .checked_add(fill_qty)
            .ok_or_else(|| invariant("envelope cumulative filled quantity overflow"))?;
        envelope.apply(
            transition.delta,
            EnvelopeAudit {
                limit: audit.limit,
                remaining_qty: remaining_qty_after,
                filled_qty,
                filled_value: value_after,
                nominal: transition.nominal_after,
                charged: transition.charged_after,
            },
            false,
        )?;
        Ok(receipt)
    }
}

fn take_ordinal(
    ordinals: &mut BTreeMap<EnvelopeKey, u64>,
    key: &EnvelopeKey,
) -> Result<u64, StepFatal> {
    let ordinal = ordinals.entry(key.clone()).or_insert(0);
    let current = *ordinal;
    *ordinal = ordinal
        .checked_add(1)
        .ok_or_else(|| invariant("receipt transition ordinal overflow"))?;
    Ok(current)
}

fn terminal_fill_keys(receipts: &[EnvelopeReceipt]) -> BTreeSet<EnvelopeKey> {
    receipts
        .iter()
        .filter(|receipt| receipt.kind == ReceiptKind::Fill && receipt.qty_after == 0)
        .map(|receipt| receipt.envelope.clone())
        .collect()
}

fn terminal_receipt(
    sealed_index: u64,
    envelope: &Envelope,
    kind: ReceiptKind,
    ordinal: u64,
) -> Result<EnvelopeReceipt, StepFatal> {
    let audit = envelope.audit();
    Ok(EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(sealed_index),
            ReceiptTransition {
                envelope: envelope.key().clone(),
                ordinal,
            },
        )?,
        envelope: envelope.key().clone(),
        kind,
        qty_before: audit.remaining_qty,
        qty_after: audit.remaining_qty,
        value_before: audit.filled_value,
        value_after: audit.filled_value,
        delta: ReceiptDelta::sealed(ResVec::ZERO, envelope.live(), ResVec::ZERO),
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: audit.charged,
        charged_after: audit.charged,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    })
}

#[cfg(test)]
pub(super) fn cancel_continuous_order(
    input: ContinuousCancelInput,
) -> Result<ContinuousCancelOutput, StepFatal> {
    let ContinuousCancelInput {
        mut market,
        envelopes,
        operation,
    } = input;
    let effect = cancel_continuous_order_in_place(&mut market, operation, |operation, _| {
        let mut matching = envelopes.iter().filter(|snapshot| {
            snapshot.envelope.key().stock == operation.code
                && snapshot.envelope.key().order == operation.order_id
        });
        let snapshot = matching
            .next()
            .ok_or_else(|| invariant("canceled order has no live envelope"))?;
        if matching.next().is_some() {
            return Err(invariant("canceled order has duplicate live envelopes"));
        }
        Ok(snapshot.clone())
    })?;
    Ok(ContinuousCancelOutput {
        market,
        receipt: effect.receipt,
        terminal_key: effect.terminal_key,
        fact: effect.fact,
    })
}

fn cancel_continuous_order_from_private_ledger(
    market: &mut Market,
    ledger: &EnvelopeLedger,
    operation: ContinuousCancelOperation,
) -> Result<ContinuousCancelEffect, StepFatal> {
    cancel_continuous_order_in_place(market, operation, |operation, order| {
        let key = EnvelopeKey {
            account: order.owner,
            stock: operation.code.clone(),
            order: order.id,
            side: order.side,
        };
        let envelope = ledger
            .get(&key)
            .map_err(|_| invariant("canceled order has no live envelope"))?;
        Ok(ContinuousEnvelopeSnapshot {
            audit: envelope.audit(),
            envelope: envelope.clone(),
        })
    })
}

fn cancel_continuous_order_in_place(
    market: &mut Market,
    operation: ContinuousCancelOperation,
    snapshot_for: impl FnOnce(
        &ContinuousCancelOperation,
        &crate::Order,
    ) -> Result<ContinuousEnvelopeSnapshot, StepFatal>,
) -> Result<ContinuousCancelEffect, StepFatal> {
    if market.code() != &operation.code {
        return Ok(rejected_effect(
            operation,
            ContinuousCancelRejection::UnknownStock,
        ));
    }
    let Some(order) = market
        .resting_order_refs()
        .find(|order| order.id == operation.order_id)
        .cloned()
    else {
        let reason = match market.filled_order_owner(operation.order_id) {
            Some(owner) if owner == operation.account => {
                ContinuousCancelRejection::OrderAlreadyFilled
            }
            Some(_) => ContinuousCancelRejection::NotOrderOwner,
            None => ContinuousCancelRejection::OrderNotFound,
        };
        return Ok(rejected_effect(operation, reason));
    };
    if order.owner != operation.account {
        return Ok(rejected_effect(
            operation,
            ContinuousCancelRejection::NotOrderOwner,
        ));
    }

    let snapshot = snapshot_for(&operation, &order)?;
    validate_snapshot(&snapshot, &order)?;
    let receipt = release_receipt(&operation, &snapshot)?;
    let canceled = market
        .cancel(operation.order_id)
        .map_err(|error| invariant(&error.to_string()))?;
    validate_snapshot(&snapshot, &canceled)?;
    Ok(ContinuousCancelEffect {
        receipt: Some(receipt),
        terminal_key: Some(snapshot.envelope.key().clone()),
        fact: ContinuousCancelFact::Canceled {
            sealed_index: operation.sealed_index,
            account: operation.account,
            code: operation.code,
            order_id: operation.order_id,
            side: canceled.side,
            remaining_qty: canceled.qty,
        },
    })
}

fn release_receipt(
    operation: &ContinuousCancelOperation,
    snapshot: &ContinuousEnvelopeSnapshot,
) -> Result<EnvelopeReceipt, StepFatal> {
    let key = snapshot.envelope.key().clone();
    Ok(EnvelopeReceipt {
        index: 0,
        local_key: ReceiptLocalKey::new(
            JournalRank::SealedBatch,
            ReceiptSource::SealedIntent(operation.sealed_index),
            ReceiptTransition {
                envelope: key.clone(),
                ordinal: 0,
            },
        )?,
        envelope: key,
        kind: ReceiptKind::Release,
        qty_before: snapshot.audit.remaining_qty,
        qty_after: snapshot.audit.remaining_qty,
        value_before: snapshot.audit.filled_value,
        value_after: snapshot.audit.filled_value,
        delta: ReceiptDelta::sealed(ResVec::ZERO, snapshot.envelope.live(), ResVec::ZERO),
        nominal: FeeComponents::ZERO,
        charged: FeeComponents::ZERO,
        charged_before: snapshot.audit.charged,
        charged_after: snapshot.audit.charged,
        deliver_qty: 0,
        deliver_cash: Money::ZERO,
    })
}

fn validate_snapshot(
    snapshot: &ContinuousEnvelopeSnapshot,
    order: &crate::Order,
) -> Result<(), StepFatal> {
    let key = snapshot.envelope.key();
    if snapshot.envelope.audit() != snapshot.audit {
        return Err(invariant("live envelope and supplied audit disagree"));
    }
    if key.account != order.owner
        || key.order != order.id
        || key.side != order.side
        || snapshot.audit.limit != order.price
        || snapshot.audit.remaining_qty != order.qty
        || snapshot.audit.filled_qty != order.filled_qty
        || snapshot.audit.filled_value != order.filled_value
    {
        return Err(invariant(
            "live envelope audit disagrees with canceled order",
        ));
    }
    Ok(())
}

fn rejected_effect(
    operation: ContinuousCancelOperation,
    reason: ContinuousCancelRejection,
) -> ContinuousCancelEffect {
    ContinuousCancelEffect {
        receipt: None,
        terminal_key: None,
        fact: ContinuousCancelFact::Rejected {
            sealed_index: operation.sealed_index,
            account: operation.account,
            code: operation.code,
            order_id: operation.order_id,
            reason,
        },
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::continuous_matching".to_owned(),
    }
}

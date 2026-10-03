//! 跨 adaptive route 保持的逐股 Continuous P4 shadow。
//!
//! coordinator 从 post-P0 candidate 初始化一次，逐个应用 P3 Accepted 操作。
//! 全部 route 排空后 consuming finish，ReceiptAggregation/Settlement/Projection 仅消费一次累计 outbox。

#[cfg(test)]
use super::ContinuousEnvelopeSnapshot;
#[cfg(feature = "simulation-diagnostics")]
use super::ContinuousOperationQuotes;
use super::{
    process_continuous_stock_step, process_continuous_stock_step_with_ledger,
    validate_private_market_ledger, ContinuousAcceptanceQuote, ContinuousCancelFact,
    ContinuousCancelRejection, ContinuousExecutionFact, ContinuousExecutionOutcome,
    ContinuousPlaceFact, ContinuousStockInput, ContinuousStockOutput, ContinuousTradeFact,
};
use crate::market::MarketDelta;
use crate::session::pipeline::{
    EnvelopeKey, EnvelopeLedger, EnvelopeReceipt, IntentCandidateKey, StepFatal, ValidatedOperation,
};
use crate::{GameConfig, Market, Money, StockCode, TradingPhase};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug)]
pub(in crate::session::pipeline) struct ContinuousStockProjection {
    /// 仅将变更订单应用到 tick candidate；worker 保留完整簿供后续轮次与最终 Settlement。
    pub(in crate::session::pipeline) market_delta: Option<MarketDelta>,
    pub(in crate::session::pipeline) acceptance_quotes: BTreeMap<u64, ContinuousAcceptanceQuote>,
}

/// 本次 `apply_round` 新产生的事实。
#[derive(Debug)]
pub(in crate::session::pipeline) struct ContinuousExecutionRound {
    pub(in crate::session::pipeline) facts: Vec<ContinuousExecutionFact>,
    pub(in crate::session::pipeline) receipts: Vec<EnvelopeReceipt>,
    #[cfg(any(test, feature = "simulation-diagnostics"))]
    pub(in crate::session::pipeline) trades: Vec<ContinuousTradeFact>,
    pub(in crate::session::pipeline) projections: BTreeMap<StockCode, ContinuousStockProjection>,
    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session::pipeline) operation_quotes: BTreeMap<u64, ContinuousOperationQuotes>,
}

/// 调用方排空全部 adaptive route 后的累计 P4 输出。
pub(in crate::session::pipeline) struct IncrementalContinuousStockFinish {
    pub(in crate::session::pipeline) workers: Vec<ContinuousStockOutput>,
    pub(in crate::session::pipeline) prices: BTreeMap<StockCode, ContinuousClosingPrice>,
    pub(in crate::session::pipeline) execution_facts: Vec<ContinuousExecutionFact>,
    /// 无对应股票 worker 的 P4 业务拒绝；当前仅包括 P3 接受的未知股票撤单。
    pub(in crate::session::pipeline) detached_facts: Vec<ContinuousExecutionFact>,
}

/// 可选 DayEnd 清簿前冻结最终行情与深度。
pub(in crate::session::pipeline) struct ContinuousClosingPrice {
    pub(in crate::session::pipeline) last: Money,
    pub(in crate::session::pipeline) bids: Vec<(Money, u64)>,
    pub(in crate::session::pipeline) asks: Vec<(Money, u64)>,
}

#[derive(Debug)]
pub(in crate::session::pipeline) struct IncrementalContinuousStockCoordinator {
    phase: Option<TradingPhase>,
    stocks: BTreeMap<StockCode, IncrementalContinuousStockShadow>,
    detached_facts: Vec<ContinuousExecutionFact>,
    seen_candidate_keys: BTreeSet<IntentCandidateKey>,
    seen_sealed_indices: BTreeSet<u64>,
    applied_operation_count: usize,
    failed: bool,
}

#[derive(Clone, Debug)]
struct IncrementalContinuousStockShadow {
    phase: TradingPhase,
    market: Market,
    ledger: EnvelopeLedger,
    config: GameConfig,
    next_trade_event_index: u64,
    created_envelopes: Vec<crate::session::pipeline::Envelope>,
    receipts: Vec<EnvelopeReceipt>,
    terminal_keys: Vec<EnvelopeKey>,
    trades: Vec<ContinuousTradeFact>,
    place_facts: Vec<ContinuousPlaceFact>,
    cancel_facts: Vec<ContinuousCancelFact>,
    execution_facts: Vec<ContinuousExecutionFact>,
}

struct StockRoundResult {
    code: StockCode,
    shadow: IncrementalContinuousStockShadow,
    facts: Vec<ContinuousExecutionFact>,
    receipts: Vec<EnvelopeReceipt>,
    #[cfg(any(test, feature = "simulation-diagnostics"))]
    trades: Vec<ContinuousTradeFact>,
    acceptance_quotes: BTreeMap<u64, ContinuousAcceptanceQuote>,
    market_delta: MarketDelta,
    #[cfg(feature = "simulation-diagnostics")]
    operation_quotes: BTreeMap<u64, ContinuousOperationQuotes>,
}

impl IncrementalContinuousStockCoordinator {
    pub(in crate::session::pipeline) fn detached(phase: TradingPhase) -> Self {
        Self {
            phase: Some(phase),
            stocks: BTreeMap::new(),
            detached_facts: Vec::new(),
            seen_candidate_keys: BTreeSet::new(),
            seen_sealed_indices: BTreeSet::new(),
            applied_operation_count: 0,
            failed: false,
        }
    }

    pub(in crate::session::pipeline) fn from_post_expiry(
        inputs: Vec<ContinuousStockInput>,
    ) -> Result<Self, StepFatal> {
        let phase = inputs.first().map(|input| input.phase);
        if inputs.iter().any(|input| Some(input.phase) != phase) {
            return Err(invariant(
                "incremental P4 stock shadows disagree on the trading phase",
            ));
        }
        let mut stocks = BTreeMap::new();
        for input in inputs {
            if !input.operations.is_empty() {
                return Err(invariant(
                    "incremental P4 initialization included sealed operations",
                ));
            }
            let code = input.market.code().clone();
            if stocks.contains_key(&code) {
                return Err(invariant(
                    "incremental P4 initialized one stock shadow more than once",
                ));
            }
            let phase = input.phase;
            let config = input.config.clone();
            let initialized = process_continuous_stock_step(input, 0)?;
            if !initialized.execution_facts.is_empty()
                || !initialized.output.receipts.is_empty()
                || !initialized.output.created_envelopes.is_empty()
            {
                return Err(invariant(
                    "operation-free P4 initialization produced an outbox",
                ));
            }
            stocks.insert(
                code,
                IncrementalContinuousStockShadow {
                    phase,
                    market: initialized.output.market,
                    ledger: initialized.ledger,
                    config,
                    next_trade_event_index: initialized.next_trade_event_index,
                    created_envelopes: Vec::new(),
                    receipts: Vec::new(),
                    terminal_keys: Vec::new(),
                    trades: Vec::new(),
                    place_facts: Vec::new(),
                    cancel_facts: Vec::new(),
                    execution_facts: Vec::new(),
                },
            );
        }
        Ok(Self {
            phase,
            stocks,
            detached_facts: Vec::new(),
            seen_candidate_keys: BTreeSet::new(),
            seen_sealed_indices: BTreeSet::new(),
            applied_operation_count: 0,
            failed: false,
        })
    }

    /// 应用一轮 ready route；typed 错误使 coordinator 失效，由调用方丢弃整个 tick candidate。
    /// 不为轮次重试复制订单簿，以免重复权威回滚边界。
    pub(in crate::session::pipeline) fn apply_round(
        &mut self,
        operations: Vec<ValidatedOperation>,
    ) -> Result<ContinuousExecutionRound, StepFatal> {
        if self.failed {
            return Err(invariant(
                "failed P4 coordinator cannot accept another round",
            ));
        }
        let result = self.apply_round_owned(operations);
        if result.is_err() {
            self.failed = true;
        }
        result
    }

    fn apply_round_owned(
        &mut self,
        operations: Vec<ValidatedOperation>,
    ) -> Result<ContinuousExecutionRound, StepFatal> {
        validate_new_operation_identities(self, &operations)?;
        let next_operation_count = self
            .applied_operation_count
            .checked_add(operations.len())
            .ok_or_else(|| invariant("incremental P4 operation count overflow"))?;

        let mut grouped = BTreeMap::<StockCode, Vec<ValidatedOperation>>::new();
        let mut detached = Vec::new();
        let mut new_candidate_keys = BTreeSet::new();
        let mut new_sealed_indices = BTreeSet::new();
        for operation in operations {
            let candidate_key = operation.candidate_key().clone();
            let sealed_index = operation.sealed_index();
            new_candidate_keys.insert(candidate_key.clone());
            new_sealed_indices.insert(sealed_index);
            let code = operation_code(&operation).clone();
            if self.stocks.contains_key(&code) {
                grouped.entry(code).or_default().push(operation);
                continue;
            }
            match operation {
                ValidatedOperation::Cancel {
                    candidate_key,
                    sealed_index,
                    account,
                    code,
                    order_id,
                } => detached.push(ContinuousExecutionFact {
                    candidate_key,
                    sealed_index,
                    allocated_order_id: None,
                    outcome: ContinuousExecutionOutcome::Cancel(ContinuousCancelFact::Rejected {
                        sealed_index,
                        account,
                        code,
                        order_id,
                        reason: if self.phase == Some(TradingPhase::PreOpen) {
                            ContinuousCancelRejection::AuctionOrderNotCancelable
                        } else {
                            ContinuousCancelRejection::UnknownStock
                        },
                    }),
                }),
                ValidatedOperation::Place(_) => {
                    return Err(invariant("P3 accepted a place for an unknown stock"));
                }
            }
        }

        let work = grouped
            .into_iter()
            .map(|(code, operations)| {
                let shadow = self
                    .stocks
                    .remove(&code)
                    .ok_or_else(|| invariant("P4 stock partition lost its initialized shadow"))?;
                Ok((code, shadow, operations))
            })
            .collect::<Result<Vec<_>, StepFatal>>()?;
        #[cfg(any(test, feature = "verification-harness"))]
        let work = {
            let mut work = work;
            crate::session::pipeline::executor_perturbation::reorder(
                crate::session::pipeline::ExecutorBoundary::ContinuousStockShards,
                &mut work,
                |(code, _, operations)| (code.0.clone(), operations.len()),
            );
            work
        };
        let results = work
            .into_par_iter()
            .map(|(code, shadow, operations)| {
                let identity = code.clone();
                let result = shadow.apply_round(code, operations);
                (identity, result)
            })
            .collect::<Vec<_>>();
        #[cfg(any(test, feature = "verification-harness"))]
        let results = {
            let mut results = results;
            crate::session::pipeline::executor_perturbation::reorder(
                crate::session::pipeline::ExecutorBoundary::ContinuousWorkerResults,
                &mut results,
                |(code, result)| {
                    (
                        code.0.clone(),
                        result.as_ref().map_or(0, |result| result.facts.len()),
                    )
                },
            );
            results
        };

        // 完成结果可能乱序交付；按股票身份选择首错并归并成功输出，不赋予交易优先级。
        let mut results = results;
        results.sort_by(|left, right| left.0.cmp(&right.0));
        let mut facts = detached.clone();
        let mut receipts = Vec::new();
        #[cfg(any(test, feature = "simulation-diagnostics"))]
        let mut trades = Vec::new();
        let mut projections = BTreeMap::new();
        let mut stock_updates = Vec::with_capacity(results.len());
        #[cfg(feature = "simulation-diagnostics")]
        let mut operation_quotes = BTreeMap::new();
        for (_, result) in results {
            let result = result?;
            facts.extend(result.facts);
            receipts.extend(result.receipts);
            #[cfg(any(test, feature = "simulation-diagnostics"))]
            trades.extend(result.trades);
            #[cfg(feature = "simulation-diagnostics")]
            for (sealed_index, quotes) in result.operation_quotes {
                if operation_quotes.insert(sealed_index, quotes).is_some() {
                    return Err(invariant(
                        "incremental P4 round contains duplicate operation quote identity",
                    ));
                }
            }
            projections.insert(
                result.code.clone(),
                ContinuousStockProjection {
                    market_delta: Some(result.market_delta),
                    acceptance_quotes: result.acceptance_quotes,
                },
            );
            stock_updates.push((result.code, result.shadow));
        }

        validate_round_identities(&facts, &receipts)?;
        self.stocks.extend(stock_updates);
        self.detached_facts.extend(detached);
        self.seen_candidate_keys.extend(new_candidate_keys);
        self.seen_sealed_indices.extend(new_sealed_indices);
        self.applied_operation_count = next_operation_count;
        Ok(ContinuousExecutionRound {
            facts,
            receipts,
            #[cfg(any(test, feature = "simulation-diagnostics"))]
            trades,
            projections,
            #[cfg(feature = "simulation-diagnostics")]
            operation_quotes,
        })
    }

    #[cfg(test)]
    pub(in crate::session::pipeline) fn finish(
        self,
    ) -> Result<IncrementalContinuousStockFinish, StepFatal> {
        self.finish_for_tick(false)
    }

    /// 仅在全部 continuation 排空后进入 consuming 边界。
    pub(in crate::session::pipeline) fn finish_for_tick(
        self,
        ends_day: bool,
    ) -> Result<IncrementalContinuousStockFinish, StepFatal> {
        if self.failed {
            return Err(invariant("failed P4 coordinator cannot finish a tick"));
        }
        let mut workers = Vec::with_capacity(self.stocks.len());
        let mut prices = BTreeMap::new();
        let mut execution_facts = self.detached_facts.clone();
        let mut execution_count = self.detached_facts.len();
        for (_, stock) in self.stocks {
            let code = stock.market.code().clone();
            let (worker, price, mut stock_facts) = stock.finish(ends_day)?;
            prices.insert(code, price);
            execution_count = execution_count
                .checked_add(stock_facts.len())
                .ok_or_else(|| invariant("incremental P4 fact count overflow"))?;
            execution_facts.append(&mut stock_facts);
            workers.push(worker);
        }
        if execution_count != self.applied_operation_count {
            return Err(invariant(
                "incremental P4 did not produce exactly one fact per operation",
            ));
        }
        Ok(IncrementalContinuousStockFinish {
            workers,
            prices,
            execution_facts,
            detached_facts: self.detached_facts,
        })
    }
}

impl IncrementalContinuousStockShadow {
    fn apply_round(
        mut self,
        code: StockCode,
        operations: Vec<ValidatedOperation>,
    ) -> Result<StockRoundResult, StepFatal> {
        let step = process_continuous_stock_step_with_ledger(
            ContinuousStockInput {
                phase: self.phase,
                market: self.market,
                envelopes: Vec::new(),
                operations,
                config: self.config.clone(),
            },
            self.next_trade_event_index,
            self.ledger,
        )?;
        let output = step.output;
        if step.execution_facts.len()
            != output
                .place_facts
                .len()
                .saturating_add(output.cancel_facts.len())
        {
            return Err(invariant(
                "stock round typed-fact count disagrees with operation outcomes",
            ));
        }
        let facts = step.execution_facts.clone();
        let receipts = output.receipts.clone();
        #[cfg(any(test, feature = "simulation-diagnostics"))]
        let trades = output.trades.clone();

        self.market = output.market;
        self.ledger = step.ledger;
        self.next_trade_event_index = step.next_trade_event_index;
        self.created_envelopes.extend(output.created_envelopes);
        self.receipts.extend(output.receipts);
        self.terminal_keys.extend(output.terminal_keys);
        self.trades.extend(output.trades);
        self.place_facts.extend(output.place_facts);
        self.cancel_facts.extend(output.cancel_facts);
        self.execution_facts.extend(step.execution_facts);

        Ok(StockRoundResult {
            code,
            shadow: self,
            facts,
            receipts,
            #[cfg(any(test, feature = "simulation-diagnostics"))]
            trades,
            acceptance_quotes: step.acceptance_quotes,
            market_delta: step.market_delta,
            #[cfg(feature = "simulation-diagnostics")]
            operation_quotes: step.operation_quotes,
        })
    }

    /// 全部 continuation 排空后消费本股；closing price 在 DayEnd 清簿前冻结。
    fn finish(
        mut self,
        ends_day: bool,
    ) -> Result<
        (
            ContinuousStockOutput,
            ContinuousClosingPrice,
            Vec<ContinuousExecutionFact>,
        ),
        StepFatal,
    > {
        if !self.execution_facts.is_empty() {
            validate_private_market_ledger(&self.market, &self.ledger, &self.config)?;
        }
        let price = ContinuousClosingPrice {
            last: self.market.last_price(),
            bids: self.market.bid_depth_limited(5),
            asks: self.market.ask_depth_limited(5),
        };
        if ends_day {
            for (ordinal, (key, envelope)) in self.ledger.iter().enumerate() {
                let source = u32::try_from(ordinal)
                    .map_err(|_| invariant("continuous DayEnd source index overflow"))?;
                self.receipts.push(
                    crate::session::pipeline::stock_auction::day_end_release_receipt(
                        envelope, source,
                    )?,
                );
                self.terminal_keys.push(key.clone());
            }
            self.market.end_of_day();
        }
        Ok((
            ContinuousStockOutput {
                market: self.market,
                created_envelopes: self.created_envelopes,
                receipts: self.receipts,
                terminal_keys: self.terminal_keys,
                trades: self.trades,
                place_facts: self.place_facts,
                cancel_facts: self.cancel_facts,
            },
            price,
            self.execution_facts,
        ))
    }
}

fn validate_new_operation_identities(
    coordinator: &IncrementalContinuousStockCoordinator,
    operations: &[ValidatedOperation],
) -> Result<(), StepFatal> {
    let mut candidates = BTreeSet::new();
    let mut sealed = BTreeSet::new();
    for operation in operations {
        if coordinator
            .seen_candidate_keys
            .contains(operation.candidate_key())
            || !candidates.insert(operation.candidate_key().clone())
        {
            return Err(invariant("incremental P4 replayed a candidate key"));
        }
        if coordinator
            .seen_sealed_indices
            .contains(&operation.sealed_index())
            || !sealed.insert(operation.sealed_index())
        {
            return Err(invariant("incremental P4 replayed a sealed identity"));
        }
    }
    Ok(())
}

fn validate_round_identities(
    facts: &[ContinuousExecutionFact],
    receipts: &[EnvelopeReceipt],
) -> Result<(), StepFatal> {
    // 各股按实际执行顺序发出 facts/receipts；不得按审计身份重排为业务时钟。
    let fact_ids = facts
        .iter()
        .map(|fact| fact.sealed_index)
        .collect::<BTreeSet<_>>();
    if fact_ids.len() != facts.len() {
        return Err(invariant(
            "incremental P4 round contains duplicate typed fact identities",
        ));
    }
    let receipt_ids = receipts
        .iter()
        .map(|receipt| &receipt.local_key)
        .collect::<BTreeSet<_>>();
    if receipt_ids.len() != receipts.len() {
        return Err(invariant(
            "incremental P4 round contains duplicate receipt identities",
        ));
    }
    Ok(())
}

fn operation_code(operation: &ValidatedOperation) -> &StockCode {
    match operation {
        ValidatedOperation::Place(draft) => draft.code(),
        ValidatedOperation::Cancel { code, .. } => code,
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::incremental_continuous_stock_shadow".to_owned(),
    }
}

#[cfg(test)]
#[path = "incremental_continuous_stock_shadow_tests.rs"]
mod tests;

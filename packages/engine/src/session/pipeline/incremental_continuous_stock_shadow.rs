//! 跨 adaptive route 保持的逐股 Continuous stock_processing shadow。
//!
//! coordinator 从 post-ExpiryShadow candidate 初始化一次，逐个应用 AccountValidation Accepted 操作。
//! 全部 route 排空后 consuming finish，ReceiptAggregation/Settlement/Projection 仅消费一次累计 outbox。

#[cfg(test)]
use super::ContinuousEnvelopeSnapshot;
#[cfg(feature = "simulation-diagnostics")]
use super::ContinuousOperationQuotes;
use super::{
    process_continuous_stock_step, process_continuous_stock_step_with_ledger,
    validate_private_market_ledger, ContinuousAcceptanceQuote, ContinuousCancelFact,
    ContinuousCancelRejection, ContinuousExecutionFact, ContinuousExecutionOutcome,
    ContinuousMarketData, ContinuousPlaceFact, ContinuousStockInput, ContinuousStockOutput,
    ContinuousTradeFact,
};
use crate::compute::parallel::*;
use crate::market::MarketDelta;
use crate::session::pipeline::event_collection::EventFact;
use crate::session::pipeline::private_stock_ledger::PrivateStockLedger;
use crate::session::pipeline::{
    EnvelopeKey, EnvelopeReceipt, EventStableKey, IntentCandidateKey, StepFatal,
    ValidatedOperation,
};
use crate::{DailyCandle, Event, GameConfig, Market, MarketMinuteClose, Money, StockId, TradingPhase};
use std::collections::{BTreeMap, BTreeSet, VecDeque};
#[cfg(test)]
use std::sync::atomic::AtomicUsize;
#[cfg(test)]
use std::sync::atomic::Ordering;

/// 测试专用：统计 from_post_expiry 真实执行次数，用于验证初始化时机，不进入生产逻辑。
#[cfg(test)]
pub(in crate::session::pipeline) static FROM_POST_EXPIRY_CALLS: AtomicUsize = AtomicUsize::new(0);

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
    pub(in crate::session::pipeline) projections: BTreeMap<StockId, ContinuousStockProjection>,
    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session::pipeline) operation_quotes: BTreeMap<u64, ContinuousOperationQuotes>,
}

/// 调用方排空全部 adaptive route 后的累计 stock_processing 输出。
pub(in crate::session::pipeline) struct IncrementalContinuousStockFinish {
    pub(in crate::session::pipeline) workers: Vec<ContinuousStockOutput>,
    pub(in crate::session::pipeline) prices: BTreeMap<StockId, ContinuousClosingPrice>,
    pub(in crate::session::pipeline) execution_facts: Vec<ContinuousExecutionFact>,
    /// 无对应股票 worker 的 stock_processing 业务拒绝；当前仅包括 AccountValidation 接受的未知股票撤单。
    pub(in crate::session::pipeline) detached_facts: Vec<ContinuousExecutionFact>,
    pub(in crate::session::pipeline) auctions:
        BTreeMap<StockId, crate::session::pipeline::book_auction_boundary::BookAuctionSummary>,
}

/// 可选 DayEnd 清簿前冻结最终行情与深度。
pub(in crate::session::pipeline) struct ContinuousClosingPrice {
    pub(in crate::session::pipeline) last: Money,
    pub(in crate::session::pipeline) bids: Vec<(Money, u64)>,
    pub(in crate::session::pipeline) asks: Vec<(Money, u64)>,
}

/// T4（Astra T4 节点审·必修 2）：股票收尾错误的**分类判别载体**——行情面
/// 错误（迁移前位置=`finalize_continuous_tick` 的 `finalize_error` 包装）保持
/// **Finalization** 分类；其余股票收尾错误分类不变（Preparation）。上游
/// `finish_market_execution` 按 variant 归类，不得一律改类。
#[derive(Debug)]
pub(in crate::session::pipeline) enum StockFinishFatal {
    /// 行情面（candle/历史/分钟/行情事件/PreOpen 边界）——Finalization。
    MarketData(StepFatal),
    /// 其余股票收尾错误（coordinator 状态/账本校验/竞价清算）——Preparation。
    Other(StepFatal),
}

impl StockFinishFatal {
    /// 归并为其承载的 fatal（分类判定已在上游完成）。
    pub(in crate::session::pipeline) fn into_fatal(self) -> StepFatal {
        match self {
            Self::MarketData(fatal) | Self::Other(fatal) => fatal,
        }
    }
}

/// T4（ADR-0042 §6-T4「行情」面）：本股 owner 的行情**先验状态**——由 prepare
/// 侧从只读 view 逐股构建、经 `continuous_shards` 注入；owner 连续持有并在
/// 收尾一次产出 [`ContinuousMarketData`]。`None` 先验=会话侧缺该股票条目
/// （收尾需要时按原错误文案显式失败，不以默认值掩盖）。
#[derive(Clone, Debug, Default)]
pub(in crate::session::pipeline) struct StockMarketDataPrior {
    pub(in crate::session::pipeline) candle: Option<DailyCandle>,
    pub(in crate::session::pipeline) price_history: Option<VecDeque<Money>>,
    pub(in crate::session::pipeline) minute_closes: Option<Vec<MarketMinuteClose>>,
    /// 活动日 K 的日期时间锚（=capture 时 civil date；与旧
    /// `update_active_daily_candle` 的 `candle_date_time(current_date())` 同源）。
    pub(in crate::session::pipeline) candle_time: i64,
    pub(in crate::session::pipeline) history_len: usize,
    pub(in crate::session::pipeline) day: u64,
}

#[derive(Clone, Debug)]
/// **执行 owner 声明（ADR-0042 §10）**：本类型=本股行情/撮合/收尾的 owner
/// 载体；生产形态每股恰一个实例、恰一个 shadow（`continuous_shards`）。
/// 同股 continuation 的计算经 `AdaptivePlanChainCoordinator` 驱动，但其
/// 写域已收窄到 `PlanContinuationDomain` 窄视图（增量一百：签名级零
/// `&mut GameSession`，见 §10.2 落地登记）；多股 map/分组/par_iter 包装
/// =该节登记的迁移缝③，与缝①②同批退役，不单飞。
pub(in crate::session::pipeline) struct IncrementalContinuousStockCoordinator {
    phase: Option<TradingPhase>,
    stocks: BTreeMap<StockId, IncrementalContinuousStockShadow>,
    detached_facts: Vec<ContinuousExecutionFact>,
    seen_candidate_keys: BTreeSet<IntentCandidateKey>,
    seen_sealed_indices: BTreeSet<u64>,
    applied_operation_count: usize,
    failed: bool,
}

#[derive(Clone, Debug)]
struct IncrementalContinuousStockShadow {
    policy: crate::session::pipeline::phase::PhasePolicy,
    market: Market,
    ledger: PrivateStockLedger,
    config: GameConfig,
    next_trade_event_index: u64,
    created_envelopes: Vec<crate::session::pipeline::Envelope>,
    receipts: Vec<EnvelopeReceipt>,
    terminal_keys: Vec<EnvelopeKey>,
    trades: Vec<ContinuousTradeFact>,
    place_facts: Vec<ContinuousPlaceFact>,
    cancel_facts: Vec<ContinuousCancelFact>,
    execution_facts: Vec<ContinuousExecutionFact>,
    /// T4：本股行情先验（prepare 注入）；owner 在收尾消费并产出 patch。
    market_prior: StockMarketDataPrior,
}

struct StockRoundResult {
    code: StockId,
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
    pub(in crate::session::pipeline) fn is_detached(&self) -> bool {
        self.stocks.is_empty()
    }
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
        #[cfg(test)]
        FROM_POST_EXPIRY_CALLS.fetch_add(1, Ordering::SeqCst);
        let phase = inputs.first().map(|input| input.policy.phase());
        if inputs
            .iter()
            .any(|input| Some(input.policy.phase()) != phase)
        {
            return Err(invariant(
                "增量 StockProcessing 的股票 shadows 对交易阶段不一致",
            ));
        }
        let mut stocks = BTreeMap::new();
        for input in inputs {
            if !input.operations.is_empty() {
                return Err(invariant(
                    "增量 StockProcessing 初始化时包含了 sealed operations",
                ));
            }
            let code = input.market.code().clone();
            if stocks.contains_key(&code) {
                return Err(invariant("增量 StockProcessing 重复初始化同一股票 shadow"));
            }
            let policy = input.policy;
            let config = input.config.clone();
            let initialized = process_continuous_stock_step(input, 0)?;
            if !initialized.execution_facts.is_empty()
                || !initialized.output.receipts.is_empty()
                || !initialized.output.created_envelopes.is_empty()
            {
                return Err(invariant(
                    "无 operation 的 StockProcessing 初始化产生了 outbox",
                ));
            }
            stocks.insert(
                code,
                IncrementalContinuousStockShadow {
                    policy,
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
                    // T4：行情先验默认空（缺先验时收尾按原文案显式失败）；
                    // 生产路径由 continuous_shards 注入真实先验。
                    market_prior: StockMarketDataPrior::default(),
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
    /// T4：为本 coordinator 持有的**全部**股票 shadow 安装同一先验（生产形态
    /// =每股一个 coordinator、恰一个 shadow；先验错配=结构错误显式失败）。
    pub(in crate::session::pipeline) fn install_market_prior(
        &mut self,
        code: &StockId,
        prior: StockMarketDataPrior,
    ) -> Result<(), StepFatal> {
        let shadow = self
            .stocks
            .get_mut(code)
            .ok_or_else(|| invariant("行情先验指向 coordinator 未持有的股票"))?;
        shadow.market_prior = prior;
        Ok(())
    }

    pub(in crate::session::pipeline) fn apply_round(
        &mut self,
        operations: Vec<ValidatedOperation>,
    ) -> Result<ContinuousExecutionRound, StepFatal> {
        if self.failed {
            return Err(invariant(
                "已失败的 StockProcessing coordinator 不能受理下一轮",
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
            .ok_or_else(|| invariant("增量 StockProcessing operation 数量溢出"))?;

        let mut grouped = BTreeMap::<StockId, Vec<ValidatedOperation>>::new();
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
                    return Err(invariant("AccountValidation 为未知股票受理了 Place"));
                }
            }
        }

        let work = grouped
            .into_iter()
            .map(|(code, operations)| {
                let shadow = self
                    .stocks
                    .remove(&code)
                    .ok_or_else(|| invariant("StockProcessing 股票分片丢失了已初始化 shadow"))?;
                Ok((code, shadow, operations))
            })
            .collect::<Result<Vec<_>, StepFatal>>()?;
        #[cfg(any(test, feature = "verification-harness"))]
        let work = {
            let mut work = work;
            crate::session::pipeline::executor_perturbation::reorder(
                crate::session::pipeline::ExecutorBoundary::ContinuousStockShards,
                &mut work,
                // ExecutorOrderRecord.identities 仍为 Vec<String>；StockId 按数值转字符串。
                |(code, _, operations)| (code.0.to_string(), operations.len()),
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
                        code.0.to_string(),
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
                        "增量 StockProcessing 本轮包含重复 operation quote 身份",
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
    ) -> Result<IncrementalContinuousStockFinish, StockFinishFatal> {
        self.finish_for_tick(false)
    }

    /// 仅在全部 continuation 排空后进入 consuming 边界。
    pub(in crate::session::pipeline) fn finish_for_tick(
        self,
        ends_day: bool,
    ) -> Result<IncrementalContinuousStockFinish, StockFinishFatal> {
        self.finish_owned(ends_day, None, None)
    }

    pub(in crate::session::pipeline) fn finish_for_market_tick(
        self,
        boundary: crate::session::pipeline::phase::MarketTickBoundary,
        exchange: crate::StockExchange,
    ) -> Result<IncrementalContinuousStockFinish, StockFinishFatal> {
        if self.phase != Some(boundary.policy.phase()) && !self.stocks.is_empty() {
            return Err(StockFinishFatal::Other(invariant(
                "stock shard 与市场边界 PhasePolicy 不一致",
            )));
        }
        self.finish_owned(boundary.ends_day, Some(boundary), Some(exchange))
    }

    fn finish_owned(
        self,
        ends_day: bool,
        boundary: Option<crate::session::pipeline::phase::MarketTickBoundary>,
        exchange: Option<crate::StockExchange>,
    ) -> Result<IncrementalContinuousStockFinish, StockFinishFatal> {
        if self.failed {
            return Err(StockFinishFatal::Other(invariant(
                "已失败的 StockProcessing coordinator 不能完成 tick",
            )));
        }
        let mut workers = Vec::with_capacity(self.stocks.len());
        let mut prices = BTreeMap::new();
        let mut auctions = BTreeMap::new();
        let mut execution_facts = self.detached_facts.clone();
        let mut execution_count = self.detached_facts.len();
        for (_, stock) in self.stocks {
            let code = stock.market.code().clone();
            let (worker, price, mut stock_facts, auction) =
                stock.finish(ends_day, boundary, exchange)?;
            if let Some(auction) = auction {
                auctions.insert(code, auction);
            }
            prices.insert(code, price);
            execution_count = execution_count.checked_add(stock_facts.len()).ok_or_else(|| {
                StockFinishFatal::Other(invariant("增量 StockProcessing 事实数量溢出"))
            })?;
            execution_facts.append(&mut stock_facts);
            workers.push(worker);
        }
        if execution_count != self.applied_operation_count {
            return Err(StockFinishFatal::Other(invariant(
                "增量 StockProcessing 未为每个 operation 产生恰好一个事实",
            )));
        }
        Ok(IncrementalContinuousStockFinish {
            workers,
            prices,
            execution_facts,
            detached_facts: self.detached_facts,
            auctions,
        })
    }
}

impl IncrementalContinuousStockShadow {
    fn apply_round(
        mut self,
        code: StockId,
        operations: Vec<ValidatedOperation>,
    ) -> Result<StockRoundResult, StepFatal> {
        let step = process_continuous_stock_step_with_ledger(
            ContinuousStockInput {
                policy: self.policy,
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
        boundary: Option<crate::session::pipeline::phase::MarketTickBoundary>,
        exchange: Option<crate::StockExchange>,
    ) -> Result<
        (
            ContinuousStockOutput,
            ContinuousClosingPrice,
            Vec<ContinuousExecutionFact>,
            Option<crate::session::pipeline::book_auction_boundary::BookAuctionSummary>,
        ),
        StockFinishFatal,
    > {
        let auction = if self.policy.is_auction() {
            let boundary = boundary
                .ok_or_else(|| StockFinishFatal::Other(invariant("Auction stock 缺少合法边界")))?;
            let exchange = exchange
                .ok_or_else(|| StockFinishFatal::Other(invariant("Auction stock 缺少交易所事实")))?;
            let tail = crate::session::pipeline::book_auction_boundary::finish_book_auction(
                &mut self.market,
                &mut self.ledger,
                &self.config,
                exchange,
                boundary.finishes_auction,
                self.policy.phase() == TradingPhase::ClosingAuction,
            )
            .map_err(StockFinishFatal::Other)?;
            self.receipts.extend(tail.receipts);
            self.terminal_keys.extend(tail.terminal_keys);
            for trade in tail.trades {
                let index = self.next_trade_event_index;
                self.next_trade_event_index = index.checked_add(1).ok_or_else(|| {
                    StockFinishFatal::Other(invariant("清算成交事件身份溢出"))
                })?;
                self.trades.push(ContinuousTradeFact {
                    stock: *self.market.code(),
                    triggering_sealed_index: 0,
                    stock_local_trade_event_index: index,
                    trade,
                });
            }
            Some(tail.summary)
        } else {
            None
        };
        if !self.execution_facts.is_empty()
            || auction.as_ref().is_some_and(|summary| summary.completed)
        {
            validate_private_market_ledger(&self.market, &self.ledger, &self.config)
                .map_err(StockFinishFatal::Other)?;
        }
        let price = ContinuousClosingPrice {
            last: self.market.last_price(),
            bids: self.market.bid_depth_limited(5),
            asks: self.market.ask_depth_limited(5),
        };
        // T4（ADR-0042 §6-T4「行情」面）：本股行情由 owner 在此一次计算——
        // 逐笔/收盘日 K 标记、价格历史追加裁剪、分钟收盘回填与行情事件；
        // 收尾层只机械安装（事件按原 finalize 发射序携带）。
        let market_data = self
            .finish_market_data(boundary, &price, auction.as_ref())
            .map_err(StockFinishFatal::MarketData)?;
        if ends_day {
            for (ordinal, (key, envelope)) in self.ledger.iter().enumerate() {
                let source = u32::try_from(ordinal).map_err(|_| {
                    StockFinishFatal::Other(invariant("continuous DayEnd source index overflow"))
                })?;
                self.receipts.push(
                    crate::session::pipeline::order_receipts::day_end_release_receipt(
                        envelope, source,
                    )
                    .map_err(StockFinishFatal::Other)?,
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
                market_data,
            },
            price,
            self.execution_facts,
            auction,
        ))
    }

    /// T4：本股行情收尾计算（原 `finalize_continuous_tick` per-worker 行情段的
    /// 逐字迁移——错误文案/事件形态/发射序不变；location 前缀随属主更名）。
    fn finish_market_data(
        &self,
        boundary: Option<crate::session::pipeline::phase::MarketTickBoundary>,
        price: &ContinuousClosingPrice,
        auction: Option<&crate::session::pipeline::book_auction_boundary::BookAuctionSummary>,
    ) -> Result<ContinuousMarketData, StepFatal> {
        let Some(boundary) = boundary else {
            // finish_for_tick（测试/日终清簿路径）无市场边界：不产出行情 patch。
            return Ok(ContinuousMarketData::default());
        };
        let code = self.market.code().clone();
        let candle_time = self.market_prior.candle_time;
        // 逐笔成交标记先于阶段分支（与原 finalize 顺序一致：先全部成交标记，
        // 再按阶段做收盘标记/历史/分钟/事件）。
        let mut candle = self.market_prior.candle.clone();
        for trade in &self.trades {
            candle = Some(checked_mark_candle(
                candle,
                candle_time,
                trade.trade.price,
                u64::from(trade.trade.qty),
            )?);
        }
        let mut data = ContinuousMarketData {
            candle,
            price_history: None,
            minute_closes: None,
            events: Vec::new(),
        };
        if boundary.policy.phase() == TradingPhase::Continuous {
            let completed_minutes = boundary
                .completed_minutes
                .ok_or_else(|| invariant("连续阶段缺少分钟边界"))?;
            let last = price.last;
            data.candle = Some(checked_mark_candle(data.candle, candle_time, last, 0)?);
            let mut history = self
                .market_prior
                .price_history
                .clone()
                .ok_or_else(|| invariant("continuous market has no price history"))?;
            history.push_back(last);
            while history.len() > self.market_prior.history_len {
                history.pop_front();
            }
            data.price_history = Some(history);
            let mut minutes = self
                .market_prior
                .minute_closes
                .clone()
                .ok_or_else(|| invariant("continuous market has no minute history"))?;
            let recorded = u16::try_from(minutes.len())
                .map_err(|_| invariant("continuous minute history exceeds u16"))?;
            if recorded > completed_minutes {
                return Err(invariant("continuous minute history is ahead of tick"));
            }
            let day_start = self
                .market_prior
                .day
                .checked_mul(u64::from(crate::session::GAME_INTRADAY_MINUTES_PER_DAY))
                .ok_or_else(|| invariant("continuous minute day base overflow"))?;
            for minute in recorded..completed_minutes {
                minutes.push(MarketMinuteClose {
                    absolute_trading_minute: day_start + u64::from(minute),
                    close: last,
                });
            }
            data.minute_closes = Some(minutes);
            data.events.push(owned_market_event(
                Event::PriceTick {
                    seq: 0,
                    tick: boundary.tick_after,
                    code: code.clone(),
                    last_price: last,
                    daily_candle: data.candle.clone().expect("收盘标记后必有活动日 K"),
                    bids: price.bids.clone(),
                    asks: price.asks.clone(),
                },
                0,
            ));
        } else if boundary.policy.is_auction() {
            let auction = auction
                .ok_or_else(|| invariant("竞价 stock 缺少实际边界输出"))?;
            data.events.push(owned_market_event(
                Event::AuctionTick {
                    seq: 0,
                    tick: boundary.tick_after,
                    phase: boundary.policy.phase(),
                    code: code.clone(),
                    indicative_price: auction.indicative_price,
                    matched_volume: auction.matched_volume,
                    imbalance: auction.imbalance,
                },
                0,
            ));
            if auction.completed {
                if self.trades.is_empty() {
                    data.candle = Some(checked_mark_candle(
                        data.candle,
                        candle_time,
                        self.market.last_price(),
                        0,
                    )?);
                }
                data.events.push(owned_market_event(
                    Event::AuctionCompleted {
                        seq: 0,
                        tick: boundary.tick_after,
                        phase: boundary.policy.phase(),
                        code,
                        clearing_price: auction.clearing_price,
                        matched_volume: auction.clearing_volume,
                    },
                    1,
                ));
            }
        } else if !self.trades.is_empty() {
            return Err(invariant("PreOpen 提前产生成交"));
        }
        Ok(data)
    }
}

/// 与原 `finalize_continuous_tick::checked_update_candle` 相同的溢出预检 +
/// `SessionCandleBook::record_trade_or_mark` 相同的标记语义，落在 owner 持有的
/// 先验日 K 上（同一事实单一载体：终值直接随 patch 交付）。
fn checked_mark_candle(
    prior: Option<DailyCandle>,
    time: i64,
    price: Money,
    qty: u64,
) -> Result<DailyCandle, StepFatal> {
    if qty > 0 {
        let gross = u64::try_from(price.cents())
            .ok()
            .filter(|cents| *cents > 0)
            .and_then(|cents| cents.checked_mul(qty))
            .ok_or_else(|| {
                invariant("continuous candle trade turnover overflow or invalid price")
            })?;
        if let Some(candle) = prior.as_ref().filter(|candle| candle.volume > 0) {
            candle
                .volume
                .checked_add(qty)
                .ok_or_else(|| invariant("continuous candle volume overflow"))?;
            let stats = candle
                .trade_stats
                .as_ref()
                .ok_or_else(|| invariant("continuous active candle lacks trade statistics"))?;
            stats
                .turnover_cents
                .checked_add(gross)
                .ok_or_else(|| invariant("continuous candle turnover overflow"))?;
            stats
                .trade_count
                .checked_add(1)
                .ok_or_else(|| invariant("continuous candle trade count overflow"))?;
        }
    }
    let mut candle = prior.unwrap_or(DailyCandle {
        time,
        open: price,
        high: price,
        low: price,
        close: price,
        volume: 0,
        trade_stats: Some(Default::default()),
    });
    // 开盘前 PriceTick 会用昨收建立零成交占位 K。集合竞价后的第一笔真实成交
    // 才是当日开盘价；此时必须丢弃占位 OHLC（与 record_trade_or_mark 同语义）。
    if candle.volume == 0 && qty > 0 {
        return Ok(DailyCandle {
            time,
            open: price,
            high: price,
            low: price,
            close: price,
            volume: qty,
            trade_stats: Some(crate::DailyTradeStats::from_trade(price, qty)),
        });
    }
    candle.high = candle.high.max(price);
    candle.low = candle.low.min(price);
    candle.close = price;
    candle.volume = candle
        .volume
        .checked_add(qty)
        .expect("daily candle volume overflow: engine state invariant violated");
    if qty > 0 {
        candle
            .trade_stats
            .as_mut()
            .expect("a real active candle must retain trade statistics")
            .record_trade(price, qty);
    } else if candle.trade_stats.is_none() {
        candle.trade_stats = Some(Default::default());
    }
    Ok(candle)
}

fn owned_market_event(event: Event, index: u64) -> EventFact {
    EventFact {
        key: EventStableKey::for_event(&event, index),
        event,
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
            return Err(invariant("增量 StockProcessing 重放了候选 key"));
        }
        if coordinator
            .seen_sealed_indices
            .contains(&operation.sealed_index())
            || !sealed.insert(operation.sealed_index())
        {
            return Err(invariant("增量 StockProcessing 重放了 sealed identity"));
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
            "增量 StockProcessing 本轮包含重复 typed fact 身份",
        ));
    }
    let receipt_ids = receipts
        .iter()
        .map(|receipt| &receipt.local_key)
        .collect::<BTreeSet<_>>();
    if receipt_ids.len() != receipts.len() {
        return Err(invariant("增量 StockProcessing 本轮包含重复 receipt 身份"));
    }
    Ok(())
}

fn operation_code(operation: &ValidatedOperation) -> &StockId {
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
pub(in crate::session::pipeline) mod tests;

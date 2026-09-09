//! 可重放的量价统计基线。
//!
//! 该模块只观察 [`GameSession`] 的权威事件，不参与撮合或 NPC 决策。诊断运行使用
//! 与正式游戏相同的 setup 和引擎路径，以便在改策略前后对比多 seed 分布。

use std::collections::{BTreeMap, BTreeSet};

use serde::{
    ser::{SerializeMap, SerializeSeq},
    Serializer,
};

use crate::{
    behavior::{DecisionReason, PositionAction},
    session::RetailOrderDiagnosticEvent,
    strategy::{HotStyle, InstitutionStyle, RetailStyle, StrategyProfile},
    AccountId, DailyCandle, Event, GameSession, Money, RejectionReason, SessionError, SessionSetup,
    StockCode, TradingPhase,
};

#[derive(Debug, thiserror::Error)]
pub enum BaselineError {
    #[error("量价基线至少需要一个 seed")]
    EmptySeeds,
    #[error("量价基线的交易日数必须大于 0")]
    ZeroTradingDays,
    #[error("量价基线 seed 数量 {0} 超过 u32 可表示范围")]
    TooManySeeds(usize),
    #[error("量价基线 seed {0} 重复；每条随机路径只能作为一个独立样本")]
    DuplicateSeed(u64),
    #[error("量价基线总 tick 数溢出：ticks_per_day={ticks_per_day}, trading_days={trading_days}")]
    TickCountOverflow {
        ticks_per_day: u64,
        trading_days: u32,
    },
    #[error("seed {seed} 创建游戏会话失败：{source}")]
    Session {
        seed: u64,
        #[source]
        source: SessionError,
    },
    #[error("seed {seed} 的股票 {code:?} 仅归档 {actual} 个交易日，期望 {expected} 个")]
    MissingDailyCandles {
        seed: u64,
        code: StockCode,
        expected: u32,
        actual: usize,
    },
    #[error("seed {seed} 的股票 {code:?} 成交量统计溢出")]
    VolumeOverflow { seed: u64, code: StockCode },
    #[error("seed {seed} 的股票 {code:?} 成交额统计溢出")]
    TurnoverOverflow { seed: u64, code: StockCode },
    #[error(
        "seed {seed} 的股票 {code:?} Trade 成交量 {event_volume} 与日 K 成交量 {daily_volume} 不一致"
    )]
    VolumeMismatch {
        seed: u64,
        code: StockCode,
        event_volume: u64,
        daily_volume: u64,
    },
    #[error(
        "seed {seed} 的股票 {code:?} Trade 成交额 {event_turnover_cents} 分与日 K 成交额 {daily_turnover_cents} 分不一致"
    )]
    TurnoverMismatch {
        seed: u64,
        code: StockCode,
        event_turnover_cents: u64,
        daily_turnover_cents: u64,
    },
    #[error("seed {seed} 的股票 {code:?} 第 {day} 日缺少权威成交统计")]
    MissingDailyTradeStats {
        seed: u64,
        code: StockCode,
        day: u32,
    },
    #[error("seed {seed} 的股票 {code:?} 在盘前静默阶段出现了成交")]
    TradeDuringPreOpen { seed: u64, code: StockCode },
    #[error("seed {seed} 的诊断计数器 {counter} 溢出")]
    CounterOverflow { seed: u64, counter: &'static str },
    #[error("seed {seed} 的成交引用未知账户 {account:?}")]
    UnknownTradeParticipant { seed: u64, account: AccountId },
    #[error("seed {seed} 的双边参与量 {actual} 与市场成交量两倍 {expected} 不一致")]
    ParticipantVolumeMismatch {
        seed: u64,
        expected: u64,
        actual: u64,
    },
    #[error("seed {seed} 的按策略档案参与量 {actual} 与双边参与量 {expected} 不一致")]
    ParticipantProfileVolumeMismatch {
        seed: u64,
        expected: u64,
        actual: u64,
    },
}

/// 一组 setup 在多个随机种子下的可比量价报告。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PriceVolumeBaselineReport {
    pub trading_days: u32,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub ticks_per_day: u64,
    pub runs: Vec<PriceVolumeRunReport>,
    /// 跨 seed 的确定性汇总；不合并或隐藏逐 seed 原始报告。
    pub stocks: BTreeMap<StockCode, StockEnsembleReport>,
    /// 每只股票保留最差流动性、最大绝对终值偏移和最大回撤对应的 seed。
    pub extreme_cases: Vec<ExtremeSeedCase>,
}

/// 一个跨 seed 指标的分布摘要。分位数采用排序样本的线性插值；均值区间采用
/// `mean ± 1.96 * sample_stddev / sqrt(n)`，用于回归比较而非总体参数推断。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct DistributionSummary {
    pub sample_count: u32,
    pub minimum: f64,
    pub p05: f64,
    pub p25: f64,
    pub median: f64,
    pub p75: f64,
    pub p95: f64,
    pub maximum: f64,
    pub mean: f64,
    pub mean_ci95_low: f64,
    pub mean_ci95_high: f64,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct StockEnsembleReport {
    pub seed_count: u32,
    pub mean_daily_volume: DistributionSummary,
    pub zero_volume_day_ratio: DistributionSummary,
    pub terminal_return_bps: DistributionSummary,
    pub daily_return_stddev_bps: DistributionSummary,
    pub maximum_drawdown_bps: DistributionSummary,
    pub mean_daily_turnover_rate_bps: Option<DistributionSummary>,
    pub auction_volume_share: Option<DistributionSummary>,
    pub continuous_volume_share_by_decile: [Option<DistributionSummary>; 10],
    pub longest_continuous_no_trade_ticks: DistributionSummary,
    pub mean_quoted_spread_bps: Option<DistributionSummary>,
    pub mean_top_five_depth_shares: Option<DistributionSummary>,
    pub mean_absolute_top_five_imbalance: Option<DistributionSummary>,
    pub all_cancellations_to_accept_ratio: Option<DistributionSummary>,
    pub daily_return_lag1_correlation: Option<DistributionSummary>,
    pub absolute_return_lag1_correlation: Option<DistributionSummary>,
    pub return_excess_kurtosis: Option<DistributionSummary>,
    pub volume_absolute_return_correlation: Option<DistributionSummary>,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct ExtremeSeedCase {
    pub code: StockCode,
    pub metric: &'static str,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub seed: u64,
    pub value: f64,
}

/// 单个 seed 的完整报告。所有 u64 序列化为十进制字符串，避免 JSON/JavaScript 精度损失。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct PriceVolumeRunReport {
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub seed: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub final_tick: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub trade_events: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub rejection_events: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub engine_error_events: u64,
    /// 仅由真实进入散户 B02/B03 判断路径的目标仓位样本聚合而来；不把目标当成委托或成交。
    pub retail_behavior: RetailBehaviorRunReport,
    pub retail_execution: RetailExecutionRunReport,
    /// 成交的双边参与量按玩家或 NPC 公开策略档案归因；每笔成交会同时计入 maker 和 taker，
    /// 不可将该量与单边成交量或市场成交量直接比较。
    pub participant_execution: ParticipantExecutionRunReport,
    pub stocks: BTreeMap<StockCode, StockPriceVolumeReport>,
}

/// 单个 seed 的成交参与归因。仅使用权威 `Trade` 事件和公开策略档案，不读取 NPC 私有资产。
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct ParticipantExecutionRunReport {
    /// 所有 `Trade` 的 maker 与 taker 各计一次的参与股数，故理论上等于市场成交股数的两倍。
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub two_sided_participant_shares: u64,
    /// 上述双边参与量按 `player` 或具体 NPC 策略风格拆分；数值使用字符串避免 JSON 精度丢失。
    #[serde(serialize_with = "serialize_string_u64_map_decimal")]
    pub two_sided_participant_shares_by_profile: BTreeMap<String, u64>,
}

/// 单个 seed 的散户目标仓位与执行边界统计。
///
/// `desired_*` 是判断层完整目标差额，`executable_*` 已受 T+1 与库存边界限制；两者都不代表
/// 实际成交。成交、拒绝和撤单仍由同一报告的权威事件统计表达。
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct RetailBehaviorRunReport {
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub observed_decisions: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub desired_buy_shares: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub desired_sell_shares: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub executable_buy_shares: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub executable_sell_shares: u64,
    #[serde(serialize_with = "serialize_named_u64_map_decimal")]
    pub action_counts: BTreeMap<&'static str, u64>,
    #[serde(serialize_with = "serialize_named_u64_map_decimal")]
    pub reason_counts: BTreeMap<&'static str, u64>,
}

/// 单个 seed 的散户真实订单结果。它从订单生命周期而非目标仓位推导。
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct RetailExecutionRunReport {
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub submitted_orders: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub submitted_shares: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub filled_shares: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub canceled_shares: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub aborted_shares: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub open_shares: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub rejected_intents: u64,
    /// 已提交股数中实际成交的比例；无已提交委托时为 None，不用 0 掩盖无样本。
    pub filled_share_ratio: Option<f64>,
    #[serde(serialize_with = "serialize_named_u64_map_decimal")]
    pub rejection_reason_counts: BTreeMap<&'static str, u64>,
}

#[derive(Default)]
struct RetailOrderOutcome {
    requested_qty: u64,
    filled_qty: u64,
    canceled_qty: u64,
}

#[derive(Default)]
struct ParticipantExecutionAccumulator {
    two_sided_participant_shares: u64,
    two_sided_participant_shares_by_profile: BTreeMap<&'static str, u64>,
}

/// 单只股票在一个 seed 下的日级统计。浮点值仅用于诊断，不写回权威游戏状态。
#[derive(Clone, Debug, PartialEq, serde::Serialize)]
pub struct StockPriceVolumeReport {
    pub completed_days: u32,
    pub traded_days: u32,
    pub zero_volume_days: u32,
    pub max_zero_volume_streak: u32,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub trade_event_volume: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub total_daily_volume: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub trade_event_turnover_cents: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub total_daily_turnover_cents: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub auction_volume: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub opening_auction_volume: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub closing_auction_volume: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub continuous_volume: u64,
    pub auction_volume_share: Option<f64>,
    #[serde(serialize_with = "serialize_u64_array_decimal")]
    pub continuous_volume_by_decile: [u64; 10],
    pub continuous_volume_share_by_decile: Option<[f64; 10]>,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub longest_continuous_no_trade_ticks: u64,
    pub mean_quoted_spread_bps: Option<f64>,
    pub mean_top_five_depth_shares: Option<f64>,
    pub mean_absolute_top_five_imbalance: Option<f64>,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub resting_order_acceptances: u64,
    #[serde(serialize_with = "serialize_u64_decimal")]
    pub all_cancellations_including_day_expiry: u64,
    pub all_cancellations_to_accept_ratio: Option<f64>,
    pub mean_daily_volume: f64,
    pub mean_daily_turnover_rate_bps: Option<f64>,
    pub first_open: Money,
    pub final_close: Money,
    pub min_close: Money,
    pub max_close: Money,
    pub terminal_return_bps: f64,
    pub mean_daily_return_bps: f64,
    pub daily_return_stddev_bps: f64,
    pub daily_return_lag1_correlation: Option<f64>,
    pub absolute_return_lag1_correlation: Option<f64>,
    pub return_excess_kurtosis: Option<f64>,
    pub volume_absolute_return_correlation: Option<f64>,
    pub maximum_drawdown_bps: f64,
    pub daily_returns_bps: Vec<f64>,
}

#[derive(Default)]
struct MarketDiagnosticsAccumulator {
    auction_volume: u64,
    opening_auction_volume: u64,
    closing_auction_volume: u64,
    continuous_volume: u64,
    continuous_volume_by_decile: [u64; 10],
    current_continuous_no_trade_ticks: u64,
    longest_continuous_no_trade_ticks: u64,
    book_samples: u64,
    spread_samples: u64,
    imbalance_samples: u64,
    spread_bps_sum: f64,
    top_five_depth_sum: f64,
    absolute_imbalance_sum: f64,
    resting_order_acceptances: u64,
    all_cancellations_including_day_expiry: u64,
}

struct StockSummaryInput {
    initial_price: Money,
    candles: Vec<DailyCandle>,
    trade_event_volume: u64,
    trade_event_turnover_cents: u64,
    float_shares: u32,
    market_diagnostics: MarketDiagnosticsAccumulator,
    seed: u64,
    code: StockCode,
}

/// 用相同 setup 依次运行多个 seed，并统计权威成交事件和每日收盘 K 线。
///
/// 此函数不读取墙钟、Publisher 或 UI 状态，所以相同输入必须得到完全相同的报告。
pub fn run_price_volume_baseline(
    setup: &SessionSetup,
    seeds: &[u64],
    trading_days: u32,
) -> Result<PriceVolumeBaselineReport, BaselineError> {
    if seeds.is_empty() {
        return Err(BaselineError::EmptySeeds);
    }
    if trading_days == 0 {
        return Err(BaselineError::ZeroTradingDays);
    }
    let mut unique_seeds = BTreeSet::new();
    for &seed in seeds {
        if !unique_seeds.insert(seed) {
            return Err(BaselineError::DuplicateSeed(seed));
        }
    }
    let seed_count =
        u32::try_from(seeds.len()).map_err(|_| BaselineError::TooManySeeds(seeds.len()))?;
    let total_ticks = setup
        .ticks_per_day
        .checked_mul(u64::from(trading_days))
        .ok_or(BaselineError::TickCountOverflow {
            ticks_per_day: setup.ticks_per_day,
            trading_days,
        })?;

    let mut runs = Vec::with_capacity(seeds.len());
    for &seed in seeds {
        runs.push(run_one_seed(setup, seed, trading_days, total_ticks)?);
    }
    let (stocks, extreme_cases) = summarize_ensemble(setup, &runs, seed_count);
    Ok(PriceVolumeBaselineReport {
        trading_days,
        ticks_per_day: setup.ticks_per_day,
        runs,
        stocks,
        extreme_cases,
    })
}

fn run_one_seed(
    setup: &SessionSetup,
    seed: u64,
    trading_days: u32,
    total_ticks: u64,
) -> Result<PriceVolumeRunReport, BaselineError> {
    let mut session = GameSession::new(setup.clone(), seed)
        .map_err(|source| BaselineError::Session { seed, source })?;
    let mut daily_candles: BTreeMap<StockCode, Vec<DailyCandle>> = setup
        .stocks
        .iter()
        .map(|stock| {
            (
                stock.code.clone(),
                Vec::with_capacity(trading_days as usize),
            )
        })
        .collect();
    let mut trade_event_volume: BTreeMap<StockCode, u64> = setup
        .stocks
        .iter()
        .map(|stock| (stock.code.clone(), 0))
        .collect();
    let mut trade_event_turnover: BTreeMap<StockCode, u64> = setup
        .stocks
        .iter()
        .map(|stock| (stock.code.clone(), 0))
        .collect();
    let mut market_diagnostics: BTreeMap<StockCode, MarketDiagnosticsAccumulator> = setup
        .stocks
        .iter()
        .map(|stock| (stock.code.clone(), MarketDiagnosticsAccumulator::default()))
        .collect();
    let mut trade_events = 0_u64;
    let mut rejection_events = 0_u64;
    let mut engine_error_events = 0_u64;
    let mut retail_behavior = RetailBehaviorRunReport::default();
    let mut retail_execution = RetailExecutionRunReport::default();
    let mut retail_orders: BTreeMap<u64, RetailOrderOutcome> = BTreeMap::new();
    let mut participant_execution = ParticipantExecutionAccumulator::default();
    let mut participant_profiles: BTreeMap<AccountId, &'static str> = session
        .account_strategy_profiles()
        .into_iter()
        .map(|(account, profile)| (account, strategy_profile_name(&profile)))
        .collect();
    participant_profiles.insert(AccountId(0), "player");

    for elapsed_tick in 1..=total_ticks {
        let phase = session.phase();
        let day_tick = (elapsed_tick - 1) % setup.ticks_per_day + 1;
        let mut traded_codes = BTreeSet::new();
        let events = session.step();
        for trace in session.last_retail_decisions() {
            record_retail_decision(
                &mut retail_behavior,
                trace.decision.action,
                trace.decision.reason,
                trace.decision.desired_delta_shares,
                trace.decision.executable_delta_shares,
                seed,
            )?;
        }
        for event in session.last_retail_order_events() {
            record_retail_order_event(&mut retail_execution, &mut retail_orders, event, seed)?;
        }
        for event in events {
            match event {
                Event::Trade {
                    code,
                    price,
                    qty,
                    maker,
                    taker,
                    ..
                } => {
                    record_trade_participant(
                        &mut participant_execution,
                        &participant_profiles,
                        maker,
                        qty,
                        seed,
                    )?;
                    record_trade_participant(
                        &mut participant_execution,
                        &participant_profiles,
                        taker,
                        qty,
                        seed,
                    )?;
                    traded_codes.insert(code.clone());
                    trade_events = checked_increment(trade_events, seed, "trade_events")?;
                    let volume = trade_event_volume.get_mut(&code).ok_or_else(|| {
                        BaselineError::MissingDailyCandles {
                            seed,
                            code: code.clone(),
                            expected: trading_days,
                            actual: 0,
                        }
                    })?;
                    *volume = volume.checked_add(u64::from(qty)).ok_or_else(|| {
                        BaselineError::VolumeOverflow {
                            seed,
                            code: code.clone(),
                        }
                    })?;
                    let turnover = u64::try_from(price.cents())
                        .ok()
                        .and_then(|price_cents| price_cents.checked_mul(u64::from(qty)))
                        .and_then(|fill_turnover| {
                            trade_event_turnover
                                .get(&code)
                                .and_then(|current| current.checked_add(fill_turnover))
                        })
                        .ok_or_else(|| BaselineError::TurnoverOverflow {
                            seed,
                            code: code.clone(),
                        })?;
                    *trade_event_turnover.get_mut(&code).ok_or_else(|| {
                        BaselineError::MissingDailyCandles {
                            seed,
                            code: code.clone(),
                            expected: trading_days,
                            actual: 0,
                        }
                    })? = turnover;
                    let diagnostics = market_diagnostics.get_mut(&code).ok_or_else(|| {
                        BaselineError::MissingDailyCandles {
                            seed,
                            code: code.clone(),
                            expected: trading_days,
                            actual: 0,
                        }
                    })?;
                    match phase {
                        TradingPhase::CallAuction | TradingPhase::ClosingAuction => {
                            diagnostics.auction_volume = diagnostics
                                .auction_volume
                                .checked_add(u64::from(qty))
                                .ok_or_else(|| BaselineError::VolumeOverflow {
                                    seed,
                                    code: code.clone(),
                                })?;
                            let phase_volume = match phase {
                                TradingPhase::CallAuction => {
                                    &mut diagnostics.opening_auction_volume
                                }
                                TradingPhase::ClosingAuction => {
                                    &mut diagnostics.closing_auction_volume
                                }
                                TradingPhase::PreOpen | TradingPhase::Continuous => unreachable!(),
                            };
                            *phase_volume =
                                phase_volume.checked_add(u64::from(qty)).ok_or_else(|| {
                                    BaselineError::VolumeOverflow {
                                        seed,
                                        code: code.clone(),
                                    }
                                })?;
                        }
                        TradingPhase::Continuous => {
                            diagnostics.continuous_volume = diagnostics
                                .continuous_volume
                                .checked_add(u64::from(qty))
                                .ok_or_else(|| BaselineError::VolumeOverflow {
                                    seed,
                                    code: code.clone(),
                                })?;
                            let continuous_ticks = setup
                                .ticks_per_day
                                .saturating_sub(setup.auction_ticks)
                                .saturating_sub(setup.closing_auction_ticks);
                            let continuous_tick = day_tick - setup.auction_ticks - 1;
                            let decile =
                                ((continuous_tick * 10) / continuous_ticks).min(9) as usize;
                            diagnostics.continuous_volume_by_decile[decile] = diagnostics
                                .continuous_volume_by_decile[decile]
                                .checked_add(u64::from(qty))
                                .ok_or_else(|| BaselineError::VolumeOverflow {
                                    seed,
                                    code: code.clone(),
                                })?;
                        }
                        TradingPhase::PreOpen => {
                            return Err(BaselineError::TradeDuringPreOpen { seed, code });
                        }
                    }
                }
                Event::DayBoundary {
                    closed_daily_candles,
                    ..
                } => {
                    for (code, candle) in closed_daily_candles {
                        let candles = daily_candles.get_mut(&code).ok_or_else(|| {
                            BaselineError::MissingDailyCandles {
                                seed,
                                code: code.clone(),
                                expected: trading_days,
                                actual: 0,
                            }
                        })?;
                        candles.push(candle);
                    }
                }
                Event::IntentRejected { .. } => {
                    rejection_events =
                        checked_increment(rejection_events, seed, "rejection_events")?;
                }
                Event::SettlementError { .. } | Event::VError { .. } => {
                    engine_error_events =
                        checked_increment(engine_error_events, seed, "engine_error_events")?;
                }
                Event::PriceTick {
                    code, bids, asks, ..
                } => {
                    let diagnostics = market_diagnostics
                        .get_mut(&code)
                        .expect("setup stocks initialized the market diagnostics map");
                    diagnostics.book_samples =
                        checked_increment(diagnostics.book_samples, seed, "book_samples")?;
                    let bid_depth = bids.iter().map(|(_, qty)| *qty as f64).sum::<f64>();
                    let ask_depth = asks.iter().map(|(_, qty)| *qty as f64).sum::<f64>();
                    diagnostics.top_five_depth_sum += bid_depth + ask_depth;
                    if let (Some((best_bid, _)), Some((best_ask, _))) = (bids.first(), asks.first())
                    {
                        let midpoint = (best_bid.cents() as f64 + best_ask.cents() as f64) / 2.0;
                        if midpoint > 0.0 {
                            diagnostics.spread_samples = checked_increment(
                                diagnostics.spread_samples,
                                seed,
                                "spread_samples",
                            )?;
                            diagnostics.spread_bps_sum +=
                                (best_ask.cents() - best_bid.cents()) as f64 / midpoint * 10_000.0;
                        }
                    }
                    let total_depth = bid_depth + ask_depth;
                    if total_depth > 0.0 {
                        diagnostics.imbalance_samples = checked_increment(
                            diagnostics.imbalance_samples,
                            seed,
                            "imbalance_samples",
                        )?;
                        diagnostics.absolute_imbalance_sum +=
                            (bid_depth - ask_depth).abs() / total_depth;
                    }
                }
                Event::OrderAccepted { code, .. } => {
                    let diagnostics = market_diagnostics
                        .get_mut(&code)
                        .expect("setup stocks initialized the market diagnostics map");
                    diagnostics.resting_order_acceptances = checked_increment(
                        diagnostics.resting_order_acceptances,
                        seed,
                        "resting_order_acceptances",
                    )?;
                }
                Event::OrderCanceled { code, .. } => {
                    let diagnostics = market_diagnostics
                        .get_mut(&code)
                        .expect("setup stocks initialized the market diagnostics map");
                    diagnostics.all_cancellations_including_day_expiry = checked_increment(
                        diagnostics.all_cancellations_including_day_expiry,
                        seed,
                        "all_cancellations_including_day_expiry",
                    )?;
                }
                Event::AuctionTick { .. } | Event::AuctionCompleted { .. } => {}
            }
        }
        for (code, diagnostics) in &mut market_diagnostics {
            if phase != TradingPhase::Continuous || traded_codes.contains(code) {
                diagnostics.current_continuous_no_trade_ticks = 0;
            } else {
                diagnostics.current_continuous_no_trade_ticks = checked_increment(
                    diagnostics.current_continuous_no_trade_ticks,
                    seed,
                    "current_continuous_no_trade_ticks",
                )?;
                diagnostics.longest_continuous_no_trade_ticks = diagnostics
                    .longest_continuous_no_trade_ticks
                    .max(diagnostics.current_continuous_no_trade_ticks);
            }
            if day_tick == setup.ticks_per_day {
                diagnostics.current_continuous_no_trade_ticks = 0;
            }
        }
    }

    let one_sided_market_trade_shares =
        trade_event_volume.values().try_fold(0_u64, |total, qty| {
            total
                .checked_add(*qty)
                .ok_or(BaselineError::CounterOverflow {
                    seed,
                    counter: "one-sided market trade shares",
                })
        })?;
    reconcile_participant_execution(&participant_execution, one_sided_market_trade_shares, seed)?;
    let participant_execution = finalize_participant_execution(participant_execution);

    let mut stocks = BTreeMap::new();
    for stock in &setup.stocks {
        let candles = daily_candles
            .remove(&stock.code)
            .expect("setup stocks initialized the diagnostics candle map");
        if candles.len() != trading_days as usize {
            return Err(BaselineError::MissingDailyCandles {
                seed,
                code: stock.code.clone(),
                expected: trading_days,
                actual: candles.len(),
            });
        }
        let event_volume = trade_event_volume
            .remove(&stock.code)
            .expect("setup stocks initialized the diagnostics volume map");
        let event_turnover = trade_event_turnover
            .remove(&stock.code)
            .expect("setup stocks initialized the diagnostics turnover map");
        let diagnostics = market_diagnostics
            .remove(&stock.code)
            .expect("setup stocks initialized the market diagnostics map");
        stocks.insert(
            stock.code.clone(),
            summarize_stock(StockSummaryInput {
                initial_price: stock.initial_price,
                candles,
                trade_event_volume: event_volume,
                trade_event_turnover_cents: event_turnover,
                float_shares: stock.float_shares,
                market_diagnostics: diagnostics,
                seed,
                code: stock.code.clone(),
            })?,
        );
    }

    Ok(PriceVolumeRunReport {
        seed,
        final_tick: total_ticks,
        trade_events,
        rejection_events,
        engine_error_events,
        retail_behavior,
        retail_execution: finalize_retail_execution(retail_execution, retail_orders, seed)?,
        participant_execution,
        stocks,
    })
}

fn record_trade_participant(
    report: &mut ParticipantExecutionAccumulator,
    profiles: &BTreeMap<AccountId, &'static str>,
    account: AccountId,
    qty: u32,
    seed: u64,
) -> Result<(), BaselineError> {
    let profile = profiles
        .get(&account)
        .ok_or(BaselineError::UnknownTradeParticipant { seed, account })?;
    let qty = u64::from(qty);
    report.two_sided_participant_shares = report
        .two_sided_participant_shares
        .checked_add(qty)
        .ok_or(BaselineError::CounterOverflow {
            seed,
            counter: "two-sided participant shares",
        })?;
    let total = report
        .two_sided_participant_shares_by_profile
        .entry(*profile)
        .or_default();
    *total = total
        .checked_add(qty)
        .ok_or(BaselineError::CounterOverflow {
            seed,
            counter: "participant shares by profile",
        })?;
    Ok(())
}

fn finalize_participant_execution(
    accumulator: ParticipantExecutionAccumulator,
) -> ParticipantExecutionRunReport {
    ParticipantExecutionRunReport {
        two_sided_participant_shares: accumulator.two_sided_participant_shares,
        two_sided_participant_shares_by_profile: accumulator
            .two_sided_participant_shares_by_profile
            .into_iter()
            .map(|(profile, shares)| (profile.to_string(), shares))
            .collect(),
    }
}

fn reconcile_participant_execution(
    report: &ParticipantExecutionAccumulator,
    one_sided_market_trade_shares: u64,
    seed: u64,
) -> Result<(), BaselineError> {
    let expected_two_sided =
        one_sided_market_trade_shares
            .checked_mul(2)
            .ok_or(BaselineError::CounterOverflow {
                seed,
                counter: "expected two-sided participant shares",
            })?;
    if report.two_sided_participant_shares != expected_two_sided {
        return Err(BaselineError::ParticipantVolumeMismatch {
            seed,
            expected: expected_two_sided,
            actual: report.two_sided_participant_shares,
        });
    }
    let profile_total = report
        .two_sided_participant_shares_by_profile
        .values()
        .try_fold(0_u64, |total, qty| {
            total
                .checked_add(*qty)
                .ok_or(BaselineError::CounterOverflow {
                    seed,
                    counter: "participant shares by profile total",
                })
        })?;
    if profile_total != report.two_sided_participant_shares {
        return Err(BaselineError::ParticipantProfileVolumeMismatch {
            seed,
            expected: report.two_sided_participant_shares,
            actual: profile_total,
        });
    }
    Ok(())
}

fn strategy_profile_name(profile: &StrategyProfile) -> &'static str {
    match profile {
        StrategyProfile::Retail(style) => match style {
            RetailStyle::Dormant => "retail_dormant",
            RetailStyle::LongTerm => "retail_long_term",
            RetailStyle::Noise => "retail_noise",
            RetailStyle::DipBuyer => "retail_dip_buyer",
            RetailStyle::Momentum => "retail_momentum",
            RetailStyle::Panic => "retail_panic",
        },
        StrategyProfile::Institution(style) => match style {
            InstitutionStyle::DeepValue => "institution_deep_value",
            InstitutionStyle::Growth => "institution_growth",
            InstitutionStyle::Balanced => "institution_balanced",
            InstitutionStyle::Defensive => "institution_defensive",
            InstitutionStyle::ActiveTrader => "institution_active_trader",
        },
        StrategyProfile::Hot(style) => match style {
            HotStyle::Momentum => "hot_momentum",
            HotStyle::Reversal => "hot_reversal",
        },
    }
}

fn record_retail_decision(
    report: &mut RetailBehaviorRunReport,
    action: PositionAction,
    reason: DecisionReason,
    desired_delta_shares: i64,
    executable_delta_shares: i64,
    seed: u64,
) -> Result<(), BaselineError> {
    report.observed_decisions =
        checked_increment(report.observed_decisions, seed, "retail observed decisions")?;
    increment_named_count(
        &mut report.action_counts,
        position_action_name(action),
        seed,
        "retail action counts",
    )?;
    increment_named_count(
        &mut report.reason_counts,
        decision_reason_name(reason),
        seed,
        "retail reason counts",
    )?;
    add_signed_delta(
        &mut report.desired_buy_shares,
        &mut report.desired_sell_shares,
        desired_delta_shares,
        seed,
        "retail desired target shares",
    )?;
    add_signed_delta(
        &mut report.executable_buy_shares,
        &mut report.executable_sell_shares,
        executable_delta_shares,
        seed,
        "retail executable target shares",
    )
}

fn record_retail_order_event(
    report: &mut RetailExecutionRunReport,
    orders: &mut BTreeMap<u64, RetailOrderOutcome>,
    event: &RetailOrderDiagnosticEvent,
    seed: u64,
) -> Result<(), BaselineError> {
    match event {
        RetailOrderDiagnosticEvent::Submitted { order_id, qty, .. } => {
            report.submitted_orders =
                checked_increment(report.submitted_orders, seed, "retail submitted orders")?;
            report.submitted_shares = report.submitted_shares.checked_add(u64::from(*qty)).ok_or(
                BaselineError::CounterOverflow {
                    seed,
                    counter: "retail submitted shares",
                },
            )?;
            if orders
                .insert(
                    order_id.0,
                    RetailOrderOutcome {
                        requested_qty: u64::from(*qty),
                        ..RetailOrderOutcome::default()
                    },
                )
                .is_some()
            {
                return Err(BaselineError::CounterOverflow {
                    seed,
                    counter: "duplicate retail order diagnostic id",
                });
            }
        }
        RetailOrderDiagnosticEvent::Filled { order_id, qty, .. } => {
            let order = orders
                .get_mut(&order_id.0)
                .ok_or(BaselineError::CounterOverflow {
                    seed,
                    counter: "retail fill without submitted order",
                })?;
            order.filled_qty = order.filled_qty.checked_add(u64::from(*qty)).ok_or(
                BaselineError::CounterOverflow {
                    seed,
                    counter: "retail filled order shares",
                },
            )?;
            if order.filled_qty > order.requested_qty {
                return Err(BaselineError::CounterOverflow {
                    seed,
                    counter: "retail fill exceeds submitted order",
                });
            }
            report.filled_shares = report.filled_shares.checked_add(u64::from(*qty)).ok_or(
                BaselineError::CounterOverflow {
                    seed,
                    counter: "retail filled shares",
                },
            )?;
        }
        RetailOrderDiagnosticEvent::Canceled {
            order_id,
            remaining_qty,
            ..
        } => {
            let order = orders
                .get_mut(&order_id.0)
                .ok_or(BaselineError::CounterOverflow {
                    seed,
                    counter: "retail cancellation without submitted order",
                })?;
            let remaining = u64::from(*remaining_qty);
            if order.filled_qty.checked_add(remaining) != Some(order.requested_qty) {
                return Err(BaselineError::CounterOverflow {
                    seed,
                    counter: "retail cancellation quantity mismatch",
                });
            }
            order.canceled_qty = order.canceled_qty.checked_add(remaining).ok_or(
                BaselineError::CounterOverflow {
                    seed,
                    counter: "retail canceled order shares",
                },
            )?;
            report.canceled_shares = report.canceled_shares.checked_add(remaining).ok_or(
                BaselineError::CounterOverflow {
                    seed,
                    counter: "retail canceled shares",
                },
            )?;
        }
        RetailOrderDiagnosticEvent::Aborted {
            order_id,
            remaining_qty,
            ..
        } => {
            let order = orders
                .get_mut(&order_id.0)
                .ok_or(BaselineError::CounterOverflow {
                    seed,
                    counter: "retail abort without submitted order",
                })?;
            let remaining = u64::from(*remaining_qty);
            if order.filled_qty.checked_add(remaining) != Some(order.requested_qty) {
                return Err(BaselineError::CounterOverflow {
                    seed,
                    counter: "retail abort quantity mismatch",
                });
            }
            order.canceled_qty = order.canceled_qty.checked_add(remaining).ok_or(
                BaselineError::CounterOverflow {
                    seed,
                    counter: "retail aborted order shares",
                },
            )?;
            report.aborted_shares = report.aborted_shares.checked_add(remaining).ok_or(
                BaselineError::CounterOverflow {
                    seed,
                    counter: "retail aborted shares",
                },
            )?;
        }
        RetailOrderDiagnosticEvent::Rejected { reason, .. } => {
            report.rejected_intents =
                checked_increment(report.rejected_intents, seed, "retail rejected intents")?;
            increment_named_count(
                &mut report.rejection_reason_counts,
                rejection_reason_name(reason),
                seed,
                "retail rejection reasons",
            )?;
        }
    }
    Ok(())
}

fn finalize_retail_execution(
    mut report: RetailExecutionRunReport,
    orders: BTreeMap<u64, RetailOrderOutcome>,
    seed: u64,
) -> Result<RetailExecutionRunReport, BaselineError> {
    for order in orders.values() {
        let accounted = order.filled_qty.checked_add(order.canceled_qty).ok_or(
            BaselineError::CounterOverflow {
                seed,
                counter: "retail final order accounting",
            },
        )?;
        let remaining =
            order
                .requested_qty
                .checked_sub(accounted)
                .ok_or(BaselineError::CounterOverflow {
                    seed,
                    counter: "retail final order quantity mismatch",
                })?;
        report.open_shares =
            report
                .open_shares
                .checked_add(remaining)
                .ok_or(BaselineError::CounterOverflow {
                    seed,
                    counter: "retail open shares",
                })?;
    }
    report.filled_share_ratio = (report.submitted_shares > 0)
        .then(|| report.filled_shares as f64 / report.submitted_shares as f64);
    Ok(report)
}

fn increment_named_count(
    counts: &mut BTreeMap<&'static str, u64>,
    name: &'static str,
    seed: u64,
    counter: &'static str,
) -> Result<(), BaselineError> {
    let count = counts.entry(name).or_default();
    *count = checked_increment(*count, seed, counter)?;
    Ok(())
}

fn add_signed_delta(
    buys: &mut u64,
    sells: &mut u64,
    delta: i64,
    seed: u64,
    counter: &'static str,
) -> Result<(), BaselineError> {
    let (target, amount) = if delta >= 0 {
        (buys, delta as u64)
    } else {
        (sells, delta.unsigned_abs())
    };
    *target = target
        .checked_add(amount)
        .ok_or(BaselineError::CounterOverflow { seed, counter })?;
    Ok(())
}

fn position_action_name(action: PositionAction) -> &'static str {
    match action {
        PositionAction::Hold => "hold",
        PositionAction::Watch => "watch",
        PositionAction::TryBuy => "try_buy",
        PositionAction::Add => "add",
        PositionAction::Reduce => "reduce",
        PositionAction::Exit => "exit",
    }
}

fn decision_reason_name(reason: DecisionReason) -> &'static str {
    match reason {
        DecisionReason::PositionRisk => "position_risk",
        DecisionReason::TakeProfit => "take_profit",
        DecisionReason::Momentum => "momentum",
        DecisionReason::Pullback => "pullback",
        DecisionReason::BroadMarketRisk => "broad_market_risk",
        DecisionReason::RangeBreakout => "range_breakout",
        DecisionReason::RangeBreakdown => "range_breakdown",
        DecisionReason::AccountDrawdown => "account_drawdown",
        DecisionReason::BaselinePositioning => "baseline_positioning",
        DecisionReason::NoSignal => "no_signal",
        DecisionReason::InsufficientHistory => "insufficient_history",
        DecisionReason::T1Locked => "t1_locked",
        DecisionReason::LowConfidence => "low_confidence",
        DecisionReason::PostExitCooldown => "post_exit_cooldown",
        DecisionReason::BreakEvenRelief => "break_even_relief",
        DecisionReason::ProfitGiveback => "profit_giveback",
    }
}

fn rejection_reason_name(reason: &RejectionReason) -> &'static str {
    match reason {
        RejectionReason::InsufficientCash => "insufficient_cash",
        RejectionReason::InsufficientShares => "insufficient_shares",
        RejectionReason::LimitExceeded => "limit_exceeded",
        RejectionReason::PriceCageExceeded => "price_cage_exceeded",
        RejectionReason::UnknownStock => "unknown_stock",
        RejectionReason::AuctionLimitOrderRequired => "auction_limit_order_required",
        RejectionReason::AuctionOrderNotCancelable => "auction_order_not_cancelable",
        RejectionReason::AuctionOrderEntryClosed => "auction_order_entry_closed",
        RejectionReason::InvalidQuantity => "invalid_quantity",
        RejectionReason::ResourceLimitExceeded => "resource_limit_exceeded",
        RejectionReason::OrderNotFound => "order_not_found",
        RejectionReason::NotOrderOwner => "not_order_owner",
    }
}

fn summarize_stock(input: StockSummaryInput) -> Result<StockPriceVolumeReport, BaselineError> {
    let StockSummaryInput {
        initial_price,
        candles,
        trade_event_volume,
        trade_event_turnover_cents,
        float_shares,
        market_diagnostics,
        seed,
        code,
    } = input;
    let completed_days = candles.len() as u32;
    let traded_days = candles.iter().filter(|candle| candle.volume > 0).count() as u32;
    let zero_volume_days = completed_days - traded_days;
    let mut max_zero_volume_streak = 0_u32;
    let mut current_zero_volume_streak = 0_u32;
    let mut total_daily_volume = 0_u64;
    let mut total_daily_turnover_cents = 0_u64;
    for (day_index, candle) in candles.iter().enumerate() {
        total_daily_volume = total_daily_volume
            .checked_add(candle.volume)
            .ok_or_else(|| BaselineError::VolumeOverflow {
                seed,
                code: code.clone(),
            })?;
        let stats =
            candle
                .trade_stats
                .as_ref()
                .ok_or_else(|| BaselineError::MissingDailyTradeStats {
                    seed,
                    code: code.clone(),
                    day: day_index as u32,
                })?;
        total_daily_turnover_cents = total_daily_turnover_cents
            .checked_add(stats.turnover_cents)
            .ok_or_else(|| BaselineError::TurnoverOverflow {
                seed,
                code: code.clone(),
            })?;
        if candle.volume == 0 {
            current_zero_volume_streak += 1;
            max_zero_volume_streak = max_zero_volume_streak.max(current_zero_volume_streak);
        } else {
            current_zero_volume_streak = 0;
        }
    }

    let mut previous_close = initial_price;
    let mut daily_returns_bps = Vec::with_capacity(candles.len());
    for candle in &candles {
        daily_returns_bps.push(return_bps(previous_close, candle.close));
        previous_close = candle.close;
    }
    let mean_daily_return_bps = mean(&daily_returns_bps);
    let daily_return_stddev_bps = population_stddev(&daily_returns_bps, mean_daily_return_bps);
    let absolute_returns: Vec<f64> = daily_returns_bps.iter().map(|value| value.abs()).collect();
    let daily_volumes: Vec<f64> = candles.iter().map(|candle| candle.volume as f64).collect();
    let first = candles
        .first()
        .expect("zero trading days was rejected before simulation");
    let final_candle = candles
        .last()
        .expect("zero trading days was rejected before simulation");
    let min_close = candles
        .iter()
        .map(|candle| candle.close)
        .min()
        .expect("non-empty candles");
    let max_close = candles
        .iter()
        .map(|candle| candle.close)
        .max()
        .expect("non-empty candles");
    let classified_volume = market_diagnostics
        .auction_volume
        .checked_add(market_diagnostics.continuous_volume)
        .ok_or_else(|| BaselineError::VolumeOverflow {
            seed,
            code: code.clone(),
        })?;
    if trade_event_volume != total_daily_volume {
        return Err(BaselineError::VolumeMismatch {
            seed,
            code: code.clone(),
            event_volume: trade_event_volume,
            daily_volume: total_daily_volume,
        });
    }
    if classified_volume != total_daily_volume {
        return Err(BaselineError::VolumeMismatch {
            seed,
            code: code.clone(),
            event_volume: classified_volume,
            daily_volume: total_daily_volume,
        });
    }
    if trade_event_turnover_cents != total_daily_turnover_cents {
        return Err(BaselineError::TurnoverMismatch {
            seed,
            code: code.clone(),
            event_turnover_cents: trade_event_turnover_cents,
            daily_turnover_cents: total_daily_turnover_cents,
        });
    }

    Ok(StockPriceVolumeReport {
        completed_days,
        traded_days,
        zero_volume_days,
        max_zero_volume_streak,
        trade_event_volume,
        total_daily_volume,
        trade_event_turnover_cents,
        total_daily_turnover_cents,
        auction_volume: market_diagnostics.auction_volume,
        opening_auction_volume: market_diagnostics.opening_auction_volume,
        closing_auction_volume: market_diagnostics.closing_auction_volume,
        continuous_volume: market_diagnostics.continuous_volume,
        auction_volume_share: (total_daily_volume > 0)
            .then(|| market_diagnostics.auction_volume as f64 / total_daily_volume as f64),
        continuous_volume_by_decile: market_diagnostics.continuous_volume_by_decile,
        continuous_volume_share_by_decile: (market_diagnostics.continuous_volume > 0).then(|| {
            market_diagnostics
                .continuous_volume_by_decile
                .map(|volume| volume as f64 / market_diagnostics.continuous_volume as f64)
        }),
        longest_continuous_no_trade_ticks: market_diagnostics.longest_continuous_no_trade_ticks,
        mean_quoted_spread_bps: (market_diagnostics.spread_samples > 0)
            .then(|| market_diagnostics.spread_bps_sum / market_diagnostics.spread_samples as f64),
        mean_top_five_depth_shares: (market_diagnostics.book_samples > 0).then(|| {
            market_diagnostics.top_five_depth_sum / market_diagnostics.book_samples as f64
        }),
        mean_absolute_top_five_imbalance: (market_diagnostics.imbalance_samples > 0).then(|| {
            market_diagnostics.absolute_imbalance_sum / market_diagnostics.imbalance_samples as f64
        }),
        resting_order_acceptances: market_diagnostics.resting_order_acceptances,
        all_cancellations_including_day_expiry: market_diagnostics
            .all_cancellations_including_day_expiry,
        all_cancellations_to_accept_ratio: (market_diagnostics.resting_order_acceptances > 0).then(
            || {
                market_diagnostics.all_cancellations_including_day_expiry as f64
                    / market_diagnostics.resting_order_acceptances as f64
            },
        ),
        mean_daily_volume: total_daily_volume as f64 / f64::from(completed_days),
        mean_daily_turnover_rate_bps: (float_shares > 0).then(|| {
            total_daily_volume as f64 / f64::from(completed_days) / float_shares as f64 * 10_000.0
        }),
        first_open: first.open,
        final_close: final_candle.close,
        min_close,
        max_close,
        terminal_return_bps: return_bps(initial_price, final_candle.close),
        mean_daily_return_bps,
        daily_return_stddev_bps,
        daily_return_lag1_correlation: lag_one_correlation(&daily_returns_bps),
        absolute_return_lag1_correlation: lag_one_correlation(&absolute_returns),
        return_excess_kurtosis: excess_kurtosis(&daily_returns_bps),
        volume_absolute_return_correlation: correlation(&daily_volumes, &absolute_returns),
        maximum_drawdown_bps: maximum_drawdown_bps(initial_price, &candles),
        daily_returns_bps,
    })
}

fn summarize_ensemble(
    setup: &SessionSetup,
    runs: &[PriceVolumeRunReport],
    seed_count: u32,
) -> (
    BTreeMap<StockCode, StockEnsembleReport>,
    Vec<ExtremeSeedCase>,
) {
    let mut stocks = BTreeMap::new();
    let mut extreme_cases = Vec::with_capacity(setup.stocks.len() * 3);
    for stock in &setup.stocks {
        let samples: Vec<_> = runs
            .iter()
            .map(|run| (run.seed, &run.stocks[&stock.code]))
            .collect();
        let collect = |metric: fn(&StockPriceVolumeReport) -> f64| {
            samples
                .iter()
                .map(|(_, report)| metric(report))
                .collect::<Vec<_>>()
        };
        let turnover_rates: Vec<_> = samples
            .iter()
            .filter_map(|(_, report)| report.mean_daily_turnover_rate_bps)
            .collect();
        let optional = |metric: fn(&StockPriceVolumeReport) -> Option<f64>| {
            optional_distribution(
                &samples
                    .iter()
                    .filter_map(|(_, report)| metric(report))
                    .collect::<Vec<_>>(),
            )
        };
        stocks.insert(
            stock.code.clone(),
            StockEnsembleReport {
                seed_count,
                mean_daily_volume: distribution(&collect(|report| report.mean_daily_volume)),
                zero_volume_day_ratio: distribution(&collect(|report| {
                    f64::from(report.zero_volume_days) / f64::from(report.completed_days)
                })),
                terminal_return_bps: distribution(&collect(|report| report.terminal_return_bps)),
                daily_return_stddev_bps: distribution(&collect(|report| {
                    report.daily_return_stddev_bps
                })),
                maximum_drawdown_bps: distribution(&collect(|report| report.maximum_drawdown_bps)),
                mean_daily_turnover_rate_bps: (!turnover_rates.is_empty())
                    .then(|| distribution(&turnover_rates)),
                auction_volume_share: optional(|report| report.auction_volume_share),
                continuous_volume_share_by_decile: std::array::from_fn(|decile| {
                    optional_distribution(
                        &samples
                            .iter()
                            .filter_map(|(_, report)| {
                                report
                                    .continuous_volume_share_by_decile
                                    .map(|shares| shares[decile])
                            })
                            .collect::<Vec<_>>(),
                    )
                }),
                longest_continuous_no_trade_ticks: distribution(&collect(|report| {
                    report.longest_continuous_no_trade_ticks as f64
                })),
                mean_quoted_spread_bps: optional(|report| report.mean_quoted_spread_bps),
                mean_top_five_depth_shares: optional(|report| report.mean_top_five_depth_shares),
                mean_absolute_top_five_imbalance: optional(|report| {
                    report.mean_absolute_top_five_imbalance
                }),
                all_cancellations_to_accept_ratio: optional(|report| {
                    report.all_cancellations_to_accept_ratio
                }),
                daily_return_lag1_correlation: optional(|report| {
                    report.daily_return_lag1_correlation
                }),
                absolute_return_lag1_correlation: optional(|report| {
                    report.absolute_return_lag1_correlation
                }),
                return_excess_kurtosis: optional(|report| report.return_excess_kurtosis),
                volume_absolute_return_correlation: optional(|report| {
                    report.volume_absolute_return_correlation
                }),
            },
        );
        for (metric, value_of) in [
            (
                "highest_zero_volume_day_ratio",
                (|report: &StockPriceVolumeReport| {
                    f64::from(report.zero_volume_days) / f64::from(report.completed_days)
                }) as fn(&StockPriceVolumeReport) -> f64,
            ),
            (
                "largest_absolute_terminal_return_bps",
                (|report: &StockPriceVolumeReport| report.terminal_return_bps.abs())
                    as fn(&StockPriceVolumeReport) -> f64,
            ),
            (
                "maximum_drawdown_bps",
                (|report: &StockPriceVolumeReport| report.maximum_drawdown_bps)
                    as fn(&StockPriceVolumeReport) -> f64,
            ),
        ] {
            let (seed, report) = samples
                .iter()
                .copied()
                .max_by(|left, right| value_of(left.1).total_cmp(&value_of(right.1)))
                .expect("empty seeds were rejected before ensemble summarization");
            extreme_cases.push(ExtremeSeedCase {
                code: stock.code.clone(),
                metric,
                seed,
                value: value_of(report),
            });
        }
    }
    (stocks, extreme_cases)
}

fn distribution(values: &[f64]) -> DistributionSummary {
    debug_assert!(!values.is_empty());
    debug_assert!(values.iter().all(|value| value.is_finite()));
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let values_mean = mean(&sorted);
    let sample_stddev = if sorted.len() > 1 {
        (sorted
            .iter()
            .map(|value| (value - values_mean).powi(2))
            .sum::<f64>()
            / (sorted.len() - 1) as f64)
            .sqrt()
    } else {
        0.0
    };
    let half_width = 1.96 * sample_stddev / (sorted.len() as f64).sqrt();
    DistributionSummary {
        sample_count: sorted.len() as u32,
        minimum: sorted[0],
        p05: quantile(&sorted, 0.05),
        p25: quantile(&sorted, 0.25),
        median: quantile(&sorted, 0.5),
        p75: quantile(&sorted, 0.75),
        p95: quantile(&sorted, 0.95),
        maximum: sorted[sorted.len() - 1],
        mean: values_mean,
        mean_ci95_low: values_mean - half_width,
        mean_ci95_high: values_mean + half_width,
    }
}

fn optional_distribution(values: &[f64]) -> Option<DistributionSummary> {
    (!values.is_empty()).then(|| distribution(values))
}

fn quantile(sorted: &[f64], probability: f64) -> f64 {
    let index = probability * (sorted.len() - 1) as f64;
    let lower = index.floor() as usize;
    let upper = index.ceil() as usize;
    sorted[lower] + (sorted[upper] - sorted[lower]) * index.fract()
}

fn maximum_drawdown_bps(initial_price: Money, candles: &[DailyCandle]) -> f64 {
    let mut peak = initial_price.cents() as f64;
    let mut maximum = 0.0_f64;
    for candle in candles {
        let close = candle.close.cents() as f64;
        peak = peak.max(close);
        maximum = maximum.max((peak - close) / peak * 10_000.0);
    }
    maximum
}

fn excess_kurtosis(values: &[f64]) -> Option<f64> {
    if values.len() < 4 {
        return None;
    }
    let values_mean = mean(values);
    let second = values
        .iter()
        .map(|value| (value - values_mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    if second == 0.0 {
        return None;
    }
    let fourth = values
        .iter()
        .map(|value| (value - values_mean).powi(4))
        .sum::<f64>()
        / values.len() as f64;
    Some(fourth / second.powi(2) - 3.0)
}

fn correlation(left: &[f64], right: &[f64]) -> Option<f64> {
    if left.len() != right.len() || left.len() < 2 {
        return None;
    }
    let left_mean = mean(left);
    let right_mean = mean(right);
    let covariance = left
        .iter()
        .zip(right)
        .map(|(x, y)| (x - left_mean) * (y - right_mean))
        .sum::<f64>();
    let left_scale = left
        .iter()
        .map(|value| (value - left_mean).powi(2))
        .sum::<f64>();
    let right_scale = right
        .iter()
        .map(|value| (value - right_mean).powi(2))
        .sum::<f64>();
    let left_reference = left.iter().map(|value| value.abs()).fold(1.0_f64, f64::max);
    let right_reference = right
        .iter()
        .map(|value| value.abs())
        .fold(1.0_f64, f64::max);
    let numerical_floor = f64::EPSILON * left.len() as f64;
    if left_scale <= numerical_floor * left_reference.powi(2)
        || right_scale <= numerical_floor * right_reference.powi(2)
    {
        return None;
    }
    let denominator = (left_scale * right_scale).sqrt();
    (denominator > 0.0).then_some(covariance / denominator)
}

fn return_bps(from: Money, to: Money) -> f64 {
    (to.cents() as f64 / from.cents() as f64 - 1.0) * 10_000.0
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn population_stddev(values: &[f64], values_mean: f64) -> f64 {
    let variance = values
        .iter()
        .map(|value| (value - values_mean).powi(2))
        .sum::<f64>()
        / values.len() as f64;
    variance.sqrt()
}

fn lag_one_correlation(values: &[f64]) -> Option<f64> {
    if values.len() < 3 {
        return None;
    }
    let left = &values[..values.len() - 1];
    let right = &values[1..];
    correlation(left, right)
}

fn serialize_u64_decimal<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    serializer.serialize_str(&value.to_string())
}

fn serialize_u64_array_decimal<S>(values: &[u64; 10], serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut sequence = serializer.serialize_seq(Some(values.len()))?;
    for value in values {
        sequence.serialize_element(&value.to_string())?;
    }
    sequence.end()
}

fn serialize_named_u64_map_decimal<S>(
    values: &BTreeMap<&'static str, u64>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(values.len()))?;
    for (name, value) in values {
        map.serialize_entry(name, &value.to_string())?;
    }
    map.end()
}

fn serialize_string_u64_map_decimal<S>(
    values: &BTreeMap<String, u64>,
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut map = serializer.serialize_map(Some(values.len()))?;
    for (name, value) in values {
        map.serialize_entry(name, &value.to_string())?;
    }
    map.end()
}

fn checked_increment(value: u64, seed: u64, counter: &'static str) -> Result<u64, BaselineError> {
    value
        .checked_add(1)
        .ok_or(BaselineError::CounterOverflow { seed, counter })
}

#[cfg(test)]
mod tests {
    use super::{
        distribution, lag_one_correlation, reconcile_participant_execution, summarize_stock,
        BaselineError, MarketDiagnosticsAccumulator, ParticipantExecutionAccumulator,
        StockSummaryInput,
    };
    use crate::{DailyCandle, DailyTradeStats, Money, StockCode};

    fn candle(open: i64, close: i64, volume: u64) -> DailyCandle {
        DailyCandle {
            time: 0,
            open: Money::from_cents(open),
            high: Money::from_cents(open.max(close)),
            low: Money::from_cents(open.min(close)),
            close: Money::from_cents(close),
            volume,
            trade_stats: Some(DailyTradeStats {
                turnover_cents: u64::try_from(open).unwrap() * volume,
                trade_count: u64::from(volume > 0),
            }),
        }
    }

    #[test]
    fn participant_execution_reconciliation_rejects_missing_side_or_profile_volume() {
        let mut missing_side = ParticipantExecutionAccumulator {
            two_sided_participant_shares: 100,
            two_sided_participant_shares_by_profile: [("player", 100)].into(),
        };
        assert!(matches!(
            reconcile_participant_execution(&missing_side, 100, 7),
            Err(BaselineError::ParticipantVolumeMismatch {
                expected: 200,
                actual: 100,
                ..
            })
        ));

        missing_side.two_sided_participant_shares = 200;
        assert!(matches!(
            reconcile_participant_execution(&missing_side, 100, 7),
            Err(BaselineError::ParticipantProfileVolumeMismatch {
                expected: 200,
                actual: 100,
                ..
            })
        ));
    }

    #[test]
    fn known_candles_produce_expected_returns_and_population_stddev() {
        let code = StockCode("600101".to_string());
        let market_diagnostics = MarketDiagnosticsAccumulator {
            auction_volume: 30,
            opening_auction_volume: 30,
            closing_auction_volume: 0,
            continuous_volume: 270,
            continuous_volume_by_decile: [27; 10],
            longest_continuous_no_trade_ticks: 12,
            book_samples: 2,
            spread_samples: 2,
            imbalance_samples: 2,
            spread_bps_sum: 30.0,
            top_five_depth_sum: 1_000.0,
            absolute_imbalance_sum: 0.5,
            resting_order_acceptances: 4,
            all_cancellations_including_day_expiry: 2,
            ..MarketDiagnosticsAccumulator::default()
        };
        let report = summarize_stock(StockSummaryInput {
            initial_price: Money::from_cents(10_000),
            candles: vec![candle(10_000, 11_000, 100), candle(11_000, 9_900, 200)],
            trade_event_volume: 300,
            trade_event_turnover_cents: 3_200_000,
            float_shares: 10_000,
            market_diagnostics,
            seed: 42,
            code,
        })
        .unwrap();

        assert!((report.daily_returns_bps[0] - 1_000.0).abs() < 1e-9);
        assert!((report.daily_returns_bps[1] - -1_000.0).abs() < 1e-9);
        assert!(report.mean_daily_return_bps.abs() < 1e-9);
        assert!((report.daily_return_stddev_bps - 1_000.0).abs() < 1e-9);
        assert!((report.terminal_return_bps - -100.0).abs() < 1e-9);
        assert_eq!(report.total_daily_volume, 300);
        assert_eq!(report.total_daily_turnover_cents, 3_200_000);
        assert!((report.mean_daily_turnover_rate_bps.unwrap() - 150.0).abs() < 1e-12);
        assert!((report.auction_volume_share.unwrap() - 0.1).abs() < 1e-12);
        assert!(report
            .continuous_volume_share_by_decile
            .unwrap()
            .iter()
            .all(|share| (*share - 0.1).abs() < 1e-12));
        assert_eq!(report.longest_continuous_no_trade_ticks, 12);
        assert!((report.mean_quoted_spread_bps.unwrap() - 15.0).abs() < 1e-12);
        assert!((report.mean_top_five_depth_shares.unwrap() - 500.0).abs() < 1e-12);
        assert!((report.mean_absolute_top_five_imbalance.unwrap() - 0.25).abs() < 1e-12);
        assert_eq!(report.resting_order_acceptances, 4);
        assert_eq!(report.all_cancellations_including_day_expiry, 2);
        assert!((report.all_cancellations_to_accept_ratio.unwrap() - 0.5).abs() < 1e-12);
        assert!((report.maximum_drawdown_bps - 1_000.0).abs() < 1e-9);
        assert_eq!(report.daily_return_lag1_correlation, None);
        assert_eq!(report.absolute_return_lag1_correlation, None);
        assert_eq!(report.return_excess_kurtosis, None);
        assert_eq!(report.volume_absolute_return_correlation, None);
    }

    #[test]
    fn lag_one_correlation_matches_a_known_perfect_series() {
        let correlation = lag_one_correlation(&[1.0, 2.0, 3.0, 4.0]).unwrap();
        assert!((correlation - 1.0).abs() < 1e-12);
        assert_eq!(lag_one_correlation(&[1.0, 1.0, 1.0]), None);
    }

    #[test]
    fn distribution_uses_interpolated_quantiles_and_contains_its_mean() {
        let summary = distribution(&[4.0, 1.0, 3.0, 2.0]);
        assert_eq!(summary.sample_count, 4);
        assert!((summary.p05 - 1.15).abs() < 1e-12);
        assert!((summary.median - 2.5).abs() < 1e-12);
        assert!((summary.p95 - 3.85).abs() < 1e-12);
        assert!(summary.mean_ci95_low <= summary.mean);
        assert!(summary.mean <= summary.mean_ci95_high);
        let singleton = distribution(&[7.0]);
        assert_eq!(singleton.mean_ci95_low, 7.0);
        assert_eq!(singleton.mean_ci95_high, 7.0);
    }

    #[test]
    fn stock_summary_rejects_trade_and_daily_turnover_disagreement() {
        let code = StockCode("600101".to_string());
        let market_diagnostics = MarketDiagnosticsAccumulator {
            continuous_volume: 100,
            continuous_volume_by_decile: [100, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            ..MarketDiagnosticsAccumulator::default()
        };
        let result = summarize_stock(StockSummaryInput {
            initial_price: Money::from_cents(10_000),
            candles: vec![candle(10_000, 10_000, 100)],
            trade_event_volume: 100,
            trade_event_turnover_cents: 999_999,
            float_shares: 10_000,
            market_diagnostics,
            seed: 42,
            code,
        });
        assert!(matches!(
            result,
            Err(BaselineError::TurnoverMismatch { .. })
        ));
    }

    #[test]
    fn stock_summary_rejects_missing_daily_statistics_and_volume_classification_drift() {
        let code = StockCode("600101".to_string());
        let mut missing_stats = candle(10_000, 10_000, 100);
        missing_stats.trade_stats = None;
        let result = summarize_stock(StockSummaryInput {
            initial_price: Money::from_cents(10_000),
            candles: vec![missing_stats],
            trade_event_volume: 100,
            trade_event_turnover_cents: 1_000_000,
            float_shares: 10_000,
            market_diagnostics: MarketDiagnosticsAccumulator {
                continuous_volume: 100,
                continuous_volume_by_decile: [100, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                ..MarketDiagnosticsAccumulator::default()
            },
            seed: 42,
            code: code.clone(),
        });
        assert!(matches!(
            result,
            Err(BaselineError::MissingDailyTradeStats { .. })
        ));

        let result = summarize_stock(StockSummaryInput {
            initial_price: Money::from_cents(10_000),
            candles: vec![candle(10_000, 10_000, 100)],
            trade_event_volume: 100,
            trade_event_turnover_cents: 1_000_000,
            float_shares: 10_000,
            market_diagnostics: MarketDiagnosticsAccumulator {
                continuous_volume: 99,
                continuous_volume_by_decile: [99, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                ..MarketDiagnosticsAccumulator::default()
            },
            seed: 42,
            code,
        });
        assert!(matches!(result, Err(BaselineError::VolumeMismatch { .. })));
    }

    #[test]
    fn zero_float_reports_turnover_rate_as_missing_instead_of_zero() {
        let report = summarize_stock(StockSummaryInput {
            initial_price: Money::from_cents(10_000),
            candles: vec![candle(10_000, 10_000, 100)],
            trade_event_volume: 100,
            trade_event_turnover_cents: 1_000_000,
            float_shares: 0,
            market_diagnostics: MarketDiagnosticsAccumulator {
                continuous_volume: 100,
                continuous_volume_by_decile: [100, 0, 0, 0, 0, 0, 0, 0, 0, 0],
                ..MarketDiagnosticsAccumulator::default()
            },
            seed: 42,
            code: StockCode("600101".to_string()),
        })
        .unwrap();
        assert_eq!(report.mean_daily_turnover_rate_bps, None);
    }
}

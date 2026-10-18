//! 在可丢弃 tick shadow 上捕获生产 DecisionAndContinuation 输入。
//!
//! 只推进密封 strategy 输入所需的 attention/retail-observation 状态。
//! 不路由 intent，不修改订单簿。

use super::{DecisionAccountInput, DecisionSnapshot, DecisionSnapshotError};
use crate::behavior::BehaviorMarketObservation;
use crate::compute::parallel::*;
use crate::observation::{
    build_account_risk_observation, build_equal_weight_market_observation, AccountRiskObservation,
    ObservationError, RiskPositionInput,
};
use crate::session::{AuctionOrdersByAccount, ContinuousOrdersByAccount};
use crate::strategy::{PositionView, SelfView, StrategyStateError};
use crate::{
    AccountId, AccountKind, ExperienceError, Money, MoneyError, RetailExperienceState, StockId,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug)]
pub(in crate::session) struct CapturedDecisionSnapshot {
    pub(super) snapshot: Arc<DecisionSnapshot>,
    pub(super) working_continuous: ContinuousOrdersByAccount,
    pub(super) working_auction: AuctionOrdersByAccount,
}

#[derive(Debug, thiserror::Error)]
pub(in crate::session) enum DecisionSnapshotCaptureError {
    #[error("DecisionAndContinuation 决策快照受理了不存在的账户 {0:?}")]
    MissingAccount(AccountId),
    #[error("DecisionAndContinuation 决策快照将玩家账户 {0:?} 作为 NPC 受理")]
    PlayerAccount(AccountId),
    #[error("DecisionAndContinuation 决策快照账户 {0:?} 缺少 attention state")]
    MissingAttention(AccountId),
    #[error("DecisionAndContinuation 决策快照账户 {account:?} 的注意力概率 {probability} 无效")]
    InvalidAttentionProbability {
        account: AccountId,
        probability: f64,
    },
    #[error("DecisionAndContinuation 决策快照账户 {0:?} 缺少生产 strategy")]
    MissingStrategy(AccountId),
    #[error("DecisionAndContinuation 决策快照无法导出账户 {account:?} 的 strategy：{source}")]
    StrategyState {
        account: AccountId,
        #[source]
        source: StrategyStateError,
    },
    #[error("DecisionAndContinuation 账户 {account:?} 的散户经历观察失败：{source}")]
    Experience {
        account: AccountId,
        #[source]
        source: ExperienceError,
    },
    #[error("DecisionAndContinuation 决策快照 {location} 缺少股票 {code:?} 的市场数据")]
    MissingMarketData {
        location: &'static str,
        code: StockId,
    },
    #[error("DecisionAndContinuation 竞价账户 {account:?} 股票 {code:?} 活动委托 {order:?} 缺少book事实")]
    MissingWorkingOrder {
        account: AccountId,
        code: StockId,
        order: crate::OrderId,
    },
    #[error("DecisionAndContinuation（决策与续行协调）账户 {account:?} 股票 {code:?} 委托 {order:?} 的已验证预留事实不可用：{source}")]
    FinancialReservation {
        account: AccountId,
        code: StockId,
        order: crate::OrderId,
        #[source]
        source: super::StepFatal,
    },
    #[error("DecisionAndContinuation 决策快照 {location} 的账户 {account:?} 金额计算失败：{source}")]
    Money {
        location: &'static str,
        account: AccountId,
        #[source]
        source: MoneyError,
    },
    #[error("DecisionAndContinuation 决策快照 {location} 的账户 {account:?} 观察失败：{source}")]
    Observation {
        location: &'static str,
        account: Option<AccountId>,
        #[source]
        source: ObservationError,
    },
    #[error("DecisionAndContinuation 决策快照契约拒绝捕获的输入：{0}")]
    Snapshot(#[source] DecisionSnapshotError),
    #[error("DecisionAndContinuation urgency policy 校验失败：{0}")]
    UrgencyPolicy(#[from] crate::plans::UrgencyError),
    #[error("DecisionAndContinuation 散户个人分析失败: {0}")]
    RetailAnalysis(#[source] crate::session::StepFatal),
}

/// 在 shadow 上推进到期 attention 与决策前散户观察，
/// 返回纯 NPC 决策源消费的不可变输入。
///
/// 此处仅修改到期 attention 与受理的散户观察；
/// 捕获失败时调用方丢弃完整 tick candidate。
pub(in crate::session) fn capture_decision_snapshot(
    shadow: &mut super::GameSession,
) -> Result<CapturedDecisionSnapshot, DecisionSnapshotCaptureError> {
    let (view, mut writes) = super::npc_split::npc_split(shadow);
    capture_decision_snapshot_on(&view, &mut writes)
}

/// 读视图+写组版本（queue 链转换目标形态）。
pub(super) fn capture_decision_snapshot_on(
    view: &super::npc_split::SessionReadView<'_>,
    writes: &mut super::npc_split::NpcWriteFields<'_>,
) -> Result<CapturedDecisionSnapshot, DecisionSnapshotCaptureError> {
    let tick = view.tick;
    let phase = view.phase();
    validate_market_view_inputs_on(view)?;
    let market = crate::session::views::build_market_view_on(view);

    let (due_npc_ids, popped) = {
        let attention = &*writes.npc_attention;
        writes.attention_scheduler.pop_due(tick, |id| {
            attention
                .get(&id)
                .map(|state| state.next_attention_candidate_tick)
        })
    };
    let attention_signal = super::super::attention::market_attention_signal(&market);
    if let Some(account) = popped
        .iter()
        .map(|(_, account)| *account)
        .find(|account| !writes.npc_attention.contains_key(account))
    {
        return Err(DecisionSnapshotCaptureError::MissingAttention(account));
    }
    let accounts = &view.accounts;
    let attention_results = view.compute.parallel(|| {
        writes.npc_attention.mutate_existing_parallel(
            &due_npc_ids,
            DecisionSnapshotCaptureError::MissingAttention,
            |account, attention| {
                let kind = accounts
                    .get(&account)
                    .map(|entry| entry.kind())
                    .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
                if kind == AccountKind::Player {
                    return Err(DecisionSnapshotCaptureError::PlayerAccount(account));
                }
                if !attention.base_probability.is_finite()
                    || attention.base_probability <= 0.0
                    || attention.base_probability > 1.0
                {
                    return Err(DecisionSnapshotCaptureError::InvalidAttentionProbability {
                        account,
                        probability: attention.base_probability,
                    });
                }
                let observes =
                    attention.evaluate_candidate_with_signal(kind, attention_signal, tick);
                Ok((observes, attention.next_attention_candidate_tick))
            },
        )
    })?;
    let mut accepted_due_npc_ids = Vec::new();
    let mut scheduled = Vec::with_capacity(attention_results.len());
    for (account, (observes, next_tick)) in attention_results {
        scheduled.push((next_tick, account));
        if observes {
            accepted_due_npc_ids.push(account);
        }
    }

    let market_minute = view.current_market_minute();
    let moment = crate::experience::ExperienceMoment {
        civil_date: view.civil_date(),
        market_minute,
        trading_day: u64::from(view.day),
    };
    let (working_continuous, working_auction) = if accepted_due_npc_ids.is_empty() {
        (BTreeMap::new(), BTreeMap::new())
    } else {
        crate::session::self_views::working_orders_for_accounts_on(
            view,
            &accepted_due_npc_ids.iter().copied().collect(),
        )
    };
    let has_retail_observer = accepted_due_npc_ids.iter().any(|account| {
        view.accounts
            .get(account)
            .is_some_and(|entry| entry.kind() == AccountKind::Retail)
    });
    let behavior_market = has_retail_observer
        .then(|| build_behavior_market_checked(view))
        .transpose()?;
    let experience = &*writes.retail_experience;
    let prepared = view.compute.parallel(|| {
        accepted_due_npc_ids
            .par_iter()
            .map(|account| {
                view.compute.record(crate::compute::ComputeWork::Observe);
                let entry = view
                    .accounts
                    .get(account)
                    .ok_or(DecisionSnapshotCaptureError::MissingAccount(*account))?;
                let observed = capture_npc_account_input_on(
                    view,
                    *account,
                    entry.kind(),
                    experience.get(account).cloned(),
                    market_minute,
                    phase,
                    &moment,
                    &working_continuous,
                    &working_auction,
                )?;
                Ok((*account, observed))
            })
            .collect::<Vec<Result<_, DecisionSnapshotCaptureError>>>()
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
    })?;
    let mut accounts = BTreeMap::new();
    let mut experience_updates = Vec::new();
    for (account, observed) in prepared {
        if let Some(row) = observed.experience_row {
            experience_updates.push((account, row));
        }
        accounts.insert(account, observed.input);
    }

    // 观察更新后的经验行先安装（分析经写组读取同批行——批处理路径保持
    // 原「先观察安装、后分析」次序）。
    for (account, row) in experience_updates {
        writes.retail_experience.insert(account, row);
    }
    let mut retail_analysis = crate::GameSession::capture_retail_analysis_on(
        view,
        writes,
        &accepted_due_npc_ids,
        &market,
    )
    .map_err(DecisionSnapshotCaptureError::RetailAnalysis)?;
    let accounts = accounts
        .into_iter()
        .map(|(account, input)| {
            (
                account,
                input.with_retail_analysis(retail_analysis.remove(&account)),
            )
        })
        .collect();
    let snapshot = DecisionSnapshot::new(
        tick,
        view.seed,
        phase,
        market_minute,
        market,
        behavior_market,
        accepted_due_npc_ids,
        accounts,
    )
    .map_err(DecisionSnapshotCaptureError::Snapshot)?
    .with_urgency_policy(view.urgency_policy.clone())?;

    for (tick, account) in scheduled {
        writes.attention_scheduler.enqueue(tick, account);
    }
    Ok(CapturedDecisionSnapshot {
        snapshot: Arc::new(snapshot),
        working_continuous,
        working_auction,
    })
}

fn validate_market_view_inputs(
    session: &super::GameSession,
) -> Result<(), DecisionSnapshotCaptureError> {
    validate_market_view_inputs_on(&session.read_view())
}

/// T1·两阶段（ADR-0042 §T1）：单账户**个人观察 + 密封输入组装**的纯输出。
pub(in crate::session) struct NpcObservedAccount {
    /// 密封决策输入（未挂 retail_analysis；持有经验行的 Arc 快照）。
    pub(in crate::session) input: DecisionAccountInput,
    /// 观察更新后的经验行（owned）——分析与 quote/plan 投影在同一行上继续
    /// 演化，任务结束后由归并侧整体安装。
    pub(in crate::session) experience_row: Option<RetailExperienceState>,
}

/// T1·两阶段（ADR-0042 §T1）：单账户个人观察（经验行权益/持仓观察、
/// SelfView、风险观察）并组装密封决策输入——纯计算、账户任务内可调，
/// 不借用会话写组。批处理捕获路径（测试适配器）与 queue 账户任务共用。
#[allow(clippy::too_many_arguments)]
pub(in crate::session) fn capture_npc_account_input_on(
    view: &super::npc_split::SessionReadView<'_>,
    account: AccountId,
    kind: AccountKind,
    experience_row: Option<RetailExperienceState>,
    market_minute: u64,
    phase: crate::TradingPhase,
    moment: &crate::experience::ExperienceMoment,
    working_continuous: &crate::session::ContinuousOrdersByAccount,
    working_auction: &crate::session::AuctionOrdersByAccount,
) -> Result<NpcObservedAccount, DecisionSnapshotCaptureError> {
    let entry = view
        .accounts
        .get(&account)
        .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
    let observation = match experience_row {
        Some(row) => CapturedExperienceObservation::capture_from_row(row, view, account, market_minute)?,
        None => CapturedExperienceObservation::NoExperience,
    };
    let CapturedAccountObservation {
        self_view,
        account_risk,
        experience,
    } = observation.consume(account, kind, entry.cash(), |self_positions| {
        build_self_view_for(
            view,
            account,
            phase,
            working_continuous,
            working_auction,
            self_positions,
        )
    })?;
    let strategy_state = entry
        .strategy()
        .ok_or(DecisionSnapshotCaptureError::MissingStrategy(account))?
        .production_state()
        .map_err(|source| DecisionSnapshotCaptureError::StrategyState { account, source })?;
    let mut input =
        DecisionAccountInput::new_shared(kind, self_view, strategy_state, account_risk, experience);
    // 经验行 owned 终值：观察更新后的行（Arc 快照留在输入内）。
    let experience_row = input.retail_experience().map(|row| row.clone());
    if kind == AccountKind::Retail {
        if let Some(state) = input.retail_experience() {
            let influence = state.failure_influence(moment).map_err(|source| {
                DecisionSnapshotCaptureError::Experience { account, source }
            })?;
            input = input.with_failure_influence(influence);
        }
    }
    Ok(NpcObservedAccount {
        input,
        experience_row,
    })
}

/// 读视图版本。
pub(super) fn validate_market_view_inputs_on(
    view: &super::npc_split::SessionReadView<'_>,
) -> Result<(), DecisionSnapshotCaptureError> {
    for code in view.markets.keys() {
        for (location, present) in [
            (
                "market price history",
                view.price_history.contains_key(code),
            ),
            (
                "market-minute history",
                view.market_minute_closes.contains_key(code),
            ),
            (
                "daily history",
                view.candle_book.histories().contains_key(code),
            ),
        ] {
            if !present {
                return Err(DecisionSnapshotCaptureError::MissingMarketData {
                    location,
                    code: code.clone(),
                });
            }
        }
    }
    Ok(())
}

/// 同一次账户观察的派生事实；非 Retail 的已有 experience 仍可不携带风险输入。
enum CapturedExperienceObservation {
    NoExperience,
    Captured {
        experience: Arc<RetailExperienceState>,
        risk_positions: Option<BTreeMap<StockId, RiskPositionInput>>,
        self_positions: BTreeMap<StockId, PositionView>,
    },
}

struct CapturedAccountObservation {
    self_view: SelfView,
    account_risk: Option<AccountRiskObservation>,
    experience: Option<Arc<RetailExperienceState>>,
}

impl CapturedExperienceObservation {
    fn capture(
        session: &super::GameSession,
        account: AccountId,
        market_minute: u64,
    ) -> Result<Self, DecisionSnapshotCaptureError> {
        Self::capture_on(
            &session.state.retail_experience,
            &session.read_view(),
            account,
            market_minute,
        )
    }

    /// 读视图+写组再借用版本：retail_experience 由调用方经 &*writes 再借用传入。
    fn capture_on(
        experience: &crate::session::AccountPagedMap<crate::RetailExperienceState>,
        view: &super::npc_split::SessionReadView<'_>,
        account: AccountId,
        market_minute: u64,
    ) -> Result<Self, DecisionSnapshotCaptureError> {
        match experience.get(&account).cloned() {
            Some(row) => Self::capture_from_row(row, view, account, market_minute),
            None => Ok(Self::NoExperience),
        }
    }

    /// T1·两阶段（ADR-0042 §T1）：owned 经验行版本——queue 链账户任务在
    /// 任务内对已 take 的个人经验行做个人观察（纯计算）。
    fn capture_from_row(
        mut experience: crate::RetailExperienceState,
        view: &super::npc_split::SessionReadView<'_>,
        account: AccountId,
        market_minute: u64,
    ) -> Result<Self, DecisionSnapshotCaptureError> {
        let entry = view
            .accounts
            .get(&account)
            .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
        let mut equity = entry.cash();
        let mut positions = Vec::with_capacity(entry.positions().len());
        let mut self_positions = BTreeMap::new();
        for (code, position) in entry.positions() {
            let price = view
                .markets
                .get(code)
                .ok_or_else(|| DecisionSnapshotCaptureError::MissingMarketData {
                    location: "retail experience position",
                    code: code.clone(),
                })?
                .last_price();
            equity = equity
                .add(price.mul_shares(position.qty()).map_err(|source| {
                    DecisionSnapshotCaptureError::Money {
                        location: "retail experience equity",
                        account,
                        source,
                    }
                })?)
                .map_err(|source| DecisionSnapshotCaptureError::Money {
                    location: "retail experience equity",
                    account,
                    source,
                })?;
            positions.push((code.clone(), price, position.qty(), position.cost_price()));
            self_positions.insert(
                code.clone(),
                PositionView {
                    qty: position.qty(),
                    sellable_qty: position.sellable(),
                    cost_price: position.cost_price(),
                },
            );
        }
        if equity.cents() > 0 {
            experience
                .observe_equity(equity)
                .map_err(|source| DecisionSnapshotCaptureError::Experience { account, source })?;
        }
        let held: BTreeSet<_> = self_positions.keys().cloned().collect();
        let moment = crate::experience::ExperienceMoment {
            civil_date: view.civil_date(),
            market_minute,
            trading_day: u64::from(view.day),
        };
        let mut risk_positions = BTreeMap::new();
        for (code, price, qty, cost_price) in positions {
            if !experience.feedback.stocks.contains_key(&code) {
                experience
                    .initialize_holding_dated(
                        &code,
                        cost_price.filter(|cost| cost.cents() > 0),
                        price,
                        moment,
                    )
                    .map_err(|source| DecisionSnapshotCaptureError::Experience {
                        account,
                        source,
                    })?;
            }
            experience
                .observe_position_dated(&code, price, moment)
                .map_err(|source| DecisionSnapshotCaptureError::Experience { account, source })?;
            if entry.kind() == AccountKind::Retail {
                risk_positions.insert(
                    code.clone(),
                    RiskPositionInput {
                        qty,
                        cost_price: cost_price.filter(|price| price.cents() > 0),
                        last_price: price,
                        peak_price_since_entry: experience
                            .stocks
                            .get(&code)
                            .and_then(|stock| stock.peak_price_since_entry),
                    },
                );
            }
        }
        experience.prune_watchlist(&held);
        let risk_positions = (entry.kind() == AccountKind::Retail).then_some(risk_positions);
        Ok(Self::Captured {
            experience: Arc::new(experience),
            risk_positions,
            self_positions,
        })
    }

    fn consume(
        self,
        account: AccountId,
        kind: AccountKind,
        cash: Money,
        build_self_view: impl FnOnce(
            Option<BTreeMap<StockId, PositionView>>,
        ) -> Result<SelfView, DecisionSnapshotCaptureError>,
    ) -> Result<CapturedAccountObservation, DecisionSnapshotCaptureError> {
        match self {
            Self::NoExperience => {
                let self_view = build_self_view(None)?;
                if kind == AccountKind::Retail {
                    return Err(DecisionSnapshotCaptureError::MissingAccount(account));
                }
                Ok(CapturedAccountObservation {
                    self_view,
                    account_risk: None,
                    experience: None,
                })
            }
            Self::Captured {
                experience,
                risk_positions,
                self_positions,
            } => {
                // SelfView 的 Money 错误仍先于风险观察错误；不得在 capture 中提前计算风险。
                let self_view = build_self_view(Some(self_positions))?;
                let account_risk = risk_positions
                    .map(|positions| {
                        build_account_risk_observation(
                            cash,
                            &positions,
                            experience.reference_equity,
                            experience.peak_equity,
                        )
                        .map_err(|source| {
                            DecisionSnapshotCaptureError::Observation {
                                location: "account risk",
                                account: Some(account),
                                source,
                            }
                        })
                    })
                    .transpose()?;
                Ok(CapturedAccountObservation {
                    self_view,
                    account_risk,
                    experience: Some(experience),
                })
            }
        }
    }
}

/// SelfView 的报价现金累计；可替换部分不构成本轮 SealAllocationSnapshot 执行预算。
#[derive(Default)]
struct SelfViewCashReservations {
    reserved: Money,
    replaceable: Money,
}

impl SelfViewCashReservations {
    fn record(&mut self, required: Money, may_replace: bool) -> Result<(), MoneyError> {
        self.reserved = self.reserved.add(required)?;
        if may_replace {
            self.replaceable = self.replaceable.add(required)?;
        }
        Ok(())
    }

    fn available_cash(&self, raw_cash: Money) -> Result<Money, MoneyError> {
        raw_cash.sub(self.reserved)?.add(self.replaceable)
    }
}

fn verified_self_reservation(
    view: &super::npc_split::SessionReadView<'_>,
    account: AccountId,
    code: StockId,
    order_id: crate::OrderId,
    side: crate::Side,
) -> Result<Money, DecisionSnapshotCaptureError> {
    let wrap = |source| DecisionSnapshotCaptureError::FinancialReservation {
        account,
        code,
        order: order_id,
        source,
    };
    let book_order = view
        .markets
        .get(&code)
        .and_then(|market| market.resting_order_by_id(order_id))
        .ok_or(DecisionSnapshotCaptureError::MissingWorkingOrder {
            account,
            code,
            order: order_id,
        })?;
    let key = super::EnvelopeKey {
        account,
        stock: code,
        order: order_id,
        side,
    };
    let envelope = view.envelope_ledger.get(&key).map_err(wrap)?;
    let audit = envelope.audit();
    if envelope.key() != &key
        || book_order.owner != account
        || book_order.side != side
        || audit.limit != book_order.price
        || audit.remaining_qty != book_order.qty
        || audit.filled_qty != book_order.filled_qty
        || audit.filled_value != book_order.filled_value
    {
        return Err(wrap(super::ledger_validation::invariant(
            "SelfView工作单与规范财务订单不一致",
        )));
    }
    Ok(envelope
        .verified_live(&view.setup.config)
        .map_err(wrap)?
        .cash)
}

fn build_self_view_for(
    view: &super::npc_split::SessionReadView<'_>,
    account: AccountId,
    phase: crate::TradingPhase,
    continuous: &super::super::ContinuousOrdersByAccount,
    auction: &super::super::AuctionOrdersByAccount,
    self_positions: Option<BTreeMap<StockId, PositionView>>,
) -> Result<SelfView, DecisionSnapshotCaptureError> {
    let auction_cancelable = phase == crate::TradingPhase::CallAuction
        && view.tick % view.setup.ticks_per_day < view.setup.auction_ticks / 3;
    let entry = view
        .accounts
        .get(&account)
        .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
    let mut reservations = SelfViewCashReservations::default();
    for (code, order) in continuous.get(&account).into_iter().flatten() {
        reservations
            .record(
                verified_self_reservation(view, account, *code, order.id, order.side)?,
                phase == crate::TradingPhase::Continuous,
            )
            .map_err(|source| DecisionSnapshotCaptureError::Money {
                location: "continuous self-view reservation sum",
                account,
                source,
            })?;
    }
    for (code, order) in auction.get(&account).into_iter().flatten() {
        reservations
            .record(
                verified_self_reservation(
                    view,
                    account,
                    *code,
                    crate::OrderId(order.order_id),
                    order.side,
                )?,
                auction_cancelable,
            )
            .map_err(|source| DecisionSnapshotCaptureError::Money {
                location: "auction self-view reservation sum",
                account,
                source,
            })?;
    }
    let cash = reservations
        .available_cash(entry.cash())
        .map_err(|source| DecisionSnapshotCaptureError::Money {
            location: "available self-view cash",
            account,
            source,
        })?;
    let positions = self_positions.unwrap_or_else(|| {
        entry
            .positions()
            .iter()
            .map(|(code, position)| {
                (
                    code.clone(),
                    PositionView {
                        qty: position.qty(),
                        sellable_qty: position.sellable(),
                        cost_price: position.cost_price(),
                    },
                )
            })
            .collect()
    });
    Ok(SelfView { cash, positions })
}

pub(super) fn build_behavior_market_checked(
    view: &super::npc_split::SessionReadView<'_>,
) -> Result<BehaviorMarketObservation, DecisionSnapshotCaptureError> {
    let price_paths =
        crate::session::views::market_price_path_observations_on(view).map_err(|source| {
            DecisionSnapshotCaptureError::Observation {
                location: "behavior price paths",
                account: None,
                source,
            }
        })?;
    let returns = price_paths
        .iter()
        .map(|(code, path)| (code.clone(), path.thirty_minute.return_ratio))
        .collect();
    let thirty_minute_market =
        build_equal_weight_market_observation(&returns).map_err(|source| {
            DecisionSnapshotCaptureError::Observation {
                location: "behavior equal-weight market",
                account: None,
                source,
            }
        })?;
    Ok(BehaviorMarketObservation {
        price_paths,
        thirty_minute_market,
    })
}

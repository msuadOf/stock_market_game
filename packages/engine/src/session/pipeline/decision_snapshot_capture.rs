//! 在可丢弃 tick shadow 上捕获生产 DecisionShadow 输入。
//!
//! 只推进密封 strategy 输入所需的 attention/retail-observation 状态。
//! 不路由 intent，不修改订单簿。

use super::{DecisionAccountInput, DecisionSnapshot, DecisionSnapshotError};
use crate::behavior::BehaviorMarketObservation;
use crate::observation::{
    build_account_risk_observation, build_equal_weight_market_observation, AccountRiskObservation,
    ObservationError, RiskPositionInput,
};
use crate::session::{AuctionOrdersByAccount, ContinuousOrdersByAccount};
use crate::strategy::{PositionView, SelfView, StrategyStateError};
use crate::{
    AccountId, AccountKind, ExperienceError, Money, MoneyError, RetailExperienceState, StockId,
};
use rayon::prelude::*;
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
    #[error("DecisionShadow 决策快照受理了不存在的账户 {0:?}")]
    MissingAccount(AccountId),
    #[error("DecisionShadow 决策快照将玩家账户 {0:?} 作为 NPC 受理")]
    PlayerAccount(AccountId),
    #[error("DecisionShadow 决策快照账户 {0:?} 缺少 attention state")]
    MissingAttention(AccountId),
    #[error("DecisionShadow 决策快照账户 {account:?} 的注意力概率 {probability} 无效")]
    InvalidAttentionProbability {
        account: AccountId,
        probability: f64,
    },
    #[error("DecisionShadow 决策快照账户 {0:?} 缺少生产 strategy")]
    MissingStrategy(AccountId),
    #[error("DecisionShadow 决策快照无法导出账户 {account:?} 的 strategy：{source}")]
    StrategyState {
        account: AccountId,
        #[source]
        source: StrategyStateError,
    },
    #[error("DecisionShadow 账户 {account:?} 的散户经历观察失败：{source}")]
    Experience {
        account: AccountId,
        #[source]
        source: ExperienceError,
    },
    #[error("DecisionShadow 决策快照 {location} 缺少股票 {code:?} 的市场数据")]
    MissingMarketData {
        location: &'static str,
        code: StockId,
    },
    #[error("DecisionShadow 决策快照 {location} 的账户 {account:?} 金额计算失败：{source}")]
    Money {
        location: &'static str,
        account: AccountId,
        #[source]
        source: MoneyError,
    },
    #[error("DecisionShadow 决策快照 {location} 的账户 {account:?} 观察失败：{source}")]
    Observation {
        location: &'static str,
        account: Option<AccountId>,
        #[source]
        source: ObservationError,
    },
    #[error("DecisionShadow 决策快照契约拒绝捕获的输入：{0}")]
    Snapshot(#[source] DecisionSnapshotError),
    #[error("DecisionShadow urgency policy 校验失败：{0}")]
    UrgencyPolicy(#[from] crate::plans::UrgencyError),
    #[error("DecisionShadow 散户个人分析失败: {0}")]
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
fn capture_decision_snapshot_on(
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
    let attention_results = writes.npc_attention.mutate_existing_parallel(
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
            let observes = attention.evaluate_candidate_with_signal(kind, attention_signal, tick);
            Ok((observes, attention.next_attention_candidate_tick))
        },
    )?;
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
        view.accounts.get(account)
            .is_some_and(|entry| entry.kind() == AccountKind::Retail)
    });
    let behavior_market = has_retail_observer
        .then(|| build_behavior_market_checked(view))
        .transpose()?;
    let experience = &*writes.retail_experience;
    let prepared = accepted_due_npc_ids
        .par_iter()
        .map(|account| {
            let entry = view
                .accounts
                .get(account)
                .ok_or(DecisionSnapshotCaptureError::MissingAccount(*account))?;
            let observation =
                CapturedExperienceObservation::capture_on(experience, view, *account, market_minute)?;
            let CapturedAccountObservation {
                self_view,
                account_risk,
                experience,
            } = observation.consume(*account, entry.kind(), entry.cash(), |self_positions| {
                build_self_view_for(
                    view,
                    *account,
                    phase,
                    &working_continuous,
                    &working_auction,
                    self_positions,
                )
            })?;
            let strategy_state = entry
                .strategy()
                .ok_or(DecisionSnapshotCaptureError::MissingStrategy(*account))?
                .production_state()
                .map_err(|source| DecisionSnapshotCaptureError::StrategyState {
                    account: *account,
                    source,
                })?;
            let input = DecisionAccountInput::new_shared(
                entry.kind(),
                self_view,
                strategy_state,
                account_risk,
                experience.clone(),
            );
            let input = if let Some(state) = experience
                .as_ref()
                .filter(|_| entry.kind() == AccountKind::Retail)
            {
                input.with_failure_influence(state.failure_influence(&moment).map_err(
                    |source| DecisionSnapshotCaptureError::Experience {
                        account: *account,
                        source,
                    },
                )?)
            } else {
                input
            };
            Ok((*account, input, experience))
        })
        .collect::<Vec<Result<_, DecisionSnapshotCaptureError>>>()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    let mut accounts = BTreeMap::new();
    let mut experience_updates = Vec::new();
    for (account, input, experience) in prepared {
        accounts.insert(account, input);
        if let Some(experience) = experience {
            experience_updates.push((account, experience));
        }
    }

    writes
        .retail_experience
        .replace_existing_shared_parallel(experience_updates)
        .map_err(DecisionSnapshotCaptureError::MissingAccount)?;
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

/// 读视图版本。
fn validate_market_view_inputs_on(
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
        Self::capture_on(&session.state.retail_experience, &session.read_view(), account, market_minute)
    }

    /// 读视图+写组再借用版本：retail_experience 由调用方经 &*writes 再借用传入。
    fn capture_on(
        experience: &crate::session::AccountPagedMap<crate::RetailExperienceState>,
        view: &super::npc_split::SessionReadView<'_>,
        account: AccountId,
        market_minute: u64,
    ) -> Result<Self, DecisionSnapshotCaptureError> {
        let Some(mut experience) = experience.get(&account).cloned() else {
            return Ok(Self::NoExperience);
        };
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

fn build_self_view_for(
    view: &super::npc_split::SessionReadView<'_>,
    account: AccountId,
    phase: crate::TradingPhase,
    continuous: &super::super::ContinuousOrdersByAccount,
    auction: &super::super::AuctionOrdersByAccount,
    self_positions: Option<BTreeMap<StockId, PositionView>>,
) -> Result<SelfView, DecisionSnapshotCaptureError> {
    let auction_cancelable = phase == crate::TradingPhase::CallAuction
        && view.tick % view.setup.ticks_per_day
            < view.setup.auction_ticks / 3;
    let entry = view
        .accounts
        .get(&account)
        .ok_or(DecisionSnapshotCaptureError::MissingAccount(account))?;
    let mut reservations = SelfViewCashReservations::default();
    for (_, order) in continuous.get(&account).into_iter().flatten() {
        reservations
            .record(
                super::super::live_cash_reservation(
                    &view.setup.config,
                    order.side,
                    order.price,
                    order.qty,
                    order.filled_value,
                )
                .map_err(|source| DecisionSnapshotCaptureError::Money {
                    location: "continuous self-view reservation",
                    account,
                    source,
                })?,
                phase == crate::TradingPhase::Continuous,
            )
            .map_err(|source| DecisionSnapshotCaptureError::Money {
                location: "continuous self-view reservation sum",
                account,
                source,
            })?;
    }
    for (_, order) in auction.get(&account).into_iter().flatten() {
        reservations
            .record(
                super::super::live_cash_reservation(
                    &view.setup.config,
                    order.side,
                    order.limit,
                    order.qty,
                    Money::ZERO,
                )
                .map_err(|source| DecisionSnapshotCaptureError::Money {
                    location: "auction self-view reservation",
                    account,
                    source,
                })?,
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

fn build_behavior_market_checked(
    view: &super::npc_split::SessionReadView<'_>,
) -> Result<BehaviorMarketObservation, DecisionSnapshotCaptureError> {
    let price_paths = crate::session::views::market_price_path_observations_on(view).map_err(|source| {
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

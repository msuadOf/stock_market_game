//! 完整决策链（任务 26）：信念机构账户的 K5a/K6 编排。
//!
//! 链条（每次 accepted 注意力触发，各账户的根观察可并行执行）：
//!
//! 1. **候选集**：持仓 ∪ 已有信念条目 ∪ 本次个体发现
//!    （[`NpcAttentionState::sample_discovery_stock`]，任务 25 的个体注意力流；
//!    发现接受时写入 [`PersonalWatchlist`]）。
//! 2. **本人信息**：对候选发行人调用 `information::discovery_candidates`
//!    （公共曝光面），未获知的公布经 `record_acquisition` 显式登记（新曝光
//!    ≠ 已读；未登记的公布对信念结构性不可读——「unpublished mutation ⇒
//!    同个人决策」的验收由此保证）。
//! 3. **信念更新**：新获知的**年报**触发 `BeliefCause::NewMaterial`（λ 修订 +
//!    按新事实重估）；既有预期到期触发 `HorizonExpired`。估值纯属个人：
//!    每股区间 = 归母整体估计 / 已发行总股本（K5 行 141——绝不把整体权益
//!    量纲与市场报价直接比较）。
//! 4. **K5a 聚合**：五路信号（基本面/趋势/量价/技术/成本经历）按账户
//!    `AnalysisProfile` 权重混合（`plans::blend_candidate`）。机构经历事实独立
//!    保存；成本经历按个人参数形成候选，不直接生成订单或统一止损。
//!    基本面方法不可用 ⇒ 该路 `Unavailable` ⇒ 不开/不反向修订方向性计划
//!    （Watch/InsufficientInformation）。
//! 5. **持续计划**：方向 = 综合分符号；目标股数经
//!    `target_position_weight_bp` + `target_share_quantity`（A 股 100 股整手）
//!    换算；既有计划按复核阈值修订，方向翻转必须跨过 ±2000bp 迟滞门槛
//!    （`reverse_crosses_threshold`）。
//! 6. **预算**：`plans::allocate_soft_budgets` 在账户权威现金上做软预算
//!    （风险减仓→既有计划→新机会排序）。
//! 7. **紧迫度与报价**：`assess_urgency` + `decide_quote`（受保护限价取
//!    个人每股区间的乐观/悲观端；无估值时退到涨跌停带边界）。
//! 8. **执行**：计划动作作为带身份的请求进入本轮 P3/P4；只有真实成交
//!    推进 `filled_qty`。账户级分配仍可能等待本账户其他股票的待反馈命令。
//!
//! 链内没有独立 RNG；交易请求的优先关系由实际资源冲突和订单簿决定。

use super::*;

use crate::information::{discovery_candidates, NpcObservationContext, PublicationId};
#[cfg(test)]
use crate::observation::{build_technical_observation, TechnicalDailyInput};
use crate::observation::{build_technical_observation_from_recent_trades, TechnicalObservation};
use crate::plans::allocate_child_quote_budgets;
#[cfg(test)]
use crate::plans::{allocate_soft_budgets, UrgencyPolicy};
use crate::plans::{
    assess_recovery, assess_urgency, blend_candidate, decide_quote, reverse_crosses_threshold,
    ActiveQuote, AllocationClass, AllocationExperience, AllocationFunds, AllocationRequest,
    AllocationResult, BookTop, CandidateAssessment, CandidateError, CandidateSignals,
    OpinionSource, PatienceStyle, PauseAssessment, PlanBook, PlanEvent, PlanId, PlanOpen,
    PlanOpinion, PlanRevision, PlanStatus, PlanTarget, QuoteDecisionInputs, RecoveryAssessment,
    RecoveryInputs, ResumeReason, RevisionReason, SignalContribution, SignalUnavailableReason,
    TerminationReason, TradingPlan, Urgency, UrgencyInputs,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::strategy::{BeliefBook, BeliefCause, BeliefInputs, PerShareRange, ValuationOutcome};
use rayon::prelude::*;
mod lifecycle;
pub(in crate::session) mod personal_state;
mod quote;
mod roots;
mod urgency;
use personal_state::PlanPersonalState;
pub(in crate::session) use roots::{InstitutionDecisionRoot, RootReadContext};

/// 同批账户 root 使用同一市场、路径、技术、时刻与曝光观察。
pub(in crate::session) struct DecisionChainObservation {
    pub(in crate::session) market: MarketView,
    pub(in crate::session) paths: BTreeMap<StockCode, crate::observation::PricePathObservation>,
    pub(in crate::session) technical: BTreeMap<StockCode, TechnicalObservation>,
    pub(in crate::session) now: crate::calendar::CivilInstant,
    pub(in crate::session) exposed: BTreeSet<StockCode>,
}

impl DecisionChainObservation {
    fn capture_for_roots(session: &GameSession, market: MarketView) -> Result<Self, StepFatal> {
        let now = session.observation_civil_instant();
        Ok(Self {
            market,
            paths: session.market_price_path_observations().map_err(|error| {
                StepFatal::InvariantViolation {
                    description: error.to_string(),
                    location: "decision_chain::capture_decision_chain_roots_with_market".to_owned(),
                }
            })?,
            technical: Self::build_technical(session)?,
            now,
            exposed: Self::exposed_stocks(session, now),
        })
    }
    fn build_technical(
        session: &GameSession,
    ) -> Result<BTreeMap<StockCode, TechnicalObservation>, StepFatal> {
        let stocks = session
            .state
            .candle_book
            .histories()
            .iter()
            .collect::<Vec<_>>();
        let prepared = stocks
            .into_par_iter()
            .map(|(code, candles)| {
                let invariant = |description: String| StepFatal::InvariantViolation {
                    description,
                    location: "decision_chain::build_chain_technical_observations".to_owned(),
                };
                let observation = (|| {
                    u32::try_from(candles.len()).map_err(|_| {
                        invariant(format!("daily candle count exceeds u32 for {code:?}"))
                    })?;
                    let bars: Vec<crate::strategy::TechnicalDailyBar> = candles
                        .recent_traded()
                        .iter()
                        .map(|candle| crate::strategy::TechnicalDailyBar {
                            high: candle.high,
                            low: candle.low,
                            close: candle.close,
                        })
                        .collect();
                    Ok(build_technical_observation_from_recent_trades(
                        &bars,
                        candles.traded_count(),
                    ))
                })();
                (code.clone(), observation)
            })
            .collect::<Vec<_>>();
        // 股票任务完成顺序可不同；仍按 StockCode 选择首个错误并构造规范结果 map。
        prepared
            .into_iter()
            .map(|(code, result)| Ok((code, result?)))
            .collect()
    }
    fn exposed_stocks(
        session: &GameSession,
        now: crate::calendar::CivilInstant,
    ) -> BTreeSet<StockCode> {
        let mut threshold = now.date();
        for _ in 0..EXPOSURE_FRESHNESS_DAYS {
            threshold = threshold
                .prev()
                .expect("runtime window stays above the civil floor");
        }
        let mut exposed = BTreeSet::new();
        for stock in &session.state.setup.stocks {
            let Some(company) = session.state.company_registry.issuer_of(&stock.code) else {
                continue;
            };
            let fresh = session
                .state
                .library
                .reports_for_company(company, now)
                .iter()
                .any(|report| report.published_at.date() >= threshold)
                || session
                    .state
                    .library
                    .announcements_for_company(company, now)
                    .iter()
                    .any(|announcement| announcement.published_at.date() >= threshold);
            if fresh {
                exposed.insert(stock.code.clone());
            }
        }
        exposed
    }
}

struct CandidateObservations<'a> {
    stock: &'a StockView,
    price_path: Option<&'a crate::observation::PricePathObservation>,
    technical: Option<&'a TechnicalObservation>,
}

fn apply_institution_experience_feedback(belief: &mut BeliefBook, trading_day: u64) {
    let failures = belief
        .experience()
        .feedback
        .failure_events
        .iter()
        .filter_map(|event| {
            event
                .order_id
                .map(|order| (event.moment, event.code.clone(), OrderId(order), false))
        });
    let profits = belief
        .experience()
        .feedback
        .exit_records
        .iter()
        .filter(|exit| exit.realized_profit)
        .filter_map(|exit| {
            exit.order_id
                .map(|order| (exit.moment, exit.code.clone(), OrderId(order), true))
        });
    let mut events: Vec<_> = failures
        .chain(profits)
        .filter(|(_, code, order, _)| {
            belief
                .entry(code)
                .is_some_and(|entry| !entry.applied_experience_orders.contains(&order.0))
        })
        .collect();
    events.sort_by_key(|(moment, _, order, _)| {
        (
            moment.civil_date,
            moment.market_minute,
            moment.trading_day,
            *order,
        )
    });
    for (_, code, order, profitable) in events {
        if belief
            .entry(&code)
            .is_some_and(|entry| !entry.applied_experience_orders.contains(&order.0))
        {
            let cause = if profitable {
                BeliefCause::ProfitableExit { order }
            } else {
                BeliefCause::ExperienceFailure { order }
            };
            belief
                .apply_experience(
                    &code,
                    cause,
                    order,
                    if profitable { 500 } else { -1_000 },
                    trading_day,
                )
                .unwrap_or_else(|error| {
                    panic!("institution experience confidence failed for {code:?}: {error}")
                });
        }
    }
}

#[cfg(feature = "simulation-diagnostics")]
pub(in crate::session) struct PlanRootDiagnostics {
    facts: Vec<crate::diagnostics::causal::CausalFactKind>,
    reports: Vec<(StockCode, PublicationId)>,
    candidates: BTreeSet<StockCode>,
}

#[cfg(not(feature = "simulation-diagnostics"))]
pub(in crate::session) struct PlanRootDiagnostics;

/// FNV-1a(tag + account id) → 与 seed 混合的独立流种子（分析档案/信念假设等
/// per-NPC 一次性抽样专用；绝不与 self.state.rng 或注意力流共用）。
pub(super) fn derived_stream(seed: u64, tag: &str, id: AccountId) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in tag.bytes().chain(id.0.to_le_bytes()) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    seed ^ hash
}

/// 公告曝光新鲜度窗口（自然日）：公布日落在本窗口内的股票获得发现权重
/// 加成（版本化游戏假设；任务 25 `ANNOUNCEMENT_BOOST` 的消费面）。
const EXPOSURE_FRESHNESS_DAYS: u32 = 2;

/// 决策链只读诊断（验收测试面）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct DecisionChainDiagnostics {
    pub plan_count: usize,
    pub plans_per_stock: BTreeMap<StockCode, usize>,
    pub belief_accounts: usize,
    pub acquired_publications: usize,
    pub library_publications: usize,
    pub available_valuation_entries: BTreeMap<StockCode, usize>,
}

/// 信念条目概览（诊断/测试面）。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct BeliefDebugSummary {
    pub method: Option<String>,
    pub per_share_pessimistic_cents: i64,
    pub per_share_optimistic_cents: i64,
    pub confidence_bp: u16,
    pub unavailable_reason: Option<String>,
}

/// f64 收益率 → 整数 bp（None 透传）。
fn ratio_to_bp(ratio: Option<f64>) -> Option<i32> {
    ratio.map(|value| (value * 10_000.0).round() as i32)
}

/// 方向目标 → 净变动股数。买入按整手向下取整（不足一手无动作）；卖出
/// 允许零股（路由守卫按 A 股零股规则校验）。无净变动 ⇒ None。
fn desired_delta_shares(direction: Side, target_qty: u32, held_qty: u32, lot: u32) -> Option<u32> {
    let delta = match direction {
        Side::Buy => target_qty.saturating_sub(held_qty),
        Side::Sell => held_qty.saturating_sub(target_qty),
    };
    let delta = match direction {
        Side::Buy => delta - delta % lot.max(1),
        Side::Sell => delta,
    };
    (delta > 0).then_some(delta)
}

fn root_candidate_codes(
    account: AccountId,
    held: &BTreeSet<StockCode>,
    belief: &BeliefBook,
    plans: &PlanBook,
    discovered: Option<StockCode>,
) -> BTreeSet<StockCode> {
    let mut candidates = held.clone();
    candidates.extend(belief.entry_stocks().cloned());
    candidates.extend(plans.active_codes(account).cloned());
    candidates.extend(discovered);
    candidates
}

/// A lifecycle decision carries no plan-book write until the caller applies it. The current
/// caller applies immediately. These actions carry no state version: if a future path crosses
/// P3/P4 before applying them, it must observe current facts and collect again.
pub(in crate::session) enum PlanLifecycleAction {
    ExecutionState {
        plan_id: PlanId,
        event: PlanEvent,
    },
    Terminate {
        plan_id: PlanId,
        code: StockCode,
        child_order_id: Option<OrderId>,
        reason: TerminationReason,
        trading_day: u64,
    },
    Observe {
        plan_id: PlanId,
        trading_day: u64,
    },
    Revise {
        plan_id: PlanId,
        revision: PlanRevision,
    },
    Restructure {
        account: AccountId,
        plan_id: PlanId,
        code: StockCode,
        child_order_id: Option<OrderId>,
        terminating: bool,
        revision: PlanRevision,
    },
    Create {
        open: PlanOpen,
    },
}

pub(in crate::session) fn apply_plan_lifecycle_actions(
    actions: Vec<PlanLifecycleAction>,
    session: &GameSession,
    market: &MarketView,
    plans: &mut PlanBook,
    operations: &mut PlanChainOperationBatch,
) {
    for action in actions {
        let existing = match &action {
            PlanLifecycleAction::ExecutionState { plan_id, .. }
            | PlanLifecycleAction::Terminate { plan_id, .. }
            | PlanLifecycleAction::Observe { plan_id, .. }
            | PlanLifecycleAction::Revise { plan_id, .. }
            | PlanLifecycleAction::Restructure { plan_id, .. } => Some(*plan_id),
            PlanLifecycleAction::Create { .. } => None,
        };
        if let Some(plan_id) = existing {
            let plan = plans
                .plan(plan_id)
                .expect("lifecycle action must name a plan");
            let (price, acquired) = session.plan_review_facts(plan.account(), plan.code(), market);
            let resources = session.plan_review_resources(plan.account(), plan.code());
            plans
                .record_review(plan_id, u64::from(session.state.day), price, acquired)
                .unwrap_or_else(|error| panic!("plan review failed for {plan_id:?}: {error}"));
            plans
                .record_resource_review(plan_id, resources)
                .unwrap_or_else(|error| {
                    panic!("plan resource review failed for {plan_id:?}: {error}")
                });
        }
        match action {
            PlanLifecycleAction::Terminate {
                plan_id,
                child_order_id,
                reason,
                trading_day,
                ..
            } => {
                operations.push_termination(plan_id, child_order_id, reason, trading_day);
            }
            PlanLifecycleAction::ExecutionState { plan_id, event } => {
                let account = plans
                    .plan(plan_id)
                    .expect("execution state plan exists")
                    .account();
                let paused = matches!(event, PlanEvent::Paused { .. });
                plans.apply(plan_id, event).unwrap_or_else(|error| {
                    panic!("execution state failed for {plan_id:?}: {error}")
                });
                if paused {
                    operations.push_account_execution(account, market.clone());
                }
            }
            PlanLifecycleAction::Observe {
                plan_id,
                trading_day,
            } => {
                GameSession::observe_plan(plans, plan_id, trading_day);
            }
            PlanLifecycleAction::Revise { plan_id, revision } => {
                plans
                    .apply(plan_id, PlanEvent::Revised { revision })
                    .unwrap_or_else(|error| {
                        panic!("plan revision failed for {plan_id:?}: {error}")
                    });
            }
            PlanLifecycleAction::Restructure {
                account,
                plan_id,
                code,
                child_order_id,
                terminating,
                revision,
            } => operations.push_restructure(
                account,
                plan_id,
                code,
                child_order_id,
                terminating,
                revision,
            ),
            PlanLifecycleAction::Create { open } => {
                let account = open.account;
                let code = open.code.clone();
                let plan_id = plans.create(open).unwrap_or_else(|error| {
                    panic!("plan creation failed for {account:?} {code:?}: {error}")
                });
                let (price, acquired) = session.plan_review_facts(account, &code, market);
                let resources = session.plan_review_resources(account, &code);
                plans
                    .record_review(plan_id, u64::from(session.state.day), price, acquired)
                    .unwrap_or_else(|error| {
                        panic!("new plan review failed for {plan_id:?}: {error}")
                    });
                plans
                    .record_resource_review(plan_id, resources)
                    .unwrap_or_else(|error| {
                        panic!("new plan resource review failed for {plan_id:?}: {error}")
                    });
                let plan = plans.plan(plan_id).expect("newly created plan exists");
                let path = crate::observation::build_price_path_observation(
                    session
                        .state
                        .market_minute_closes
                        .get(&code)
                        .expect("new plan stock has minute history"),
                    &[],
                    None,
                )
                .expect("new plan public path is valid");
                let (assessment, _) = session.plan_execution_urgency_at_view(
                    plan,
                    ratio_to_bp(path.thirty_minute.return_ratio),
                    ratio_to_bp(path.one_minute.return_ratio),
                    market,
                );
                if let PauseAssessment::PauseAndRequestCancel(reason) = assessment.pause {
                    plans
                        .apply(
                            plan_id,
                            PlanEvent::Paused {
                                reason,
                                trading_day: u64::from(session.state.day),
                            },
                        )
                        .unwrap_or_else(|error| {
                            panic!("new institution plan pause failed: {error}")
                        });
                }
            }
        }
    }
}

impl GameSession {
    fn plan_review_resources(
        &self,
        account: AccountId,
        code: &StockCode,
    ) -> crate::plans::ReviewResources {
        let own = &self.state.accounts[&account];
        let position = own.positions().get(code);
        crate::plans::ReviewResources {
            cash: own.cash(),
            frozen_cash: self
                .reserved_cash_for_account(account)
                .unwrap_or_else(|error| panic!("personal review frozen cash failed: {error}")),
            held_qty: position.map_or(0, |holding| holding.qty()),
            t1_locked: position.map_or(0, |holding| holding.t1_locked()),
        }
    }

    /// Captures the complete root domain before the first P4 operation. The batch is lazy:
    /// account discovery/lifecycle/quotes run only when the private coordinator visits that root.
    pub(in crate::session) fn capture_decision_chain_roots(
        &self,
        accepted_due_npc_ids: &[AccountId],
        snapshot: &crate::session::pipeline::DecisionSnapshot,
    ) -> Result<PlanChainOperationBatch, StepFatal> {
        let invariant = |description: String| StepFatal::InvariantViolation {
            description,
            location: "decision_chain::capture_decision_chain_roots".to_owned(),
        };
        if snapshot.tick() != self.state.tick
            || snapshot.market_minute() != self.current_market_minute()
            || snapshot.phase() != self.phase()
        {
            return Err(invariant(
                "plan-chain P1 observation clock does not match tick".to_owned(),
            ));
        }
        if accepted_due_npc_ids
            .windows(2)
            .any(|pair| pair[0] >= pair[1])
        {
            return Err(invariant(
                "plan-chain root accounts are not canonical and unique".to_owned(),
            ));
        }
        for id in accepted_due_npc_ids {
            snapshot
                .account(*id)
                .map_err(|error| invariant(error.to_string()))?;
        }
        self.capture_decision_chain_roots_with_market(accepted_due_npc_ids, || {
            snapshot.market().clone()
        })
    }

    pub(in crate::session) fn capture_ready_decision_chain_roots(
        &self,
        observed_accounts: &[AccountId],
    ) -> Result<PlanChainOperationBatch, StepFatal> {
        if observed_accounts.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(StepFatal::InvariantViolation {
                description: "observed NPC accounts are not unique".to_owned(),
                location: "decision_chain::capture_ready_decision_chain_roots".to_owned(),
            });
        }
        let mut ready = observed_accounts.iter().copied().collect::<BTreeSet<_>>();
        if self.is_open_review_tick() {
            ready.extend(self.state.accounts.iter().filter_map(|(id, account)| {
                account.strategy().and_then(|strategy| {
                    strategy
                        .belief_chain_params()
                        .and_then(|params| params.daily_plan_review.then_some(*id))
                })
            }));
        }
        for account in self.state.plans.active_accounts() {
            if self.active_plan_requires_review(account) {
                ready.insert(account);
            }
        }
        let ready = ready.into_iter().collect::<Vec<_>>();
        self.capture_decision_chain_roots_with_market(&ready, || self.build_market_view())
    }

    fn plan_review_facts(
        &self,
        account: AccountId,
        code: &StockCode,
        market: &MarketView,
    ) -> (crate::Money, u32) {
        let price = market
            .stocks
            .get(code)
            .unwrap_or_else(|| panic!("reviewed stock {code:?} has no market view"))
            .last_price;
        let acquired = self
            .state
            .company_registry
            .issuer_of(code)
            .and_then(|company| {
                self.state
                    .belief_participants
                    .get(&account)
                    .map(|participant| participant.information())
                    .map(|known| known.records_for_company(company).len())
            })
            .unwrap_or(0);
        (
            price,
            u32::try_from(acquired).expect("publication ids fit in u32"),
        )
    }

    fn active_plan_requires_review(&self, account: AccountId) -> bool {
        let Some(strategy) = self
            .state
            .accounts
            .get(&account)
            .and_then(|owner| owner.strategy())
        else {
            return false;
        };
        if strategy.belief_chain_params().is_none() {
            return false;
        }
        self.state
            .plans
            .active_plan_ids_for_account(account)
            .into_iter()
            .any(|id| {
                let plan = self
                    .state
                    .plans
                    .plan(id)
                    .expect("active index must resolve");
                if u64::from(self.state.day) > plan.last_valid_trading_day() {
                    return false;
                }
                let Some(market) = self.state.markets.get(plan.code()) else {
                    return false;
                };
                let price = market.last_price();
                if plan.review().last_review_price.is_none_or(|baseline| {
                    (i128::from(price.cents()) - i128::from(baseline.cents())).abs() * 10_000
                        >= i128::from(baseline.cents())
                            * i128::from(plan.review().min_price_change_bp)
                }) {
                    return true;
                }
                let known = self
                    .state
                    .company_registry
                    .issuer_of(plan.code())
                    .and_then(|company| {
                        self.state
                            .belief_participants
                            .get(&account)
                            .map(|participant| participant.information())
                            .map(|state| state.records_for_company(company).len())
                    })
                    .unwrap_or(0);
                known > plan.review().last_review_acquired_count as usize
            })
    }

    fn is_open_review_tick(&self) -> bool {
        self.state.tick % self.state.setup.ticks_per_day == self.state.setup.auction_ticks
    }

    fn capture_decision_chain_roots_with_market(
        &self,
        observed_accounts: &[AccountId],
        market: impl FnOnce() -> MarketView,
    ) -> Result<PlanChainOperationBatch, StepFatal> {
        let invariant = |description: String| StepFatal::InvariantViolation {
            description,
            location: "decision_chain::capture_decision_chain_roots_with_market".to_owned(),
        };
        if observed_accounts.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(invariant("observed NPC accounts are not unique".to_owned()));
        }
        let mut accounts = Vec::new();
        for id in observed_accounts {
            let account =
                self.state.accounts.get(id).ok_or_else(|| {
                    invariant(format!("plan-chain root account {id:?} is absent"))
                })?;
            if account
                .strategy()
                .is_some_and(|strategy| strategy.belief_chain_params().is_some())
            {
                accounts.push(*id);
            }
        }
        if accounts.is_empty() {
            return Ok(PlanChainOperationBatch::empty());
        }
        Ok(PlanChainOperationBatch::from_observation(
            accounts,
            DecisionChainObservation::capture_for_roots(self, market())?,
        ))
    }

    /// step 串行段的决策链入口：对本次 accepted 注意力中的信念机构账户执行
    /// 完整链条。事件（成交/接受/撤销/拒绝）按链内顺序追加进 step 事件流。
    #[cfg(test)]
    pub(super) fn run_decision_chain(&mut self, npc_ids: &[AccountId]) -> PlanChainOperationBatch {
        // 能力探针是唯一入口判据：只有暴露链参数的信念机构策略进链（测试
        // 替身换掉策略后即退出链，状态面不单独驱动行为）。
        let belief_ids: Vec<AccountId> = npc_ids
            .iter()
            .filter(|id| {
                self.state
                    .accounts
                    .get(id)
                    .and_then(|account| account.strategy())
                    .is_some_and(|strategy| strategy.belief_chain_params().is_some())
            })
            .copied()
            .collect();
        if belief_ids.is_empty() {
            return PlanChainOperationBatch::empty();
        }
        let market_view = self.build_market_view();
        let price_paths = self
            .market_price_path_observations()
            .unwrap_or_else(|error| panic!("decision chain price paths failed: {error}"));
        let technical = self
            .build_chain_technical_observations()
            .unwrap_or_else(|error| {
                panic!("decision chain technical observations failed: {error}")
            });
        let now = self.chain_observation_instant();
        let exposed = self.chain_exposed_stocks(now);
        let mut operations = PlanChainOperationBatch::accounts(
            belief_ids,
            market_view,
            price_paths,
            technical,
            now,
            exposed,
        );
        while operations
            .prepare_ready_accounts(self, true)
            .expect("test plan roots must prepare")
        {}
        operations
    }

    /// 技术指标只读取最近所需的有效日 K。完整日 K 已在建局、读档和每日追加时
    /// 保留；指标只依赖最后 60 个有成交样本，不随多年历史重复扫描。
    fn build_chain_technical_observations(
        &self,
    ) -> Result<BTreeMap<StockCode, TechnicalObservation>, StepFatal> {
        DecisionChainObservation::build_technical(self)
    }

    #[cfg(test)]
    fn chain_observation_instant(&self) -> crate::calendar::CivilInstant {
        self.observation_civil_instant()
    }

    /// 曝光股票集合：公布/公告落在新鲜度窗口内的发行人股票（任务 25 发现
    /// 权重的公告加成输入；公司 ↔ 股票映射 = 发行人注册表）。
    #[cfg(test)]
    fn chain_exposed_stocks(&self, now: crate::calendar::CivilInstant) -> BTreeSet<StockCode> {
        DecisionChainObservation::exposed_stocks(self, now)
    }

    #[cfg(feature = "simulation-diagnostics")]
    pub(in crate::session) fn record_plan_root_diagnostics(
        &mut self,
        id: AccountId,
        diagnostics: PlanRootDiagnostics,
        plans: &PlanBook,
    ) {
        let time = self.causal_time();
        self.state
            .causal
            .record_plan_root(time, id, diagnostics.facts);
        self.record_npc_decision_trace(
            id,
            &diagnostics.reports,
            &diagnostics.candidates,
            plans,
            &[],
        );
    }

    #[cfg(feature = "simulation-diagnostics")]
    fn record_npc_decision_trace(
        &mut self,
        account: AccountId,
        reports: &[(StockCode, PublicationId)],
        candidates: &BTreeSet<StockCode>,
        plans: &PlanBook,
        events: &[Event],
    ) {
        use crate::diagnostics::NpcDecisionTraceRecord;

        let plan_ids = plans
            .plan_ids()
            .filter(|plan_id| {
                plans
                    .plan(*plan_id)
                    .is_ok_and(|plan| plan.account() == account)
            })
            .collect();
        let expectation_method = self
            .state
            .belief_participants
            .get(&account)
            .map(|participant| participant.belief())
            .and_then(|belief| candidates.iter().find_map(|code| belief.entry(code)))
            .and_then(|entry| entry.method)
            .map(|method| format!("{method:?}"));
        let order_ids = events
            .iter()
            .filter_map(|event| match event {
                Event::OrderAccepted {
                    account: owner, id, ..
                } if *owner == account => Some(*id),
                Event::OrderCanceled {
                    account: owner, id, ..
                } if *owner == account => Some(*id),
                _ => None,
            })
            .collect();
        let budget_constraints = plans
            .plan_ids()
            .filter_map(|plan_id| plans.plan(plan_id).ok())
            .filter(|plan| plan.account() == account && plan.is_terminal())
            .map(|plan| format!("{:?}", plan.status()))
            .collect();
        self.state
            .npc_decision_traces
            .record(NpcDecisionTraceRecord {
                account,
                tick: self.state.tick,
                source_report_ids: reports
                    .iter()
                    .map(|(_, report)| report.value().to_string())
                    .collect(),
                expectation_method,
                plan_ids,
                budget_constraints,
                order_ids,
                codes: candidates.iter().cloned().collect(),
            });
    }

    /// 计划生命周期驱动：新开/修订/平静观察。需要基本面的策略在
    /// 个人估值不可用时不产生新方向性动作；零基本面权重的策略用其他信号。
    ///
    /// 日中终止/反向修订前必须先真实撤销在途子单（task-24 复核移交项）：
    /// 见 [`Self::cancel_in_flight_child_before_restructure`]。
    pub(in crate::session) fn collect_plan_lifecycle_actions(
        &self,
        id: AccountId,
        assessments: &BTreeMap<StockCode, CandidateAssessment>,
        market_view: &MarketView,
        plans: &PlanBook,
    ) -> Vec<PlanLifecycleAction> {
        lifecycle::PlanLifecycleReview::capture(self, id, market_view, plans)
            .map_or_else(Vec::new, |review| review.collect(assessments))
    }

    /// 平静观察（信息/风险/约束无变化）：仅推进最近事件日。
    pub(in crate::session) fn observe_plan(
        plans: &mut PlanBook,
        plan_id: PlanId,
        trading_day: u64,
    ) {
        plans
            .apply(plan_id, PlanEvent::ObservedNoChange { trading_day })
            .unwrap_or_else(|error| panic!("plan observation failed for {plan_id:?}: {error}"));
    }

    /// 信念机构的链参数（能力探针；非信念策略在调用方已被过滤）。
    fn chain_strategy_params(&self, id: AccountId) -> crate::strategy::BeliefChainParams {
        self.state
            .accounts
            .get(&id)
            .and_then(|account| account.strategy())
            .and_then(|strategy| strategy.belief_chain_params())
            .unwrap_or_else(|| panic!("belief account {id:?} must expose chain params"))
    }

    /// 为本账户的非终止 ShareCount 计划计算软预算和报价输入。
    /// 调用方先应用已经收到的计划事实；这里仅观察会话并返回账户私有游标。
    pub(in crate::session) fn prepare_plan_quotes_for_account(
        &self,
        id: AccountId,
        market_view: &MarketView,
        plans: &PlanBook,
    ) -> Option<super::plan_chain_candidates::QuotePlans> {
        let active_plans: Vec<TradingPlan> = plans
            .active_plan_ids_for_account(id)
            .into_iter()
            .map(|plan_id| {
                plans
                    .plan(plan_id)
                    .expect("collected plan ids must resolve")
                    .clone()
            })
            .collect();
        if active_plans.is_empty() {
            return None;
        }
        let chain_params = self.chain_strategy_params(id);
        // 账户事实先取局部值，供此账户的预算与报价游标共用。
        let account_cash = self.state.accounts[&id].cash();
        let sellable_by_code: BTreeMap<StockCode, u32> = self.state.accounts[&id]
            .positions()
            .keys()
            .map(|code| (code.clone(), self.state.accounts[&id].sellable_qty(code)))
            .collect();
        // 6. 软预算（权威现金 = 账户现金；在途冻结单独传入，不由分配器重复计）。
        let frozen = self
            .reserved_cash_for_account(id)
            .unwrap_or_else(|error| panic!("reserved cash for {id:?} failed: {error}"));
        let funds = AllocationFunds {
            cash: account_cash,
            frozen_cash: frozen,
        };
        let requests: Vec<AllocationRequest> = active_plans
            .iter()
            .filter(|plan| matches!(plan.status(), PlanStatus::Active))
            .filter_map(|plan| {
                let remaining = plan.remaining_share_qty()?;
                match plan.direction() {
                    Side::Buy => {
                        let stock = self
                            .state
                            .setup
                            .stocks
                            .iter()
                            .find(|stock| &stock.code == plan.code())
                            .unwrap_or_else(|| {
                                panic!("plan stock {:?} must have a spec", plan.code())
                            });
                        // Quotes may move to any legal buy limit up to the daily upper band.
                        // Reserve against that upper bound, but only for the next routable child;
                        // the remaining parent target is not a live order or a fee obligation.
                        let reservation_price = self
                            .state
                            .markets
                            .get(plan.code())
                            .unwrap_or_else(|| {
                                panic!("plan stock {:?} must have a market", plan.code())
                            })
                            .up_stop()
                            .unwrap_or_else(|error| {
                                panic!("up stop failed for {:?}: {error}", plan.code())
                            });
                        buy_allocation_request(
                            &self.state.setup.config,
                            plan.plan_id(),
                            plan.code().clone(),
                            plan.confidence_bp(),
                            reservation_price,
                            remaining,
                            chain_params.order_size,
                            self.state.setup.config.lot_size,
                            stock.category.max_order_qty(false),
                        )
                    }
                    Side::Sell => {
                        let sellable = sellable_by_code.get(plan.code()).copied().unwrap_or(0);
                        let stock = self
                            .state
                            .setup
                            .stocks
                            .iter()
                            .find(|stock| &stock.code == plan.code())
                            .unwrap_or_else(|| {
                                panic!("plan stock {:?} must have a spec", plan.code())
                            });
                        let qty = next_routable_sell_qty(
                            remaining,
                            sellable,
                            self.state.setup.config.lot_size,
                            stock.category.max_order_qty(false),
                        )?;
                        Some(AllocationRequest {
                            plan_id: plan.plan_id(),
                            code: plan.code().clone(),
                            side: Side::Sell,
                            class: AllocationClass::ExistingPlan,
                            confidence_bp: plan.confidence_bp(),
                            requested_cash: Money::ZERO,
                            fee_reserve: Money::ZERO,
                            requested_sell_qty: qty,
                            sellable_qty: sellable,
                            experience: AllocationExperience::default(),
                        })
                    }
                }
            })
            .collect();
        let grants: Option<AllocationResult> = if requests.is_empty() {
            None
        } else {
            Some(
                allocate_child_quote_budgets(
                    &funds,
                    &requests,
                    self.state.setup.config.commission_min,
                )
                .unwrap_or_else(|error| panic!("allocation failed for {id:?}: {error}")),
            )
        };
        // 报价游标逐计划使用这些预算；真实路由由候选批协调。
        let one_minute_bp: BTreeMap<StockCode, Option<i32>> = self
            .market_price_path_observations()
            .expect("price paths must build for urgency inputs")
            .iter()
            .map(|(code, path)| (code.clone(), ratio_to_bp(path.one_minute.return_ratio)))
            .collect();
        let thirty_minute_bp: BTreeMap<StockCode, Option<i32>> = self
            .market_price_path_observations()
            .expect("price paths must build for urgency inputs")
            .iter()
            .map(|(code, path)| (code.clone(), ratio_to_bp(path.thirty_minute.return_ratio)))
            .collect();
        Some(super::plan_chain_candidates::QuotePlans {
            account: id,
            market: market_view.clone(),
            plans: active_plans
                .into_iter()
                .map(|plan| plan.plan_id())
                .collect(),
            grants,
            sellable: sellable_by_code,
            one_minute_bp,
            thirty_minute_bp,
        })
    }

    /// 账户的机构风格（信念壳身份）。
    fn belief_style(&self, id: AccountId) -> Option<crate::strategy::InstitutionStyle> {
        self.state
            .accounts
            .get(&id)
            .and_then(|account| account.strategy())
            .and_then(|strategy| strategy.institution_style())
    }

    /// 集合竞价委托快照（诊断/测试）：(side, price_cents, qty, owner) 列表。
    pub fn auction_orders_debug(&self, code: &StockCode) -> Vec<(Side, i64, u32, AccountId)> {
        self.state
            .auction_orders
            .get(code)
            .map(|orders| {
                orders
                    .iter()
                    .map(|order| (order.side, order.limit.cents(), order.qty, order.owner))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// 单账户单股的 K5a 评估（诊断/测试）。
    pub fn assessment_debug(
        &self,
        account: AccountId,
        code: &StockCode,
    ) -> Result<Option<String>, String> {
        let market_view = self.build_market_view();
        let price_paths = self
            .market_price_path_observations()
            .map_err(|error| error.to_string())?;
        let technical = self
            .build_chain_technical_observations()
            .map_err(|error| error.to_string())?;
        let belief = self
            .state
            .belief_participants
            .get(&account)
            .map(|participant| participant.belief())
            .ok_or_else(|| format!("no belief book for {account:?}"))?;
        let weights = belief.analysis().weights();
        let Some(view) = market_view.stocks.get(code) else {
            return Ok(None);
        };
        let signals = RootReadContext::capture(self)
            .map_err(|error| error.to_string())?
            .build_candidate_signals(
                account,
                belief,
                code,
                &market_view,
                CandidateObservations {
                    stock: view,
                    price_path: price_paths.get(code),
                    technical: technical.get(code),
                },
            )
            .map_err(|error| error.to_string())?;
        let assessment = blend_candidate(&weights, &signals);
        Ok(Some(format!(
            "fund={:?} trend={:?} pv={:?} tech={:?} cost={:?} weights={:?}x5 -> {assessment:?}",
            signals.fundamental.score().map(|s| s.value()),
            signals.trend.score().map(|s| s.value()),
            signals.price_volume.score().map(|s| s.value()),
            signals.technical.score().map(|s| s.value()),
            signals.experience.score().map(|s| s.value()),
            [
                weights.fundamental_bp(),
                weights.trend_bp(),
                weights.price_volume_bp(),
                weights.technical_bp(),
                weights.experience_cost_bp()
            ],
        )))
    }

    /// 单账户信念条目概览（诊断/测试）：(method, per_share_pessimistic_cents,
    /// per_share_optimistic_cents, confidence_bp, unavailable_reason)。
    pub fn belief_debug(&self, account: AccountId, code: &StockCode) -> Option<BeliefDebugSummary> {
        let belief = self
            .state
            .belief_participants
            .get(&account)
            .map(|participant| participant.belief())?;
        let entry = belief.entry(code)?;
        let (pessimistic, optimistic) = match &entry.valuation {
            ValuationOutcome::Available { per_share, .. } => {
                (per_share.pessimistic.cents(), per_share.optimistic.cents())
            }
            ValuationOutcome::Unavailable { .. } => (-1, -2),
        };
        Some(BeliefDebugSummary {
            method: entry.method.map(|method| format!("{method:?}")),
            per_share_pessimistic_cents: pessimistic,
            per_share_optimistic_cents: optimistic,
            confidence_bp: entry.confidence_bp,
            unavailable_reason: match &entry.valuation {
                ValuationOutcome::Unavailable { reason } => Some(format!("{reason:?}")),
                ValuationOutcome::Available { .. } => None,
            },
        })
    }

    /// 计划概览（诊断/测试）：(account, code, direction, target, filled, status) 列表。
    pub fn plans_debug(&self) -> Vec<(AccountId, String, Side, u32, u32, String)> {
        self.state
            .plans
            .plan_ids()
            .filter_map(|plan_id| {
                let plan = self.state.plans.plan(plan_id).ok()?;
                Some((
                    plan.account(),
                    plan.code().0.clone(),
                    plan.direction(),
                    match plan.target() {
                        crate::plans::PlanTarget::ShareCount(qty) => qty,
                        crate::plans::PlanTarget::PositionFractionBp(_) => 0,
                    },
                    plan.filled_qty(),
                    format!("{:?}", plan.status()),
                ))
            })
            .collect()
    }

    /// 决策链诊断（只读；验收测试与离线对账用——不泄露任何隐藏市场信息，
    /// 共同 V 已不存在，这里只有链自身的状态计数与个人估值概览）。
    pub fn decision_chain_diagnostics(&self) -> DecisionChainDiagnostics {
        let mut per_stock: BTreeMap<StockCode, usize> = BTreeMap::new();
        let mut plan_count = 0_usize;
        for plan_id in self.state.plans.plan_ids() {
            if let Ok(plan) = self.state.plans.plan(plan_id) {
                plan_count += 1;
                *per_stock.entry(plan.code().clone()).or_default() += 1;
            }
        }
        let available_valuations: BTreeMap<StockCode, usize> = self
            .state
            .belief_participants
            .values()
            .map(|participant| participant.belief())
            .flat_map(|belief| belief.entry_stocks())
            .fold(BTreeMap::new(), |mut acc, code| {
                *acc.entry(code.clone()).or_default() += 1;
                acc
            });
        DecisionChainDiagnostics {
            plan_count,
            plans_per_stock: per_stock,
            belief_accounts: self.state.belief_participants.len(),
            acquired_publications: self
                .state
                .belief_participants
                .values()
                .map(|participant| participant.information())
                .map(|state| state.acquired_count())
                .sum(),
            library_publications: self.state.library.report_count()
                + self.state.library.announcement_count(),
            available_valuation_entries: available_valuations,
        }
    }

    /// 个人每股估值区间（无条目/不可用 ⇒ None）。
    fn belief_per_share(&self, id: AccountId, code: &StockCode) -> Option<PerShareRange> {
        let entry = self
            .state
            .belief_participants
            .get(&id)
            .map(|participant| participant.belief())?
            .entry(code)?;
        match &entry.valuation {
            ValuationOutcome::Available { per_share, .. } => Some(*per_share),
            ValuationOutcome::Unavailable { .. } => None,
        }
    }
}

/// Builds one buy-side soft-budget request for the next child that the quote policy may route.
///
/// A `TradingPlan` records a parent target and can span many child orders. Funds and fees are
/// therefore reserved only for the next board-lot child, at the greatest legal buy limit, never
/// for the complete unsubmitted parent remainder.
#[allow(
    clippy::too_many_arguments,
    reason = "keeps the existing child-order cash calculation explicit without changing plan routing"
)]
fn buy_allocation_request(
    config: &crate::config::GameConfig,
    plan_id: PlanId,
    code: StockCode,
    confidence_bp: u32,
    reservation_price: Money,
    remaining: u32,
    order_size: u32,
    lot_size: u32,
    max_order_qty: u32,
) -> Option<AllocationRequest> {
    let qty = next_routable_buy_qty(remaining, order_size, lot_size, max_order_qty)?;
    let requested_cash = reservation_price
        .mul_shares(qty)
        .unwrap_or_else(|error| panic!("plan child cash failed: {error}"));
    let total_reservation = buy_order_reservation(config, reservation_price, qty, Money::ZERO)
        .unwrap_or_else(|error| panic!("plan child reservation failed: {error}"));
    let fee_reserve = total_reservation
        .sub(requested_cash)
        .unwrap_or_else(|error| panic!("plan child fee extraction failed: {error}"));
    Some(AllocationRequest {
        plan_id,
        code,
        side: Side::Buy,
        class: AllocationClass::ExistingPlan,
        confidence_bp,
        requested_cash,
        fee_reserve,
        requested_sell_qty: 0,
        sellable_qty: 0,
        experience: AllocationExperience::default(),
    })
}

pub(in crate::session) fn next_routable_buy_qty(
    remaining: u32,
    order_size: u32,
    lot_size: u32,
    max_order_qty: u32,
) -> Option<u32> {
    assert!(
        lot_size > 0,
        "session configuration requires a positive lot size"
    );
    // An explicit sub-board-lot strategy size cannot be silently enlarged into an
    // executable A-share order. It instead has no routable child this tick.
    let capped = remaining.min(order_size).min(max_order_qty);
    let qty = capped - capped % lot_size;
    (qty > 0).then_some(qty)
}

/// Returns the next legal A-share sell child.  A partial reduction cannot split or carry an
/// odd-lot remainder; the sole exception is an order that disposes of the entire available
/// sellable balance, which may carry its odd lot exactly once.
pub(in crate::session) fn next_routable_sell_qty(
    remaining: u32,
    sellable: u32,
    lot_size: u32,
    max_order_qty: u32,
) -> Option<u32> {
    assert!(
        lot_size > 0,
        "session configuration requires a positive lot size"
    );
    let capped = remaining.min(sellable).min(max_order_qty);
    if capped == sellable {
        return (capped > 0).then_some(capped);
    }
    let qty = capped - capped % lot_size;
    (qty > 0).then_some(qty)
}

#[cfg(test)]
mod chain_restructure_tests {
    #[test]
    fn institution_active_buy_withdraws_when_its_positive_opinion_ends() {
        for score in [0, -1_000] {
            let mut session = seeded_buy_plan_with_child();
            let code = StockCode("000812".to_owned());
            let before = session
                .state
                .plans
                .active_plan(AccountId(1), &code)
                .unwrap()
                .clone();
            let child = before.active_child_order_id().unwrap();
            let events = drive_once(&mut session, &reversal_assessment(score));
            let after = session.state.plans.plan(before.plan_id()).unwrap();
            assert_eq!(
                after.status(),
                PlanStatus::Terminated {
                    reason: TerminationReason::Cancelled
                }
            );
            assert_eq!(after.filled_qty(), before.filled_qty());
            assert!(events
                .iter()
                .any(|event| matches!(event, Event::OrderCanceled { id, .. } if *id == child)));
            assert!(!events.iter().any(|event| matches!(event, Event::OrderAccepted { account, .. } if *account == AccountId(1))));
        }
    }

    #[test]
    fn institution_small_signal_changes_do_not_revise_a_quiet_plan() {
        let mut session = seeded_buy_plan_with_child();
        let code = StockCode("000812".to_owned());
        let id = session
            .state
            .plans
            .active_plan(AccountId(1), &code)
            .unwrap()
            .plan_id();
        let resources = session.plan_review_resources(AccountId(1), &code);
        session
            .state
            .plans
            .record_resource_review(id, resources)
            .unwrap();
        let plan = session
            .state
            .plans
            .active_plan(AccountId(1), &StockCode("000812".to_owned()))
            .unwrap();
        let score = plan.opinion().signal_score_bp - 1;
        assert!(score >= session.state.plans.policy().reverse_revision_threshold_bp);
        let actions = session.collect_plan_lifecycle_actions(
            AccountId(1),
            &reversal_assessment(score),
            &session.build_market_view(),
            &session.state.plans,
        );
        assert!(
            matches!(actions.as_slice(), [PlanLifecycleAction::Observe { .. }]),
            "one bp of score noise cannot bypass the saved review threshold"
        );
    }

    #[test]
    fn institution_revisions_preserve_the_assessed_patience() {
        let session = seeded_buy_plan_with_child();
        let actions = session.collect_plan_lifecycle_actions(
            AccountId(1),
            &reversal_assessment(-8_000),
            &session.build_market_view(),
            &session.state.plans,
        );
        assert!(
            actions.iter().any(|action| match action {
                PlanLifecycleAction::Revise { revision, .. }
                | PlanLifecycleAction::Restructure { revision, .. } =>
                    revision.urgency == Urgency::Patient,
                _ => false,
            }),
            "a DeepValue revision must not overwrite assessed urgency with Normal"
        );
    }

    #[test]
    fn institution_known_drawdown_does_not_clear_a_still_triggered_risk_pause() {
        let mut session = seeded_buy_plan_with_child();
        let account = AccountId(1);
        let plan_id = session
            .state
            .plans
            .active_plan(account, &StockCode("000812".to_owned()))
            .unwrap()
            .plan_id();
        let equity = session.account_equity(account).unwrap();
        session
            .state
            .belief_participants
            .get_mut(&account)
            .map(|participant| participant.belief_mut())
            .unwrap()
            .experience_mut()
            .observe_equity(Money::from_cents(equity.cents() * 2))
            .unwrap();
        session
            .state
            .plans
            .apply(
                plan_id,
                PlanEvent::Paused {
                    reason: crate::plans::PauseReason::RiskPressure,
                    trading_day: 0,
                },
            )
            .unwrap();
        set_urgency_minutes(&mut session, false);
        observe_account_risk_for_review(&mut session, account);
        let actions = session.collect_plan_lifecycle_actions(
            account,
            &reversal_assessment(8_000),
            &session.build_market_view(),
            &session.state.plans,
        );
        assert!(
            matches!(actions.as_slice(), [PlanLifecycleAction::Observe { .. }]),
            "a still-triggered personal drawdown must not resume a paused buy"
        );
    }

    #[test]
    fn institution_risk_pressure_cancels_buy_and_recovers_only_after_own_review() {
        let mut session = seeded_buy_plan_with_child();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let before = session
            .state
            .plans
            .active_plan(account, &code)
            .unwrap()
            .clone();
        let child = before.active_child_order_id().unwrap();
        let equity = session.account_equity(account).unwrap();
        session
            .state
            .belief_participants
            .get_mut(&account)
            .map(|participant| participant.belief_mut())
            .unwrap()
            .experience_mut()
            .observe_equity(Money::from_cents(equity.cents() * 2))
            .unwrap();
        set_urgency_minutes(&mut session, false);
        let events = drive_once(&mut session, &reversal_assessment(8_000));
        assert_eq!(
            session.state.plans.plan(before.plan_id()).unwrap().status(),
            PlanStatus::Paused {
                reason: crate::plans::PauseReason::RiskPressure
            }
        );
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::OrderCanceled { id, .. } if *id == child)));
        assert_eq!(
            session
                .state
                .plans
                .plan(before.plan_id())
                .unwrap()
                .filled_qty(),
            before.filled_qty()
        );
        let peak = session.state.belief_participants[&account]
            .belief()
            .experience()
            .peak_equity
            .unwrap();
        let restored_cash = session.state.accounts[&account].cash().add(peak).unwrap();
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_cash(restored_cash);
        assert!(matches!(
            session.state.plans.plan(before.plan_id()).unwrap().status(),
            PlanStatus::Paused { .. }
        ));
        drive_once(&mut session, &reversal_assessment(8_000));
        let reviewed = session.state.plans.plan(before.plan_id()).unwrap();
        assert_eq!(reviewed.status(), PlanStatus::Active);
        assert_eq!(reviewed.last_resume(), Some(ResumeReason::TriggerCleared));
        assert_eq!(reviewed.direction(), Side::Buy);
    }

    #[test]
    fn institution_existing_reduction_uses_its_own_observed_drawdown() {
        let mut session = probe_session();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let equity = session.account_equity(account).unwrap();
        session
            .state
            .belief_participants
            .get_mut(&account)
            .map(|participant| participant.belief_mut())
            .unwrap()
            .experience_mut()
            .observe_equity(Money::from_cents(equity.cents() * 5 / 4))
            .unwrap();
        let plan_id = session
            .state
            .plans
            .create(PlanOpen {
                account,
                code,
                direction: Side::Sell,
                target: PlanTarget::ShareCount(100),
                opinion: PlanOpinion {
                    signal_score_bp: -8_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 6_000,
                urgency: Urgency::Normal,
                horizon_trading_days: 20,
                created_trading_day: 0,
            })
            .unwrap();
        let plan = session.state.plans.plan(plan_id).unwrap();
        let (urgency, risk) = session.plan_execution_urgency(plan, Some(0), Some(0));
        assert_eq!(urgency.urgency, Urgency::Urgent);
        assert!(matches!(
            risk,
            crate::plans::urgency::risk::RiskUrgencyAssessment::Assessed {
                risk_reduction_active: true,
                account_drawdown_bp: Some(2_000),
                ..
            }
        ));
        let buy = plan.review_preview(Side::Buy, plan.confidence_bp());
        let (buy_urgency, risk) = session.plan_execution_urgency(&buy, Some(0), Some(0));
        assert_eq!(
            buy_urgency.urgency,
            Urgency::Patient,
            "drawdown must not impose a sale or universal stop-loss"
        );
        assert!(matches!(
            risk,
            crate::plans::urgency::risk::RiskUrgencyAssessment::Assessed {
                risk_reduction_active: false,
                ..
            }
        ));
        assert_eq!(
            session.state.plans.plan(plan_id).unwrap().direction(),
            Side::Sell
        );
    }

    #[test]
    fn institution_observation_records_only_its_private_equity_and_holding_facts() {
        let mut session = probe_session();
        let account = AccountId(1);
        let expected = session.account_equity(account).unwrap();
        assert_eq!(
            session.state.belief_participants[&account]
                .belief()
                .experience()
                .peak_equity,
            None
        );
        force_attention(&mut session, account);
        session.state.pending_npc = None;
        crate::session::pipeline::queue_npc_for_next_tick(&mut session).unwrap();
        session.step().unwrap();
        let experience = session.state.belief_participants[&account]
            .belief()
            .experience();
        assert_eq!(experience.reference_equity, Some(expected));
        assert_eq!(experience.peak_equity, Some(expected));
        assert!(!experience.stocks.is_empty());
        assert!(
            experience
                .stocks
                .values()
                .all(|stock| stock.last_buy_order_id.is_none()),
            "initial holdings are not fabricated fills"
        );
    }

    #[test]
    fn institution_net_profitable_exit_updates_confidence_once_on_own_review() {
        let session = seeded_buy_plan_with_child();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let mut belief = session.state.belief_participants[&account].belief().clone();
        let before = belief.entry(&code).unwrap().clone();
        belief
            .experience_mut()
            .feedback
            .exit_records
            .push(crate::experience::ExitRecord {
                code: code.clone(),
                order_id: Some(999),
                cooldown_until_market_minute: None,
                realized_profit: true,
                moment: crate::experience::ExperienceMoment {
                    civil_date: session.civil_date(),
                    market_minute: session.current_market_minute(),
                    trading_day: u64::from(session.state.day),
                },
            });
        apply_institution_experience_feedback(&mut belief, u64::from(session.state.day));
        let after = belief.entry(&code).unwrap();
        assert_eq!(
            after.confidence_bp,
            (before.confidence_bp + 500).min(10_000)
        );
        assert_eq!(after.valuation, before.valuation);
        assert_eq!(after.forecast, before.forecast);
        assert!(after.applied_experience_orders.contains(&999));
        let once = serde_json::to_vec(&belief).unwrap();
        apply_institution_experience_feedback(&mut belief, u64::from(session.state.day));
        assert_eq!(serde_json::to_vec(&belief).unwrap(), once);
    }

    #[test]
    fn institution_new_direction_requires_the_confirmed_signal_threshold() {
        let mut session = probe_session();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let style = crate::strategy::InstitutionStyle::ActiveTrader;
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_strategy(Some(crate::account::StoredStrategy::production(Box::new(
                crate::strategy::BeliefInstitutionStrategy::new(0.05, 100)
                    .unwrap()
                    .with_institution_style(style),
            ))));
        let owner = session.state.accounts.get_mut(&account).unwrap();
        owner.fixture_set_cash(Money::from_cents(10_000_000));
        owner.fixture_insert_position(
            code.clone(),
            crate::Position::from_restored_parts(20_000, 0, 5_700_000, 0),
        );
        let profile = crate::strategy::StrategyProfile::Institution(style);
        let mut rng = crate::session::SplitMix64::new(17);
        let analysis =
            crate::strategy::derive_analysis_profile(&profile, account, &mut rng).unwrap();
        *session
            .state
            .belief_participants
            .get_mut(&account)
            .expect("fixture institution participant exists")
            .belief_mut() = crate::strategy::BeliefBook::new(account, profile, analysis, &mut rng);
        let market = session.build_market_view();
        for score in [-1_999, -1, 0, 1, 1_999] {
            let actions = session.collect_plan_lifecycle_actions(
                account,
                &reversal_assessment(score),
                &market,
                &session.state.plans,
            );
            assert!(
                actions.is_empty(),
                "subthreshold score {score} cannot open a direction"
            );
        }
        for (score, direction) in [(-2_000, Side::Sell), (2_000, Side::Buy)] {
            let actions = session.collect_plan_lifecycle_actions(
                account,
                &reversal_assessment(score),
                &market,
                &session.state.plans,
            );
            assert!(
                matches!(actions.as_slice(), [PlanLifecycleAction::Create { open }] if open.direction == direction && open.target != PlanTarget::ShareCount(0))
            );
        }
    }

    #[test]
    fn institution_initial_plan_records_its_personal_execution_urgency() {
        let mut session = seeded_buy_plan_with_child();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let original = session
            .state
            .plans
            .active_plan(account, &code)
            .unwrap()
            .clone();
        session.state.plans = PlanBook::default();
        let actions = session.collect_plan_lifecycle_actions(
            account,
            &reversal_assessment(8_000),
            &session.build_market_view(),
            &session.state.plans,
        );
        assert!(
            matches!(actions.as_slice(), [PlanLifecycleAction::Create { open }] if open.urgency == Urgency::Patient && open.horizon_trading_days > 1),
            "DeepValue must persist patient urgency at plan creation"
        );
        assert_eq!(original.account(), account);
    }

    #[test]
    fn institution_frozen_policy_and_historical_review_resources_survive_restore() {
        let mut session = probe_session();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let policy = crate::strategy::InstitutionExperiencePolicy::new(
            1,
            crate::strategy::InstitutionLossResponse::PauseAndReview,
            900,
            1_800,
            3_100,
            1_300,
            2,
            700,
        )
        .unwrap();
        session
            .state
            .belief_participants
            .get_mut(&account)
            .map(|participant| participant.belief_mut())
            .unwrap()
            .set_institution_policy(policy);
        let plan_id = session
            .state
            .plans
            .create(PlanOpen {
                account,
                code,
                direction: Side::Buy,
                target: PlanTarget::ShareCount(100),
                opinion: PlanOpinion {
                    signal_score_bp: 8_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 6_000,
                urgency: Urgency::Patient,
                horizon_trading_days: 20,
                created_trading_day: 0,
            })
            .unwrap();
        let history = crate::plans::ReviewResources {
            cash: Money::from_cents(500),
            frozen_cash: Money::from_cents(100),
            held_qty: 200,
            t1_locked: 100,
        };
        session
            .state
            .plans
            .record_resource_review(plan_id, history)
            .unwrap();
        let save = session.save().unwrap();
        let restored = GameSession::restore(&save).unwrap();
        assert_eq!(
            restored.state.belief_participants[&account]
                .belief()
                .institution_policy(),
            Some(&policy)
        );
        assert_eq!(
            restored
                .state
                .plans
                .plan(plan_id)
                .unwrap()
                .review()
                .last_review_resources,
            Some(history)
        );
        assert_ne!(restored.state.accounts[&account].cash(), history.cash);
        let mut missing = serde_json::to_value(&save).unwrap();
        missing["belief_books"]["1"]
            .as_object_mut()
            .unwrap()
            .remove("institution_policy");
        assert!(serde_json::from_value::<SaveSlot>(missing)
            .unwrap_err()
            .to_string()
            .contains("institution_policy"));
    }

    fn paused_buy_ready_for_recovery_review() -> (GameSession, PlanId, OrderId) {
        let mut session = seeded_buy_plan_with_child();
        let plan_id = session
            .state
            .plans
            .active_plan(AccountId(1), &StockCode("000812".to_owned()))
            .unwrap()
            .plan_id();
        let child_id = session
            .state
            .plans
            .plan(plan_id)
            .unwrap()
            .active_child_order_id()
            .unwrap();
        session
            .state
            .plans
            .apply(
                plan_id,
                PlanEvent::Paused {
                    reason: crate::plans::PauseReason::IntradayDropAcceleration,
                    trading_day: 0,
                },
            )
            .unwrap();
        set_urgency_minutes(&mut session, false);
        (session, plan_id, child_id)
    }

    #[test]
    fn institution_account_latch_blocks_other_buys_after_plan_termination_until_own_root() {
        let mut setup = probe_session().state.setup.clone();
        setup.npcs.inst_count = 2;
        setup.closing_auction_ticks = 0;
        setup.stocks[0].initial_price = Money::from_cents(1);
        setup.stocks[0].float_shares = 0;
        for code in ["000813", "000814"] {
            let mut stock = setup.stocks[0].clone();
            stock.code = StockCode(code.to_owned());
            setup.stocks.push(stock);
        }
        let mut session = GameSession::new(setup, 42).unwrap();
        let account = AccountId(1);
        let other = AccountId(2);
        let original_code = StockCode("000812".to_owned());
        let next_code = StockCode("000813".to_owned());
        let sell_code = StockCode("000814".to_owned());
        let peak = Money::from_cents(1_000_000);
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_insert_position(
                sell_code.clone(),
                crate::Position::from_restored_parts(100, 0, 100, 0),
            );
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_cash(peak.sub(Money::from_cents(100)).unwrap());
        session
            .state
            .accounts
            .get_mut(&other)
            .unwrap()
            .fixture_set_cash(peak);
        let policy = crate::strategy::InstitutionExperiencePolicy::new(
            1,
            crate::strategy::InstitutionLossResponse::HoldOrAdd,
            800,
            1200,
            3000,
            1500,
            3,
            500,
        )
        .unwrap();
        for owner in [account, other] {
            session
                .state
                .belief_participants
                .get_mut(&owner)
                .map(|participant| participant.belief_mut())
                .unwrap()
                .set_institution_policy(policy);
            observe_account_root_for_latch(&mut session, owner);
        }
        let original = open_account_latch_plan(&mut session, account, original_code, Side::Buy);
        let next = open_account_latch_plan(&mut session, account, next_code.clone(), Side::Buy);
        let sell = open_account_latch_plan(&mut session, account, sell_code, Side::Sell);
        let unrelated = open_account_latch_plan(&mut session, other, next_code, Side::Buy);
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_cash(Money::from_cents(700_000 - 100));
        observe_account_root_for_latch(&mut session, account);
        session
            .state
            .plans
            .apply(
                original,
                PlanEvent::Paused {
                    reason: crate::plans::PauseReason::RiskPressure,
                    trading_day: 0,
                },
            )
            .unwrap();
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_cash(Money::from_cents(800_000 - 100));
        observe_account_root_for_latch(&mut session, account);
        let blocked = quote_account_latch_plan(&session, account, next);
        assert_eq!(blocked.decision.action, crate::plans::QuoteAction::Wait);
        assert_eq!(
            blocked.decision.reason,
            crate::plans::QuoteReason::PauseRequested
        );
        assert!(matches!(
            quote_account_latch_plan(&session, other, unrelated)
                .decision
                .action,
            crate::plans::QuoteAction::Submit { .. }
        ));
        let sell_quote = quote_account_latch_plan(&session, account, sell);
        assert_ne!(
            sell_quote.decision.reason,
            crate::plans::QuoteReason::PauseRequested
        );
        session
            .state
            .plans
            .apply(
                original,
                PlanEvent::Terminated {
                    reason: TerminationReason::Cancelled,
                    trading_day: 0,
                },
            )
            .unwrap();
        assert_eq!(
            quote_account_latch_plan(&session, account, next)
                .decision
                .reason,
            crate::plans::QuoteReason::PauseRequested
        );
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_cash(peak.sub(Money::from_cents(100)).unwrap());
        assert_eq!(
            quote_account_latch_plan(&session, account, next)
                .decision
                .reason,
            crate::plans::QuoteReason::PauseRequested
        );
        observe_account_root_for_latch(&mut session, account);
        assert!(matches!(
            quote_account_latch_plan(&session, account, next)
                .decision
                .action,
            crate::plans::QuoteAction::Submit { .. }
        ));
    }

    fn open_account_latch_plan(
        session: &mut GameSession,
        account: AccountId,
        code: StockCode,
        direction: Side,
    ) -> PlanId {
        session
            .state
            .plans
            .create(PlanOpen {
                account,
                code,
                direction,
                target: PlanTarget::ShareCount(100),
                opinion: PlanOpinion {
                    signal_score_bp: if direction == Side::Buy { 8000 } else { -8000 },
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8000,
                urgency: Urgency::Urgent,
                horizon_trading_days: 1,
                created_trading_day: 0,
            })
            .unwrap()
    }

    fn quote_account_latch_plan(
        session: &GameSession,
        account: AccountId,
        plan_id: PlanId,
    ) -> PlanExecutionRequest {
        let market = session.build_market_view();
        let mut cursor = session
            .prepare_plan_quotes_for_account(account, &market, &session.state.plans)
            .unwrap();
        cursor.plans = std::collections::VecDeque::from([plan_id]);
        session
            .generate_next_plan_quote(&mut cursor, &session.state.plans, &BTreeSet::new())
            .unwrap()
    }

    fn observe_account_root_for_latch(session: &mut GameSession, account: AccountId) {
        let market = session.build_market_view();
        let paths = session.market_price_path_observations().unwrap();
        let technical = session.build_chain_technical_observations().unwrap();
        let now = session.chain_observation_instant();
        let exposed = session.chain_exposed_stocks(now);
        let snapshot = RootReadContext::capture(session).unwrap();
        let mut personal = PlanPersonalState::take(session, account);
        let mut operations = PlanChainOperationBatch::empty();
        InstitutionDecisionRoot::observe_personal(
            &snapshot,
            account,
            &mut personal,
            &market,
            &paths,
            &technical,
            now,
            &exposed,
            &session.state.plans,
            &mut operations,
        );
        personal.install(session, account);
    }

    #[test]
    fn institution_boundary_mixed_pause_latches_risk_pressure_until_recovery() {
        for original_reason in [
            crate::plans::PauseReason::IntradayDropAcceleration,
            crate::plans::PauseReason::AdverseSelection,
        ] {
            let (mut session, plan_id, _) = paused_buy_ready_for_recovery_review();
            let account = AccountId(1);
            let code = StockCode("000812".to_owned());
            let mut encoded = serde_json::to_value(&session.state.plans).unwrap();
            encoded["plans"][plan_id.0.to_string()]["status"] =
                serde_json::json!({ "Paused": { "reason": original_reason } });
            session.state.plans = serde_json::from_value(encoded).unwrap();
            let policy = crate::strategy::InstitutionExperiencePolicy::new(
                1,
                crate::strategy::InstitutionLossResponse::HoldOrAdd,
                800,
                1200,
                3000,
                1500,
                3,
                500,
            )
            .unwrap();
            session
                .state
                .belief_participants
                .get_mut(&account)
                .map(|participant| participant.belief_mut())
                .unwrap()
                .set_institution_policy(policy);
            let equity = session.account_equity(account).unwrap();
            let peak = Money::from_cents(equity.cents() * 2);
            session
                .state
                .belief_participants
                .get_mut(&account)
                .map(|participant| participant.belief_mut())
                .unwrap()
                .experience_mut()
                .observe_equity(peak)
                .unwrap();
            drive_once(&mut session, &reversal_assessment(8000));
            assert_eq!(
                session.state.plans.plan(plan_id).unwrap().status(),
                PlanStatus::Paused {
                    reason: crate::plans::PauseReason::RiskPressure
                }
            );
            assert_eq!(
                session.state.plans.plan(plan_id).unwrap().last_resume(),
                None
            );
            let positions_value = session
                .account_equity(account)
                .unwrap()
                .sub(session.state.accounts[&account].cash())
                .unwrap();
            session
                .state
                .accounts
                .get_mut(&account)
                .unwrap()
                .fixture_set_cash(
                    Money::from_cents(peak.cents() * 4 / 5)
                        .sub(positions_value)
                        .unwrap(),
                );
            drive_once(&mut session, &reversal_assessment(8000));
            assert_eq!(
                session.state.plans.plan(plan_id).unwrap().status(),
                PlanStatus::Paused {
                    reason: crate::plans::PauseReason::RiskPressure
                }
            );
            session
                .state
                .accounts
                .get_mut(&account)
                .unwrap()
                .fixture_set_cash(peak.sub(positions_value).unwrap());
            set_urgency_minutes(&mut session, true);
            drive_once(&mut session, &reversal_assessment(8000));
            assert!(matches!(
                session.state.plans.plan(plan_id).unwrap().status(),
                PlanStatus::Paused { .. }
            ));
            set_urgency_minutes(&mut session, false);
            drive_once(&mut session, &reversal_assessment(8000));
            assert_eq!(
                session.state.plans.plan(plan_id).unwrap().status(),
                PlanStatus::Active
            );
            assert_eq!(
                session.state.plans.plan(plan_id).unwrap().last_resume(),
                Some(ResumeReason::TriggerCleared)
            );
            assert_eq!(session.state.plans.plan(plan_id).unwrap().code(), &code);
        }
    }

    #[test]
    fn institution_boundary_buy_quote_shrinks_to_actual_fee_budget() {
        let mut session = probe_session();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_clear_positions();
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_cash(Money::from_cents(60_000));
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_strategy(Some(crate::account::StoredStrategy::production(Box::new(
                crate::strategy::BeliefInstitutionStrategy::new(0.05, 1000).unwrap(),
            ))));
        session.state.plans = PlanBook::default();
        let plan_id = session
            .state
            .plans
            .create(PlanOpen {
                account,
                code: code.clone(),
                direction: Side::Buy,
                target: PlanTarget::ShareCount(1000),
                opinion: PlanOpinion {
                    signal_score_bp: 8000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8000,
                urgency: Urgency::Urgent,
                horizon_trading_days: 1,
                created_trading_day: 0,
            })
            .unwrap();
        let price = Money::from_cents(285);
        let mut market = session.build_market_view();
        market.stocks.get_mut(&code).unwrap().best_bid = Some(price);
        market.stocks.get_mut(&code).unwrap().best_ask = Some(price);
        let mut cursor = session
            .prepare_plan_quotes_for_account(account, &market, &session.state.plans)
            .unwrap();
        let request = session
            .generate_next_plan_quote(&mut cursor, &session.state.plans, &BTreeSet::new())
            .expect("cash can fund a smaller legal child");
        assert_eq!(request.plan_id, plan_id);
        let crate::plans::QuoteAction::Submit { price, qty } = request.decision.action else {
            panic!("funded child must submit")
        };
        let maximum = crate::strategy::affordable_buy_qty(
            1000,
            price,
            request.allocation.allocated_cash,
            &session.state.setup.config,
        )
        .unwrap();
        assert_eq!(qty, maximum);
        assert!((100..1000).contains(&qty));
        assert!(
            buy_order_reservation(&session.state.setup.config, price, qty, Money::ZERO).unwrap()
                <= request.allocation.allocated_cash
        );
        assert_eq!(session.state.plans.plan(plan_id).unwrap().filled_qty(), 0);
    }

    #[test]
    fn institution_boundary_existing_child_keeps_priority_without_reusing_frozen_cash() {
        let mut session = seeded_buy_plan_with_child();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let child = session.state.parent_orders[&account][&code].clone();
        let frozen = session.reserved_cash_for_account(account).unwrap();
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_cash(frozen);
        session
            .state
            .belief_participants
            .get_mut(&account)
            .map(|participant| participant.belief_mut())
            .unwrap()
            .set_institution_policy(
                crate::strategy::InstitutionExperiencePolicy::new(
                    1,
                    crate::strategy::InstitutionLossResponse::HoldOrAdd,
                    800,
                    1200,
                    10_000,
                    5000,
                    3,
                    10_000,
                )
                .unwrap(),
            );
        let mut market = session.build_market_view();
        market.stocks.get_mut(&code).unwrap().best_bid = Some(child.limit_price());
        market.stocks.get_mut(&code).unwrap().best_ask = Some(child.limit_price());
        let mut cursor = session
            .prepare_plan_quotes_for_account(account, &market, &session.state.plans)
            .unwrap();
        assert_eq!(cursor.grants.as_ref().unwrap().available_cash, Money::ZERO);
        let request = session
            .generate_next_plan_quote(&mut cursor, &session.state.plans, &BTreeSet::new())
            .expect("keeping an already funded child needs no second budget");
        assert_eq!(
            request.decision.action,
            crate::plans::QuoteAction::Keep {
                order_id: child.active_child_order_id().unwrap()
            }
        );
        assert_eq!(request.allocation.allocated_cash, Money::ZERO);
        assert_eq!(session.reserved_cash_for_account(account).unwrap(), frozen);
        market.stocks.get_mut(&code).unwrap().best_bid =
            Some(child.limit_price().add(Money::from_cents(1)).unwrap());
        let mut cursor = session
            .prepare_plan_quotes_for_account(account, &market, &session.state.plans)
            .unwrap();
        let request = session
            .generate_next_plan_quote(&mut cursor, &session.state.plans, &BTreeSet::new())
            .unwrap();
        assert_eq!(
            request.decision.action,
            crate::plans::QuoteAction::Keep {
                order_id: child.active_child_order_id().unwrap()
            }
        );
        assert_eq!(request.allocation.allocated_cash, Money::ZERO);
        assert_eq!(session.reserved_cash_for_account(account).unwrap(), frozen);
    }

    #[test]
    fn institution_boundary_patient_buy_sizes_at_its_actual_quote_not_protection() {
        assert_patient_buy_sizes_at_actual_quote(1000);
    }

    #[test]
    fn institution_boundary_large_child_fee_estimate_cannot_hide_a_funded_board_lot() {
        assert_patient_buy_sizes_at_actual_quote(1_000_000);
    }

    fn assert_patient_buy_sizes_at_actual_quote(order_size: u32) {
        let mut session = probe_session();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let price = Money::from_cents(285);
        let cash =
            buy_order_reservation(&session.state.setup.config, price, 100, Money::ZERO).unwrap();
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_clear_positions();
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_cash(cash);
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_strategy(Some(crate::account::StoredStrategy::production(Box::new(
                crate::strategy::BeliefInstitutionStrategy::new(0.05, order_size)
                    .unwrap()
                    .with_institution_style(crate::strategy::InstitutionStyle::DeepValue),
            ))));
        session.state.plans = PlanBook::default();
        session
            .state
            .plans
            .create(PlanOpen {
                account,
                code: code.clone(),
                direction: Side::Buy,
                target: PlanTarget::ShareCount(order_size),
                opinion: PlanOpinion {
                    signal_score_bp: 8000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8000,
                urgency: Urgency::Patient,
                horizon_trading_days: 5,
                created_trading_day: 0,
            })
            .unwrap();
        let mut market = session.build_market_view();
        market.stocks.get_mut(&code).unwrap().best_bid = Some(price);
        market.stocks.get_mut(&code).unwrap().best_ask = Some(price);
        let mut cursor = session
            .prepare_plan_quotes_for_account(account, &market, &session.state.plans)
            .unwrap();
        let request = session
            .generate_next_plan_quote(&mut cursor, &session.state.plans, &BTreeSet::new())
            .unwrap();
        assert_eq!(
            request.decision.action,
            crate::plans::QuoteAction::Submit { price, qty: 100 }
        );
        assert_eq!(request.allocation.allocated_cash, cash);
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_cash(cash.sub(Money::from_cents(1)).unwrap());
        let mut cursor = session
            .prepare_plan_quotes_for_account(account, &market, &session.state.plans)
            .unwrap();
        let request = session
            .generate_next_plan_quote(&mut cursor, &session.state.plans, &BTreeSet::new())
            .unwrap();
        assert_eq!(request.decision.action, crate::plans::QuoteAction::Wait);
        assert_eq!(
            session
                .state
                .plans
                .active_plan(account, &code)
                .unwrap()
                .filled_qty(),
            0
        );
    }

    fn assert_recovery_review_withdraws_stale_buy(score_bp: i32) {
        let (mut session, plan_id, child_id) = paused_buy_ready_for_recovery_review();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let filled = session.state.plans.plan(plan_id).unwrap().filled_qty();
        let position =
            serde_json::to_value(session.state.accounts[&account].positions().get(&code)).unwrap();
        let events = drive_once(&mut session, &reversal_assessment(score_bp));
        assert_eq!(
            session.state.plans.plan(plan_id).unwrap().status(),
            PlanStatus::Terminated {
                reason: TerminationReason::Cancelled,
            }
        );
        assert_eq!(
            session
                .state
                .plans
                .plan(plan_id)
                .unwrap()
                .opinion()
                .signal_score_bp,
            score_bp
        );
        assert_eq!(
            session.state.plans.plan(plan_id).unwrap().filled_qty(),
            filled
        );
        assert!(session.state.plans.active_plan(account, &code).is_none());
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::OrderCanceled { id, .. } if *id == child_id)));
        assert!(!events.iter().any(|event| matches!(event, Event::OrderAccepted { account: owner, .. } if *owner == account)));
        assert_eq!(
            serde_json::to_value(session.state.accounts[&account].positions().get(&code)).unwrap(),
            position
        );
        assert!(!session
            .state
            .parent_orders
            .get(&account)
            .is_some_and(|parents| parents.contains_key(&code)));
    }

    #[test]
    fn urgency_recovery_review_neutral_signal_withdraws_buy_through_real_cancel() {
        assert_recovery_review_withdraws_stale_buy(0);
    }

    #[test]
    fn urgency_recovery_review_negative_subthreshold_withdraws_buy_without_opening_sell() {
        assert_recovery_review_withdraws_stale_buy(-1_000);
    }

    #[test]
    fn urgency_recovery_review_weak_positive_revises_opinion_without_resuming() {
        let (mut session, plan_id, _) = paused_buy_ready_for_recovery_review();
        let original = session.state.plans.plan(plan_id).unwrap().clone();
        let events = drive_once(&mut session, &reversal_assessment(1_999));
        let reviewed = session.state.plans.plan(plan_id).unwrap();
        assert!(matches!(reviewed.status(), PlanStatus::Paused { .. }));
        assert_eq!(reviewed.opinion().signal_score_bp, 1_999);
        assert!(reviewed.version() > original.version());
        assert_eq!(reviewed.target(), original.target());
        assert_eq!(reviewed.filled_qty(), original.filled_qty());
        assert!(!events.iter().any(|event| matches!(event, Event::OrderAccepted { account: owner, .. } if *owner == AccountId(1))));
    }

    #[test]
    fn urgency_recovery_review_uses_custom_resume_threshold_without_resuming_below_it() {
        let (mut session, plan_id, _) = paused_buy_ready_for_recovery_review();
        session.state.urgency_policy.resume_signal_threshold_bp = 3_000;
        drive_once(&mut session, &reversal_assessment(2_500));
        assert!(matches!(
            session.state.plans.plan(plan_id).unwrap().status(),
            PlanStatus::Paused { .. }
        ));
        drive_once(&mut session, &reversal_assessment(3_000));
        assert_eq!(
            session.state.plans.plan(plan_id).unwrap().status(),
            PlanStatus::Active
        );
        assert_eq!(
            session.state.plans.plan(plan_id).unwrap().last_resume(),
            Some(ResumeReason::TriggerCleared)
        );
    }

    #[test]
    fn urgency_paused_buy_reversal_restarts_sell_leg_without_buy_pause() {
        let mut plans = PlanBook::default();
        let plan_id = plans
            .create(PlanOpen {
                account: AccountId(1),
                code: StockCode("000812".to_owned()),
                direction: Side::Buy,
                target: PlanTarget::ShareCount(100),
                opinion: PlanOpinion {
                    signal_score_bp: 3_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: 0,
            })
            .unwrap();
        plans
            .apply(
                plan_id,
                PlanEvent::Paused {
                    reason: crate::plans::PauseReason::IntradayDropAcceleration,
                    trading_day: 0,
                },
            )
            .unwrap();
        plans
            .apply(
                plan_id,
                PlanEvent::Revised {
                    revision: PlanRevision {
                        reason: RevisionReason::SignalShift,
                        trading_day: 0,
                        direction: Side::Sell,
                        target: PlanTarget::ShareCount(100),
                        opinion: PlanOpinion {
                            signal_score_bp: -3_000,
                            source: OpinionSource::Blended,
                        },
                        confidence_bp: 8_000,
                        urgency: Urgency::Normal,
                        below_filled_rationale: None,
                    },
                },
            )
            .unwrap();
        assert_eq!(plans.plan(plan_id).unwrap().status(), PlanStatus::Active);
        assert_eq!(plans.plan(plan_id).unwrap().direction(), Side::Sell);
        assert_eq!(plans.plan(plan_id).unwrap().filled_qty(), 0);
    }

    fn set_urgency_minutes(session: &mut GameSession, falling: bool) {
        session.state.tick = 30;
        session.state.pending_npc = None;
        let end = session.current_market_minute();
        session.state.market_minute_closes.insert(
            StockCode("000812".to_owned()),
            (0..end)
                .map(|minute| MarketMinuteClose {
                    absolute_trading_minute: minute,
                    close: Money::from_cents(if falling && minute + 1 == end {
                        270
                    } else {
                        285
                    }),
                })
                .collect(),
        );
        crate::session::pipeline::queue_npc_for_next_tick(session).unwrap();
    }

    #[test]
    fn urgency_acceleration_pauses_live_plan_and_routes_real_cancel() {
        let mut session = seeded_buy_plan_with_child();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let plan_id = session
            .state
            .plans
            .active_plan(account, &code)
            .unwrap()
            .plan_id();
        let child = session
            .state
            .plans
            .plan(plan_id)
            .unwrap()
            .active_child_order_id()
            .unwrap();
        let filled = session.state.plans.plan(plan_id).unwrap().filled_qty();
        set_urgency_minutes(&mut session, true);
        let events = drive_once(&mut session, &reversal_assessment(5_000));
        assert_eq!(
            session.state.plans.plan(plan_id).unwrap().status(),
            PlanStatus::Paused {
                reason: crate::plans::PauseReason::IntradayDropAcceleration,
            }
        );
        assert!(events
            .iter()
            .any(|event| matches!(event, Event::OrderCanceled { id, .. } if *id == child)));
        assert_eq!(
            session.state.plans.plan(plan_id).unwrap().filled_qty(),
            filled
        );
    }

    #[test]
    fn urgency_paused_plan_requires_own_observation_and_reaffirmed_signal() {
        let mut session = seeded_buy_plan_with_child();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let plan_id = session
            .state
            .plans
            .active_plan(account, &code)
            .unwrap()
            .plan_id();
        session
            .state
            .plans
            .apply(
                plan_id,
                PlanEvent::Paused {
                    reason: crate::plans::PauseReason::IntradayDropAcceleration,
                    trading_day: 0,
                },
            )
            .unwrap();
        set_urgency_minutes(&mut session, false);
        let view = session.build_market_view();
        let mut cursor = session
            .prepare_plan_quotes_for_account(account, &view, &session.state.plans)
            .unwrap();
        let request =
            session.generate_next_plan_quote(&mut cursor, &session.state.plans, &BTreeSet::new());
        assert!(
            request.is_some(),
            "paused child still needs a cancellation request"
        );
        assert!(matches!(
            request.unwrap().decision.action,
            crate::plans::QuoteAction::Cancel { .. }
        ));
        assert!(matches!(
            session.state.plans.plan(plan_id).unwrap().status(),
            PlanStatus::Paused { .. }
        ));
        drive_once(&mut session, &reversal_assessment(1_999));
        assert!(matches!(
            session.state.plans.plan(plan_id).unwrap().status(),
            PlanStatus::Paused { .. }
        ));
        session.state.tick += 1;
        session.state.pending_npc = None;
        crate::session::pipeline::queue_npc_for_next_tick(&mut session).unwrap();
        drive_once(&mut session, &reversal_assessment(2_000));
        assert_eq!(
            session.state.plans.plan(plan_id).unwrap().status(),
            PlanStatus::Active
        );
        assert_eq!(
            session.state.plans.plan(plan_id).unwrap().last_resume(),
            Some(crate::plans::ResumeReason::TriggerCleared)
        );
    }

    #[test]
    fn urgency_paused_child_in_no_cancel_window_stays_pending_without_releasing_resources() {
        let mut session = seeded_buy_plan_with_child();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let plan_id = session
            .state
            .plans
            .active_plan(account, &code)
            .unwrap()
            .plan_id();
        session
            .state
            .plans
            .apply(
                plan_id,
                PlanEvent::Paused {
                    reason: crate::plans::PauseReason::IntradayDropAcceleration,
                    trading_day: 0,
                },
            )
            .unwrap();
        session.state.tick =
            session.state.setup.ticks_per_day - session.state.setup.closing_auction_ticks;
        let child = session
            .state
            .plans
            .plan(plan_id)
            .unwrap()
            .active_child_order_id()
            .unwrap();
        let frozen = session.reserved_cash_for_account(account).unwrap();
        let before = session.state.plans.plan(plan_id).unwrap().clone();
        let view = session.build_market_view();
        let mut cursor = session
            .prepare_plan_quotes_for_account(account, &view, &session.state.plans)
            .unwrap();
        let request = session
            .generate_next_plan_quote(&mut cursor, &session.state.plans, &BTreeSet::new())
            .unwrap();
        assert_eq!(
            request.decision.action,
            crate::plans::QuoteAction::Keep { order_id: child }
        );
        assert_eq!(
            request.decision.reason,
            crate::plans::QuoteReason::PendingReconsideration
        );
        assert_eq!(session.state.plans.plan(plan_id).unwrap(), &before);
        assert_eq!(session.reserved_cash_for_account(account).unwrap(), frozen);
        assert!(session.state.markets[&code]
            .resting_orders()
            .iter()
            .any(|order| order.id == child));
    }

    #[test]
    fn urgency_unavailable_personal_risk_does_not_clear_existing_risk_pause() {
        let mut session = seeded_buy_plan_with_child();
        let account = AccountId(1);
        let experience = session
            .state
            .belief_participants
            .get_mut(&account)
            .map(|participant| participant.belief_mut())
            .unwrap()
            .experience_mut();
        experience.reference_equity = None;
        experience.peak_equity = None;
        let code = StockCode("000812".to_owned());
        let plan_id = session
            .state
            .plans
            .active_plan(account, &code)
            .unwrap()
            .plan_id();
        session
            .state
            .plans
            .apply(
                plan_id,
                PlanEvent::Paused {
                    reason: crate::plans::PauseReason::RiskPressure,
                    trading_day: 0,
                },
            )
            .unwrap();
        set_urgency_minutes(&mut session, false);
        let (_, risk) = session.plan_execution_urgency(
            session.state.plans.plan(plan_id).unwrap(),
            Some(0),
            Some(0),
        );
        assert!(matches!(
            risk,
            crate::plans::urgency::risk::RiskUrgencyAssessment::Unavailable { .. }
        ));
        let actions = session.collect_plan_lifecycle_actions(
            account,
            &reversal_assessment(8_000),
            &session.build_market_view(),
            &session.state.plans,
        );
        assert!(
            matches!(actions.as_slice(), [PlanLifecycleAction::Observe { plan_id: observed, .. }] if *observed == plan_id)
        );
    }

    #[test]
    fn urgency_policy_is_required_in_saved_session() {
        let session = probe_session();
        let save = session.save().unwrap();
        let mut json = serde_json::to_value(&save).unwrap();
        assert_eq!(json["urgency_policy"]["policy_version"], 1);
        json.as_object_mut().unwrap().remove("urgency_policy");
        let error = serde_json::from_value::<SaveSlot>(json).unwrap_err();
        assert!(error.to_string().contains("urgency_policy"));
    }

    #[test]
    fn saved_urgency_policy_restores_and_survives_session_clones() {
        let session = probe_session();
        let mut json = serde_json::to_value(session.save().unwrap()).unwrap();
        json["urgency_policy"] = serde_json::json!({
            "policy_version": 1,
            "drop_30min_threshold_bp": -450,
            "drop_1min_threshold_bp": -100,
            "urgent_drawdown_threshold_bp": 2500,
            "patient_confidence_threshold_bp": 3500,
            "urgent_remaining_trading_days": 2,
            "resume_signal_threshold_bp": 2200
        });
        let save = serde_json::from_value::<SaveSlot>(json.clone()).unwrap();
        let restored = GameSession::restore(&save).unwrap();
        assert_eq!(
            serde_json::to_value(restored.state.urgency_policy).unwrap(),
            json["urgency_policy"]
        );
        let cloned = restored.clone_for_tick_shadow().unwrap();
        assert_eq!(
            serde_json::to_value(cloned.state.urgency_policy).unwrap(),
            json["urgency_policy"]
        );
    }

    #[test]
    fn urgency_quote_uses_frozen_policy_instead_of_default() {
        let mut session = probe_session();
        session.state.setup.auction_ticks = 10;
        session
            .state
            .plans
            .create(PlanOpen {
                account: AccountId(1),
                code: StockCode("000812".to_owned()),
                direction: Side::Buy,
                target: PlanTarget::ShareCount(100),
                opinion: PlanOpinion {
                    signal_score_bp: 5_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: Urgency::Normal,
                horizon_trading_days: 3,
                created_trading_day: 0,
            })
            .unwrap();
        let mut json = serde_json::to_value(session.save().unwrap()).unwrap();
        let mut policy = serde_json::to_value(UrgencyPolicy::default()).unwrap();
        policy["urgent_remaining_trading_days"] = serde_json::json!(2);
        json["urgency_policy"] = policy;
        let save = serde_json::from_value::<SaveSlot>(json).unwrap();
        let restored = GameSession::restore(&save).unwrap();
        let view = restored.build_market_view();
        let mut cursor = restored
            .prepare_plan_quotes_for_account(AccountId(1), &view, &restored.state.plans)
            .unwrap();
        let request = restored
            .generate_next_plan_quote(&mut cursor, &restored.state.plans, &BTreeSet::new())
            .unwrap();
        assert_eq!(
            request.decision.reason,
            crate::plans::QuoteReason::UrgentProtectedLimit
        );
    }

    #[test]
    fn urgency_policy_changes_authoritative_business_hash() {
        let original = probe_session();
        let mut changed = original.clone_for_tick_shadow().unwrap();
        changed.state.urgency_policy.urgent_remaining_trading_days = 2;
        assert_ne!(
            original.business_state_hash().unwrap(),
            changed.business_state_hash().unwrap()
        );
    }

    #[test]
    fn urgent_plan_quote_applies_price_cage_only_when_enabled_in_continuous_trading() {
        for (auction_ticks, cage_enabled, buy_price, sell_price) in [
            (0, true, 295, 275),
            (0, false, 314, 257),
            (10, true, 314, 257),
        ] {
            for (side, expected) in [(Side::Buy, buy_price), (Side::Sell, sell_price)] {
                let mut session = probe_session();
                session.state.setup.auction_ticks = auction_ticks;
                session.state.setup.config.price_cage_enabled = cage_enabled;
                let owner = AccountId(1);
                let code = StockCode("000812".to_string());
                let account = session.state.accounts.get_mut(&owner).unwrap();
                account.fixture_set_cash(Money::from_cents(1_000_000));
                account.fixture_clear_positions();
                if side == Side::Sell {
                    account.fixture_insert_position(
                        code.clone(),
                        crate::Position::from_restored_parts(100, 0, 28_500, 0),
                    );
                }
                session
                    .state
                    .plans
                    .create(PlanOpen {
                        account: owner,
                        code,
                        direction: side,
                        target: PlanTarget::ShareCount(100),
                        opinion: PlanOpinion {
                            signal_score_bp: 8_000,
                            source: OpinionSource::Blended,
                        },
                        confidence_bp: 8_000,
                        urgency: Urgency::Urgent,
                        horizon_trading_days: 1,
                        created_trading_day: 0,
                    })
                    .unwrap();
                let market = session.build_market_view();
                let mut cursor = session
                    .prepare_plan_quotes_for_account(owner, &market, &session.state.plans)
                    .unwrap();
                let request = session
                    .generate_next_plan_quote(&mut cursor, &session.state.plans, &BTreeSet::new())
                    .unwrap();
                assert_eq!(
                    request.decision.action,
                    crate::plans::QuoteAction::Submit {
                        price: Money::from_cents(expected),
                        qty: 100,
                    },
                    "phase={:?}, cage_enabled={cage_enabled}, side={side:?}",
                    session.phase()
                );
            }
        }
    }

    #[test]
    fn plan_wake_uses_strategy_cadence_and_its_own_price_threshold() {
        let mut session = probe_session();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let plan_id = session
            .state
            .plans
            .create(PlanOpen {
                account,
                code: code.clone(),
                direction: Side::Buy,
                target: PlanTarget::ShareCount(100),
                opinion: PlanOpinion {
                    signal_score_bp: 2_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 5_000,
                urgency: Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: 0,
            })
            .unwrap();
        session
            .state
            .plans
            .record_review(plan_id, 0, Money::from_cents(285), 0)
            .unwrap();
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_strategy(Some(crate::account::StoredStrategy::production(Box::new(
                crate::strategy::BeliefInstitutionStrategy::new(0.05, 100)
                    .unwrap()
                    .with_institution_style(crate::strategy::InstitutionStyle::Growth),
            ))));
        assert!(!session.active_plan_requires_review(account));

        session.state.setup.auction_ticks = 10;
        session.state.tick = session.state.setup.ticks_per_day;
        session.state.day = 1;
        assert!(!session.active_plan_requires_review(account));
        session
            .state
            .plans
            .record_review(plan_id, 1, Money::from_cents(285), 0)
            .unwrap();
        session.state.tick += 10;
        assert!(!session.active_plan_requires_review(account));
        assert!(!session
            .capture_ready_decision_chain_roots(&[])
            .unwrap()
            .is_empty());
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_strategy(Some(crate::account::StoredStrategy::production(Box::new(
                crate::strategy::BeliefInstitutionStrategy::new(0.05, 100)
                    .unwrap()
                    .with_institution_style(crate::strategy::InstitutionStyle::DeepValue),
            ))));
        assert!(!session.active_plan_requires_review(account));
        session
            .state
            .markets
            .get_mut(&code)
            .unwrap()
            .fixture_set_last_price(Money::from_cents(291));
        assert!(session.active_plan_requires_review(account));
    }

    #[test]
    fn active_trader_creates_only_a_same_day_plan_after_the_opening_auction() {
        let mut session = probe_session();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let style = crate::strategy::InstitutionStyle::ActiveTrader;
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_strategy(Some(crate::account::StoredStrategy::production(Box::new(
                crate::strategy::BeliefInstitutionStrategy::new(0.05, 100)
                    .unwrap()
                    .with_institution_style(style),
            ))));
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_set_cash(Money::from_cents(10_000_000));
        session
            .state
            .accounts
            .get_mut(&account)
            .unwrap()
            .fixture_remove_position(&code);
        let profile = crate::strategy::StrategyProfile::Institution(style);
        let mut rng = crate::session::SplitMix64::new(17);
        let analysis =
            crate::strategy::derive_analysis_profile(&profile, account, &mut rng).unwrap();
        assert_eq!(analysis.weights().fundamental_bp(), 0);
        *session
            .state
            .belief_participants
            .get_mut(&account)
            .expect("fixture institution participant exists")
            .belief_mut() = crate::strategy::BeliefBook::new(account, profile, analysis, &mut rng);
        let assessment = BTreeMap::from([(
            code,
            CandidateAssessment::Scored {
                score: crate::plans::SignalScore::new(10_000).unwrap(),
                used_weight_bp: 10_000,
                excluded: Vec::new(),
            },
        )]);
        session.state.setup.auction_ticks = 10;
        let market = session.build_market_view();
        assert!(session
            .collect_plan_lifecycle_actions(account, &assessment, &market, &session.state.plans)
            .is_empty());
        assert!(session
            .capture_ready_decision_chain_roots(&[])
            .unwrap()
            .is_empty());
        session.state.tick = 10;
        assert_eq!(session.phase(), TradingPhase::Continuous);
        assert!(!session
            .capture_ready_decision_chain_roots(&[])
            .unwrap()
            .is_empty());
        let actions = session.collect_plan_lifecycle_actions(
            account,
            &assessment,
            &market,
            &session.state.plans,
        );
        assert_eq!(
            actions.len(),
            1,
            "active trader should create one intraday plan"
        );
        assert!(
            matches!(actions.as_slice(), [PlanLifecycleAction::Create { open }] if open.horizon_trading_days == 1)
        );
    }

    #[test]
    fn partial_sell_plan_does_not_split_or_include_the_odd_lot_remainder() {
        let events = route_sell_plan(25_500, 25_491);

        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::OrderAccepted { account, remaining_qty, .. }
                    if *account == AccountId(1) && *remaining_qty == 25_400
            )),
            "a partial A-share sell must route only whole board lots: {events:?}"
        );
    }

    #[test]
    fn full_sell_plan_exits_the_available_odd_lot_in_one_order() {
        let events = route_sell_plan(25_491, 25_491);

        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::OrderAccepted { account, remaining_qty, .. }
                    if *account == AccountId(1) && *remaining_qty == 25_491
            )),
            "a complete sellable odd lot must be routed in the one permitted sell order: {events:?}"
        );
    }

    #[test]
    fn sub_board_lot_buy_strategy_size_has_no_routable_child() {
        assert_eq!(
            next_routable_buy_qty(1_000, 99, 100, 1_000_000),
            None,
            "an explicit sub-lot strategy size must not be silently enlarged to one board lot"
        );
    }

    #[test]
    fn sub_board_lot_buy_plan_does_not_submit_or_advance_a_parent() {
        let mut session = probe_session();
        let owner = AccountId(1);
        let code = StockCode("000812".to_string());
        session
            .state
            .accounts
            .get_mut(&owner)
            .unwrap()
            .fixture_set_strategy(Some(crate::account::StoredStrategy::production(Box::new(
                crate::strategy::BeliefInstitutionStrategy::new(0.05, 99)
                    .expect("strategy permits an explicit sub-board-lot size"),
            ))));
        let owner_account = session
            .state
            .accounts
            .get_mut(&owner)
            .expect("fixture owner must exist");
        owner_account.fixture_set_cash(Money::from_cents(1_000_000));
        owner_account.fixture_clear_positions();
        let plan_id = session
            .state
            .plans
            .create(PlanOpen {
                account: owner,
                code: code.clone(),
                direction: Side::Buy,
                target: PlanTarget::ShareCount(1_000),
                opinion: PlanOpinion {
                    signal_score_bp: 8_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: u64::from(session.state.day),
            })
            .unwrap();
        let mut plans = std::mem::take(&mut session.state.plans);
        let view = session.build_market_view();
        let mut operations = PlanChainOperationBatch::empty();

        session
            .synchronize_owned_plan_execution(&mut plans)
            .expect("fixture plan facts must synchronize");
        let observation: &GameSession = &session;
        if let Some(cursor) = observation.prepare_plan_quotes_for_account(owner, &view, &plans) {
            operations.push_quote_plans(cursor);
        }
        session.state.plans = plans;
        let events =
            crate::session::pipeline::commit_injected_plan_roots_for_test(&mut session, operations);

        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Event::OrderAccepted { .. } | Event::Trade { .. })),
            "a 99-share strategy must not emit an order or trade event"
        );
        assert!(session.state.markets[&code]
            .resting_orders_for(owner)
            .is_empty());
        assert!(session
            .state
            .parent_orders
            .get(&owner)
            .and_then(|parents| parents.get(&code))
            .is_none());
        let plan = session.state.plans.plan(plan_id).unwrap();
        assert_eq!(plan.filled_qty(), 0);
        assert!(plan.active_child_order_id().is_none());
    }

    #[test]
    fn buy_allocation_request_reserves_only_the_next_routable_child() {
        // A long-lived plan may represent more shares than its account could buy in one
        // order. Its allocation input must reserve the next routable child, not the entire
        // remaining parent target; otherwise a valid affordable child is never considered.
        let config = crate::config::GameConfig::proposed_defaults();
        let request = buy_allocation_request(
            &config,
            PlanId(0),
            StockCode("000812".to_string()),
            8_000,
            Money::from_cents(2_000),
            1_000_000,
            1_000,
            100,
            1_000_000,
        )
        .expect("one board-lot child is routable");

        assert_eq!(request.requested_cash, Money::from_cents(2_000_000));
        let authoritative_reservation =
            buy_order_reservation(&config, Money::from_cents(2_000), 1_000, Money::ZERO)
                .expect("test fee schedule is valid");
        assert_eq!(
            request
                .requested_cash
                .add(request.fee_reserve)
                .expect("test totals fit Money"),
            authoritative_reservation,
            "allocation splits the authoritative total reservation into gross and fee"
        );
        let cash = authoritative_reservation
            .add(authoritative_reservation)
            .expect("test cash fits Money");
        let full_parent_reservation =
            buy_order_reservation(&config, Money::from_cents(2_000), 1_000_000, Money::ZERO)
                .expect("test parent reservation fits Money");
        assert!(
            full_parent_reservation > cash,
            "the regression needs a parent that is unaffordable as one fictitious order"
        );
        let allocation = allocate_soft_budgets(
            &AllocationFunds {
                cash,
                frozen_cash: Money::ZERO,
            },
            &[request],
        )
        .expect("the exact authoritative reservation is affordable");
        assert_eq!(
            allocation.grants[0].allocated_cash, authoritative_reservation,
            "available cash funds the next child and its actual fees"
        );
        assert_eq!(allocation.grants[0].constraint, None);
        assert!(
            allocation.total_allocated <= allocation.available_cash,
            "soft budgets never exceed authoritative available cash"
        );
    }

    #[test]
    fn unfinished_buy_plan_with_less_cash_than_its_fee_receives_zero_budget() {
        let mut session = probe_session();
        let owner = AccountId(1);
        let code = StockCode("000812".to_string());
        let plan_id = session
            .state
            .plans
            .create(PlanOpen {
                account: owner,
                code,
                direction: Side::Buy,
                target: PlanTarget::ShareCount(1_000_000),
                opinion: PlanOpinion {
                    signal_score_bp: 8_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: u64::from(session.state.day),
            })
            .unwrap();
        let view = session.build_market_view();
        for cash_cents in [400, 0] {
            session
                .state
                .accounts
                .get_mut(&owner)
                .unwrap()
                .fixture_set_cash(Money::from_cents(cash_cents));

            let cursor = session
                .prepare_plan_quotes_for_account(owner, &view, &session.state.plans)
                .expect("an unfinished plan still receives an observation");
            let allocation = cursor.grants.expect("the buy plan requested a budget");

            assert_eq!(allocation.total_allocated, Money::ZERO);
            assert_eq!(allocation.grants.len(), 1);
            assert_eq!(allocation.grants[0].plan_id, plan_id);
            assert_eq!(allocation.grants[0].allocated_cash, Money::ZERO);
            assert_eq!(
                allocation.grants[0].constraint,
                Some(crate::plans::AllocationConstraint::InsufficientAvailableCash)
            );
            assert_eq!(session.state.plans.plan(plan_id).unwrap().filled_qty(), 0);
        }
    }

    #[test]
    fn unaffordable_parent_target_routes_its_affordable_next_buy_child() {
        let mut session = probe_session();
        let owner = AccountId(1);
        let code = StockCode("000812".to_string());
        // Enter the call auction: an empty book is explicitly represented as a two-sided
        // protected quote, so the test exercises the actual submit route rather than a wait.
        session.state.setup.auction_ticks = 10;
        let owner_account = session
            .state
            .accounts
            .get_mut(&owner)
            .expect("fixture owner must exist");
        owner_account.fixture_set_cash(Money::from_cents(1_000_000));
        // `probe_session` assigns the institution a deterministic random float position.
        // Clear it to isolate cash budgeting from position valuation and T+1 state.
        owner_account.fixture_clear_positions();
        assert!(owner_account.positions().is_empty());
        let parent_qty = 1_000_000;
        let stock = session
            .state
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == code)
            .expect("fixture stock must exist");
        let child_qty = next_routable_buy_qty(
            parent_qty,
            session.chain_strategy_params(owner).order_size,
            session.state.setup.config.lot_size,
            stock.category.max_order_qty(false),
        )
        .expect("fixture parent must have one routable child");
        let child_reservation = buy_order_reservation(
            &session.state.setup.config,
            session.state.markets[&code].up_stop().unwrap(),
            child_qty,
            Money::ZERO,
        )
        .unwrap();
        let parent_reservation = buy_order_reservation(
            &session.state.setup.config,
            session.state.markets[&code].up_stop().unwrap(),
            parent_qty,
            Money::ZERO,
        )
        .unwrap();
        let cash = session.state.accounts[&owner].cash();
        let equity = session.account_equity(owner).unwrap();
        assert_eq!(
            equity, cash,
            "cash-only fixture must have no hidden position equity"
        );
        assert!(parent_reservation > cash);
        assert!(
            child_reservation <= cash,
            "available cash must cover the real child reservation"
        );
        session
            .state
            .plans
            .create(PlanOpen {
                account: owner,
                code: code.clone(),
                direction: Side::Buy,
                target: PlanTarget::ShareCount(parent_qty),
                opinion: PlanOpinion {
                    signal_score_bp: 8_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: u64::from(session.state.day),
            })
            .unwrap();
        let mut plans = std::mem::take(&mut session.state.plans);
        let view = session.build_market_view();
        let mut operations = PlanChainOperationBatch::empty();

        session
            .synchronize_owned_plan_execution(&mut plans)
            .expect("fixture plan facts must synchronize");
        let observation: &GameSession = &session;
        if let Some(cursor) = observation.prepare_plan_quotes_for_account(owner, &view, &plans) {
            operations.push_quote_plans(cursor);
        }
        session.state.plans = plans;
        let events =
            crate::session::pipeline::commit_injected_plan_roots_for_test(&mut session, operations);

        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::OrderAccepted { account, remaining_qty, .. }
                    if *account == owner && *remaining_qty == child_qty
            )),
            "the affordable child must route through P3/P4: {events:?}"
        );
        assert!(
            session
                .state
                .parent_orders
                .get(&owner)
                .and_then(|parents| parents.get(&code))
                .and_then(|parent| parent.active_child_order_id())
                .is_some(),
            "the accepted child must be linked to its parent plan"
        );
        assert!(
            session.reserved_cash_for_account(owner).unwrap()
                <= session.state.accounts[&owner].cash(),
            "the actual child reservation remains within authoritative cash"
        );
    }

    #[test]
    fn observation_clock_maps_session_boundaries_and_compressed_days() {
        let mut setup = probe_session().save().expect("healthy save").setup;
        setup.ticks_per_day = 15_300;
        setup.auction_ticks = 900;
        setup.closing_auction_ticks = 180;
        let mut session = GameSession::new(setup, 42).unwrap();
        for (tick, seconds) in [
            (0, 33_300),
            (300, 33_600),
            (600, 33_900),
            (900, 34_200),
            (8_099, 41_399),
            (8_100, 46_800),
            (15_120, 53_820),
            (15_300, 54_000),
        ] {
            session.state.tick = tick;
            assert_eq!(session.chain_observation_instant().second_of_day(), seconds);
        }
        let mut setup = session.save().expect("healthy save").setup;
        setup.ticks_per_day = 240;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
        let mut session = GameSession::new(setup, 42).unwrap();
        for (tick, seconds) in [(119, 41_340), (120, 46_800), (121, 46_860), (240, 54_000)] {
            session.state.tick = tick;
            assert_eq!(session.chain_observation_instant().second_of_day(), seconds);
        }
    }

    #[test]
    fn decision_observations_include_lunch_without_advancing_market_minutes() {
        let mut setup = probe_session().save().expect("healthy save").setup;
        setup.ticks_per_day = 15_300;
        setup.auction_ticks = 900;
        setup.closing_auction_ticks = 180;
        let mut session = GameSession::new(setup, 42).unwrap();
        session.state.tick = 8_099;
        session.state.pending_npc = None;
        crate::session::pipeline::queue_npc_for_next_tick(&mut session).unwrap();
        let before = session.chain_observation_instant();
        let market_before = session.current_market_minute();
        let _ = session.run_decision_chain(&[AccountId(1)]);

        session.step().expect("healthy step");
        let _ = session.run_decision_chain(&[AccountId(1)]);

        let after = session.chain_observation_instant();
        assert_eq!(before.second_of_day(), 11 * 3600 + 29 * 60 + 59);
        assert_eq!(after.second_of_day(), 13 * 3600);
        assert_eq!(after.second_of_day() - before.second_of_day(), 5_401);
        assert_eq!(session.current_market_minute() - market_before, 0);
        #[cfg(feature = "simulation-diagnostics")]
        {
            let decisions: Vec<_> = session.causal_facts().iter().filter(|fact| {
                matches!(fact.kind, crate::diagnostics::causal::CausalFactKind::Decision { account } if account == AccountId(1))
            }).collect();
            assert!(decisions.len() >= 2);
            assert_eq!(decisions.first().unwrap().time.civil, before);
            assert_eq!(decisions.last().unwrap().time.civil, after);
        }
    }

    #[test]
    fn decision_chain_generates_a_candidate_without_routing_it() {
        let mut session = probe_session();
        force_attention(&mut session, AccountId(1));
        let mut events = Vec::new();
        session.seed_order_for_test(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockCode("000812".to_string()),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(280)),
                qty: 100,
            },
            &mut events,
        );
        let next_order_id = session.state.next_order_id;
        let resting_before = session
            .state
            .markets
            .values()
            .map(Market::resting_order_count)
            .sum::<usize>();

        let batch = session.run_decision_chain(&[AccountId(1)]);

        assert_eq!(batch.len(), 2);
        assert_eq!(session.state.next_order_id, next_order_id);
        assert_eq!(
            session
                .state
                .markets
                .values()
                .map(Market::resting_order_count)
                .sum::<usize>(),
            resting_before
        );
    }

    #[test]
    fn generation_preserves_pending_plan_facts_when_a_child_fill_is_waiting() {
        let mut session = seeded_buy_plan_with_child();
        let parent =
            session.state.parent_orders[&AccountId(1)][&StockCode("000812".to_string())].clone();
        session
            .state
            .pending_plan_events
            .push(PendingPlanEvent::Filled {
                plan_id: parent.linked_plan_id().unwrap(),
                order_id: parent.active_child_order_id().unwrap(),
                qty: 100,
                child_complete: false,
                trading_day: u64::from(session.state.day),
            });
        let pending = serde_json::to_value(&session.state.pending_plan_events).unwrap();
        let parents = serde_json::to_value(&session.state.parent_orders).unwrap();
        let identities = (session.state.next_order_id, session.state.seq);

        let _batch = session.run_decision_chain(&[AccountId(1)]);

        assert_eq!(
            serde_json::to_value(&session.state.pending_plan_events).unwrap(),
            pending
        );
        assert_eq!(
            serde_json::to_value(&session.state.parent_orders).unwrap(),
            parents
        );
        assert_eq!((session.state.next_order_id, session.state.seq), identities);
    }

    use super::*;
    use crate::session::plan_execution::PlanRouteOutcome;

    #[test]
    fn plan_root_owns_its_personal_state_until_the_private_candidate_installs_it() {
        let mut session = probe_session();
        let account = AccountId(1);
        let market = session.build_market_view();
        let paths = session.market_price_path_observations().unwrap();
        let technical = session.build_chain_technical_observations().unwrap();
        let now = session.chain_observation_instant();
        let exposed = session.chain_exposed_stocks(now);
        let mut operations = PlanChainOperationBatch::empty();
        let snapshot = RootReadContext::capture(&session).unwrap();
        let mut personal = PlanPersonalState::take(&mut session, account);

        let diagnostics = InstitutionDecisionRoot::observe_personal(
            &snapshot,
            account,
            &mut personal,
            &market,
            &paths,
            &technical,
            now,
            &exposed,
            &snapshot.plans,
            &mut operations,
        );

        assert!(session.state.npc_attention.get(&account).is_none());
        assert!(session
            .state
            .belief_participants
            .get(&account)
            .map(|participant| participant.watchlist())
            .is_none());
        assert!(session
            .state
            .belief_participants
            .get(&account)
            .map(|participant| participant.price_memory())
            .is_none());
        assert!(!session.state.belief_participants.contains_key(&account));
        assert!(session
            .state
            .belief_participants
            .get(&account)
            .map(|participant| participant.belief())
            .is_none());
        assert!(!operations.is_empty());
        personal.install(&mut session, account);
        #[cfg(feature = "simulation-diagnostics")]
        {
            let plans = session.state.plans.clone();
            session.record_plan_root_diagnostics(account, diagnostics, &plans);
        }
        #[cfg(not(feature = "simulation-diagnostics"))]
        let _ = diagnostics;
        assert!(session.state.npc_attention.get(&account).is_some());
        assert!(session
            .state
            .belief_participants
            .get(&account)
            .map(|participant| participant.watchlist())
            .is_some());
        assert!(session
            .state
            .belief_participants
            .get(&account)
            .map(|participant| participant.price_memory())
            .is_some());
        assert!(session.state.belief_participants.contains_key(&account));
        assert!(session
            .state
            .belief_participants
            .get(&account)
            .map(|participant| participant.belief())
            .is_some());
    }

    #[test]
    fn live_plan_without_position_or_belief_still_enters_root_observation() {
        let mut session = probe_session();
        let account = AccountId(1);
        let code = StockCode("000812".to_string());
        session
            .state
            .accounts
            .get_mut(&account)
            .expect("institution account exists")
            .fixture_remove_position(&code);
        let held = session.state.accounts[&account]
            .positions()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>();
        assert!(held.is_empty());
        assert!(session.state.belief_participants[&account]
            .belief()
            .entry(&code)
            .is_none());
        session
            .state
            .plans
            .create(PlanOpen {
                account,
                code: code.clone(),
                direction: Side::Buy,
                target: PlanTarget::ShareCount(100),
                opinion: PlanOpinion {
                    signal_score_bp: 5_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: u64::from(session.state.day),
            })
            .unwrap();
        let candidates = root_candidate_codes(
            account,
            &held,
            session.state.belief_participants[&account].belief(),
            &session.state.plans,
            None,
        );
        assert_eq!(candidates, BTreeSet::from([code]));
    }

    #[test]
    fn root_observation_rejects_tick_minute_and_phase_clock_mismatches() {
        let session = probe_session();
        for (tick, minute, phase) in [
            (
                session.state.tick + 1,
                session.current_market_minute(),
                session.phase(),
            ),
            (
                session.state.tick,
                session.current_market_minute() + 1,
                session.phase(),
            ),
            (
                session.state.tick,
                session.current_market_minute(),
                TradingPhase::ClosingAuction,
            ),
        ] {
            assert!(
                tick != session.state.tick
                    || minute != session.current_market_minute()
                    || phase != session.phase()
            );
            let mut market = session.build_market_view();
            market.tick = tick;
            market.market_minute = minute;
            let snapshot = crate::session::pipeline::DecisionSnapshot::new(
                tick,
                1,
                phase,
                minute,
                market,
                None,
                vec![],
                BTreeMap::new(),
            )
            .unwrap();
            assert!(
                matches!(session.capture_decision_chain_roots(&[], &snapshot), Err(StepFatal::InvariantViolation { description, .. }) if description.contains("observation clock"))
            );
        }
    }

    #[test]
    fn shared_root_context_keeps_own_facts_when_account_execution_order_changes() {
        let mut setup = probe_session().state.setup.clone();
        setup.npcs.inst_count = 2;
        let mut forward = GameSession::new(setup.clone(), 42).unwrap();
        let mut reverse = GameSession::new(setup, 42).unwrap();
        let observations =
            DecisionChainObservation::capture_for_roots(&forward, forward.build_market_view())
                .unwrap();
        let context = RootReadContext::capture(&forward).unwrap();
        let expected_equities =
            [AccountId(1), AccountId(2)].map(|id| forward.account_equity(id).unwrap());
        forward
            .state
            .accounts
            .get_mut(&AccountId(1))
            .unwrap()
            .fixture_set_cash(Money::from_cents(1));
        let observe = |session: &mut GameSession, ids: [AccountId; 2]| {
            ids.into_iter()
                .map(|id| {
                    let personal = PlanPersonalState::take(session, id);
                    let (_, personal, operations, _) =
                        InstitutionDecisionRoot::new(id, personal).observe(&context, &observations);
                    assert_eq!(
                        personal.belief.experience().reference_equity,
                        Some(expected_equities[id.0 as usize - 1])
                    );
                    assert_eq!(operations.len(), 2);
                    (
                        id,
                        serde_json::to_value((
                            &personal.belief,
                            &personal.information,
                            &personal.watchlist,
                            &personal.price_memory,
                        ))
                        .unwrap(),
                    )
                })
                .collect::<BTreeMap<_, _>>()
        };
        assert_eq!(
            observe(&mut forward, [AccountId(1), AccountId(2)]),
            observe(&mut reverse, [AccountId(2), AccountId(1)])
        );
    }

    #[test]
    fn technical_observation_uses_bounded_recent_samples_with_full_valid_count() {
        let mut session = probe_session();
        let code = session
            .state
            .candle_book
            .histories()
            .keys()
            .next()
            .unwrap()
            .clone();
        let template = session.state.candle_book.histories()[&code][0].clone();
        let all = (0..6_000)
            .map(|day| DailyCandle {
                time: i64::from(day) * 86_400,
                volume: if day % 5 == 0 { 0 } else { 100 },
                ..template.clone()
            })
            .collect::<Vec<_>>();
        let full_bars = all
            .iter()
            .enumerate()
            .map(|(day, candle)| TechnicalDailyInput {
                trading_day: day as u32,
                high: candle.high,
                low: candle.low,
                close: candle.close,
                volume: candle.volume,
            })
            .collect::<Vec<_>>();
        let reference = build_technical_observation(&full_bars, 6_000).unwrap();
        session
            .state
            .candle_book
            .replace_history(code.clone(), all.into());

        let observed = session.build_chain_technical_observations().unwrap();

        assert_eq!(observed[&code], reference);
        assert_eq!(observed[&code].valid_sample_count, 4_800);
    }

    /// 公共场景：真实链路跑出一个带在途子单的 Buy 计划（DeepValue 机构在
    /// 低价股上信念看多 → 计划 + Patient 子单挂在玩家买一上方）。
    fn seeded_buy_plan_with_child() -> GameSession {
        let mut session = probe_session();
        force_attention(&mut session, AccountId(1));
        let mut seed_events = Vec::new();
        session.seed_order_for_test(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockCode("000812".to_string()),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(280)),
                qty: 100,
            },
            &mut seed_events,
        );
        session.state.pending_npc = None;
        crate::session::pipeline::queue_npc_for_next_tick(&mut session).unwrap();
        session.step().expect("healthy step");
        // 场景断言：计划 + 在途子单确实就位（后续测试依赖）。
        let plans = session.plans_debug();
        assert!(
            plans.iter().any(|(account, code, direction, ..)| {
                account == &AccountId(1) && code == "000812" && direction == &Side::Buy
            }),
            "seed scenario must produce the institution buy plan: {plans:?}"
        );
        assert!(
            session
                .state
                .parent_orders
                .get(&AccountId(1))
                .and_then(|parents| parents.get(&StockCode("000812".to_string())))
                .and_then(|parent| parent.active_child_order_id())
                .is_some(),
            "seed scenario must leave an in-flight child order"
        );
        session
    }

    fn reversal_assessment(score_bp: i32) -> BTreeMap<StockCode, CandidateAssessment> {
        let score = crate::plans::SignalScore::new(score_bp)
            .expect("test scores stay within the -10000..=10000 contract");
        BTreeMap::from([(
            StockCode("000812".to_string()),
            CandidateAssessment::Scored {
                score,
                used_weight_bp: 10_000,
                excluded: Vec::new(),
            },
        )])
    }

    #[test]
    fn shrinking_a_live_child_below_its_remaining_exposure_requests_cancellation() {
        let mut session = seeded_buy_plan_with_child();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let plan = session.state.plans.active_plan(account, &code).unwrap();
        let parent = &session.state.parent_orders[&account][&code];
        let child_id = parent.active_child_order_id().unwrap();
        let child_exposure = plan.filled_qty() + parent.active_child_remaining_qty().unwrap();
        let market = session.build_market_view();
        let matching = (1..=10_000).find_map(|score| {
            let actions = session.collect_plan_lifecycle_actions(
                account,
                &reversal_assessment(score),
                &market,
                &session.state.plans,
            );
            actions.into_iter().find_map(|action| match action {
                PlanLifecycleAction::Restructure {
                    child_order_id: Some(order_id),
                    terminating: false,
                    revision,
                    ..
                } if order_id == child_id
                    && matches!(revision.target, PlanTarget::ShareCount(target) if target >= plan.filled_qty() && target < child_exposure) =>
                {
                    Some((score, revision))
                }
                _ => None,
            })
        });
        let (score, revision) = matching
            .expect("a smaller same-direction target must cancel the live child before revision");
        let events = drive_once(&mut session, &reversal_assessment(score));
        assert!(events.iter().any(|event| matches!(
            event,
            Event::OrderCanceled { id, .. } if *id == child_id
        )));
        let current = session.state.plans.active_plan(account, &code).unwrap();
        assert_eq!(current.target(), revision.target);
        assert_eq!(current.active_child_order_id(), None);
        assert_eq!(
            session.state.parent_orders[&account][&code].active_child_order_id(),
            None
        );
        assert!(!session.state.markets[&code]
            .resting_orders_for(account)
            .iter()
            .any(|order| order.id == child_id));
    }

    #[test]
    fn filled_child_cancel_failure_reconsiders_the_live_plan() {
        let mut session = seeded_buy_plan_with_child();
        let account = AccountId(1);
        let code = StockCode("000812".to_owned());
        let plan = session.state.plans.active_plan(account, &code).unwrap();
        let parent = &session.state.parent_orders[&account][&code];
        let child_id = parent.active_child_order_id().unwrap();
        let child_remaining = parent.active_child_remaining_qty().unwrap();
        let exposure = plan.filled_qty() + child_remaining;
        let market = session.build_market_view();
        let score = (1..=10_000)
            .find(|score| {
                session
                    .collect_plan_lifecycle_actions(
                        account,
                        &reversal_assessment(*score),
                        &market,
                        &session.state.plans,
                    )
                    .into_iter()
                    .any(|action| matches!(
                        action,
                        PlanLifecycleAction::Restructure {
                            child_order_id: Some(id),
                            revision: PlanRevision { target: PlanTarget::ShareCount(target), .. },
                            ..
                        } if id == child_id && target < exposure
                    ))
            })
            .expect("fixture must offer a smaller target than the old child's exposure");
        let mut roots = PlanChainOperationBatch::empty();
        roots.push_lifecycle(account, reversal_assessment(score), market.clone());
        roots.push_account_execution(account, market);
        let mut observation =
            crate::session::plan_chain_candidates::FrozenPlanChainObservation::capture(&session)
                .unwrap();
        let ready = roots
            .yield_adaptive_candidates(&mut session, &mut observation)
            .unwrap();
        assert_eq!(ready.len(), 1);
        assert!(matches!(ready[0].intent, Intent::Cancel { id, .. } if id == child_id));

        // The stock book fills the entire old child before the cancellation reaches it.
        let plan_id = session
            .state
            .plans
            .active_plan(account, &code)
            .unwrap()
            .plan_id();
        session
            .state
            .plans
            .apply(
                plan_id,
                PlanEvent::ChildOrderFilled {
                    order_id: child_id,
                    qty: child_remaining,
                    child_complete: true,
                    trading_day: u64::from(session.state.day),
                },
            )
            .unwrap();
        let parent = session
            .state
            .parent_orders
            .get_mut(&account)
            .unwrap()
            .get_mut(&code)
            .unwrap();
        parent.replace_execution_facts_for_test(parent.filled_qty() + child_remaining, None, None);
        session
            .state
            .markets
            .get_mut(&code)
            .unwrap()
            .cancel(child_id)
            .unwrap();
        assert_eq!(
            session.state.plans.plan(plan_id).unwrap().status(),
            PlanStatus::Active
        );

        roots
            .resume_adaptive_candidates(
                &mut session,
                [(
                    account,
                    code.clone(),
                    ready[0].chain_generation_index,
                    PlanRouteOutcome::Rejected(RejectionReason::OrderAlreadyFilled),
                )],
            )
            .unwrap();
        assert!(roots
            .yield_adaptive_candidates(&mut session, &mut observation)
            .unwrap()
            .is_empty());
        let reports = roots.finish_adaptive().unwrap();
        assert!(reports.iter().any(|report| matches!(
            report.disposition,
            PlanExecutionDisposition::RouteRejected {
                reason: RejectionReason::OrderAlreadyFilled
            }
        )));
        assert_eq!(
            session.state.plans.plan(plan_id).unwrap().status(),
            PlanStatus::Terminated {
                reason: TerminationReason::Cancelled
            }
        );
        assert!(session
            .state
            .parent_orders
            .get(&account)
            .and_then(|by_stock| by_stock.get(&code))
            .is_none());
    }

    fn drive_once(
        session: &mut GameSession,
        assessments: &BTreeMap<StockCode, CandidateAssessment>,
    ) -> Vec<Event> {
        observe_account_risk_for_review(session, AccountId(1));
        let mut plans = std::mem::take(&mut session.state.plans);
        let view = session.build_market_view();
        let mut operations = PlanChainOperationBatch::empty();
        let observation: &GameSession = session;
        let before = plans.clone();
        let actions =
            observation.collect_plan_lifecycle_actions(AccountId(1), assessments, &view, &plans);
        assert_eq!(
            plans, before,
            "deciding plan actions must not write the plan book"
        );
        super::apply_plan_lifecycle_actions(actions, session, &view, &mut plans, &mut operations);
        session.state.plans = plans;
        crate::session::pipeline::commit_injected_plan_roots_for_test(session, operations)
    }

    fn observe_account_risk_for_review(session: &mut GameSession, account: AccountId) {
        let equity = session.account_equity(account).unwrap();
        let observed = crate::experience::ExperienceMoment {
            civil_date: session.civil_date(),
            market_minute: session.current_market_minute(),
            trading_day: u64::from(session.state.day),
        };
        crate::session::institutional_behavior::observe_institution_account_risk(
            session
                .state
                .belief_participants
                .get_mut(&account)
                .map(|participant| participant.belief_mut())
                .unwrap(),
            equity,
            observed,
        )
        .unwrap();
    }

    fn plan_summary(session: &GameSession) -> (Side, u32, u32, String) {
        session
            .plans_debug()
            .into_iter()
            .find(|(account, ..)| account == &AccountId(1))
            .map(|(_, _, direction, target, filled, status)| (direction, target, filled, status))
            .expect("institution plan must exist")
    }

    #[test]
    fn mid_day_reversal_cancels_the_in_flight_child_before_flipping() {
        let mut session = seeded_buy_plan_with_child();
        let child_id = session
            .state
            .parent_orders
            .get(&AccountId(1))
            .and_then(|parents| parents.get(&StockCode("000812".to_string())))
            .and_then(|parent| parent.active_child_order_id())
            .expect("child must be in flight");

        // 反向信号跨过 ±2000bp 迟滞：Buy → Sell。
        let events = drive_once(&mut session, &reversal_assessment(-6_000));

        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::OrderCanceled { id, .. } if *id == child_id
            )),
            "the in-flight child must be really canceled BEFORE the reversal: {events:?}"
        );
        let (direction, _target, filled, status) = plan_summary(&session);
        assert_eq!(direction, Side::Sell, "plan must have flipped to Sell");
        assert_eq!(filled, 0, "ReverseRestart resets the fill progress");
        assert_eq!(status, "Active");
        // 撤单清空了母单在途子单引用——反向后的新子单不会被
        // 「第二在途子单」断言卡死（install_plan_parent 可覆盖安装）。
        assert!(
            session
                .state
                .parent_orders
                .get(&AccountId(1))
                .and_then(|parents| parents.get(&StockCode("000812".to_string())))
                .map(|parent| parent.active_child_order_id().is_none())
                .unwrap_or(true),
            "the linked parent must no longer carry the canceled child"
        );
    }

    #[test]
    fn mid_day_termination_cancels_the_in_flight_child_first() {
        let mut session = seeded_buy_plan_with_child();
        let child_id = session
            .state
            .parent_orders
            .get(&AccountId(1))
            .and_then(|parents| parents.get(&StockCode("000812".to_string())))
            .and_then(|parent| parent.active_child_order_id())
            .expect("child must be in flight");
        // 手工推进计划进度（计划簿事件路径）：filled 500_000 / target 599_300；
        // 同向弱信号（score 1000）修订后的目标差量 ≈13.9 万股 < 已成交 50 万股
        // ⇒ below_filled_rationale ⇒ 终止（真实撤子单先行）。
        {
            let mut plans = std::mem::take(&mut session.state.plans);
            plans
                .apply(
                    crate::plans::PlanId(0),
                    crate::plans::PlanEvent::ChildOrderAccepted {
                        order_id: OrderId(999),
                        trading_day: 0,
                    },
                )
                .unwrap();
            plans
                .apply(
                    crate::plans::PlanId(0),
                    crate::plans::PlanEvent::ChildOrderFilled {
                        order_id: OrderId(999),
                        qty: 500_000,
                        child_complete: false,
                        trading_day: 0,
                    },
                )
                .unwrap();
            session.state.plans = plans;
        }

        let events = drive_once(&mut session, &reversal_assessment(1_000));

        assert!(
            events.iter().any(|event| matches!(
                event,
                Event::OrderCanceled { id, .. } if *id == child_id
            )),
            "termination must really cancel the in-flight child first: {events:?}"
        );
        let (_, _, _, status) = plan_summary(&session);
        assert_eq!(
            status, "Terminated { reason: Cancelled }",
            "below-filled revision must terminate the plan"
        );
        // 终止路径连链接母单条目一并移除（计划已死，不再有后续子单）。
        assert!(
            session
                .state
                .parent_orders
                .get(&AccountId(1))
                .and_then(|parents| parents.get(&StockCode("000812".to_string())))
                .is_none(),
            "the dead linked parent entry must be removed"
        );
    }

    #[test]
    fn mid_day_termination_removes_a_linked_parent_without_an_active_child() {
        // Given: a plan-linked parent whose child has already resolved but whose plan remains active.
        let mut session = seeded_buy_plan_with_child();
        let code = StockCode("000812".to_string());
        let parent = session
            .state
            .parent_orders
            .get_mut(&AccountId(1))
            .and_then(|parents| parents.get_mut(&code))
            .expect("seeded parent must exist");
        parent.replace_execution_facts_for_test(parent.filled_qty(), None, None);
        {
            let mut plans = std::mem::take(&mut session.state.plans);
            plans
                .apply(
                    PlanId(0),
                    PlanEvent::ChildOrderAccepted {
                        order_id: OrderId(999),
                        trading_day: 0,
                    },
                )
                .unwrap();
            plans
                .apply(
                    PlanId(0),
                    PlanEvent::ChildOrderFilled {
                        order_id: OrderId(999),
                        qty: 500_000,
                        child_complete: false,
                        trading_day: 0,
                    },
                )
                .unwrap();
            session.state.plans = plans;
        }

        // When: the target is revised below the true fill, terminating the plan.
        let events = drive_once(&mut session, &reversal_assessment(1_000));

        // Then: no child cancellation is needed, but the dead plan cannot retain the parent slot.
        assert!(!events
            .iter()
            .any(|event| matches!(event, Event::OrderCanceled { .. })));
        assert_eq!(plan_summary(&session).3, "Terminated { reason: Cancelled }");
        assert!(
            session
                .state
                .parent_orders
                .get(&AccountId(1))
                .and_then(|parents| parents.get(&code))
                .is_none(),
            "a terminated plan without an active child must release its linked parent slot"
        );
    }

    #[test]
    fn mid_day_restructure_in_a_non_cancellable_phase_keeps_plan_and_child() {
        let mut session = seeded_buy_plan_with_child();
        let child_id = session
            .state
            .parent_orders
            .get(&AccountId(1))
            .and_then(|parents| parents.get(&StockCode("000812".to_string())))
            .and_then(|parent| parent.active_child_order_id())
            .expect("child must be in flight");
        // ticks_per_day=100、closing_auction_ticks=10 ⇒ tick ≥ 90 为收盘集合
        // 竞价（不可撤阶段）。
        session.state.tick = 95;
        session.state.pending_npc = Some(crate::session::PendingNpcBatch {
            dependencies: Vec::new(),
            observed_tick: session.state.tick,
            observed_accounts: Vec::new(),
            intents: Vec::new(),
        });

        let events = drive_once(&mut session, &reversal_assessment(-6_000));

        assert!(
            !events
                .iter()
                .any(|event| matches!(event, Event::OrderCanceled { .. })),
            "non-cancellable phase must not route a cancel: {events:?}"
        );
        let (direction, _target, filled, status) = plan_summary(&session);
        assert_eq!(direction, Side::Buy, "the plan must stay untouched (Keep)");
        assert_eq!(filled, 0);
        assert_eq!(status, "Active");
        // 旧子单保持自身生命周期（仍在簿上），冲突新单由执行适配器的
        // IncompatibleExecutionState 守卫抑制——绝不搁置孤儿。
        assert!(
            session
                .state
                .markets
                .get(&StockCode("000812".to_string()))
                .map(|market| market.resting_order_count() > 0)
                .unwrap_or(false),
            "the resting child must stay in the book"
        );
        assert_eq!(
            session
                .state
                .parent_orders
                .get(&AccountId(1))
                .and_then(|parents| parents.get(&StockCode("000812".to_string())))
                .and_then(|parent| parent.active_child_order_id()),
            Some(child_id),
            "the linked parent must still reference its child"
        );
    }

    fn probe_session() -> GameSession {
        let setup = SessionSetup {
            stocks: vec![StockSpec {
                code: StockCode("000812".to_string()),
                exchange: StockExchange::Shenzhen,
                initial_price: Money::from_cents(285),
                category: SecurityCategory::StMainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: 1_000_000,
                float_shares: 400_000,
            }],
            npcs: NpcSetup {
                retail_count: 0,
                inst_count: 1,
                hot_count: 0,
                retail_cash_median: Money::from_cents(60_000),
            },
            config: crate::config::GameConfig::proposed_defaults(),
            strategy_params: crate::strategy::StrategyParams {
                retail: crate::strategy::RetailParams {
                    arrival_rate: 0.0,
                    order_size_mean: 100,
                    chase_prob: 0.0,
                },
                inst: crate::strategy::InstParams {
                    margin: 0.05,
                    order_size: 1_000,
                },
                hot: crate::strategy::HotParams {
                    lookback: 3,
                    trend_threshold: 0.02,
                    order_size: 100,
                },
            },
            ticks_per_day: 100,
            auction_ticks: 0,
            closing_auction_ticks: 10,
            history_len: 5,
            t1_enabled: true,
            float_allocation: FloatAllocation::Random,
            start_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            simulation_policy_id: SIMULATION_POLICY_ID_V2.to_string(),
        };
        GameSession::new(setup, 42).unwrap()
    }

    /// Runs the plan-chain allocation, quote, and router path for a sell plan.  It uses a
    /// call-auction window to give the quote policy a protected two-sided empty-book quote,
    /// so the assertion observes a real routed child rather than a policy wait.
    fn route_sell_plan(holding_qty: u32, target_qty: u32) -> Vec<Event> {
        let mut session = probe_session();
        let owner = AccountId(1);
        let code = StockCode("000812".to_string());
        session.state.setup.auction_ticks = 10;
        session
            .state
            .accounts
            .get_mut(&owner)
            .unwrap()
            .fixture_insert_position(
                code.clone(),
                crate::Position::from_restored_parts(
                    holding_qty,
                    0,
                    i64::from(holding_qty) * 285,
                    0,
                ),
            );
        session
            .state
            .plans
            .create(PlanOpen {
                account: owner,
                code: code.clone(),
                direction: Side::Sell,
                target: PlanTarget::ShareCount(target_qty),
                opinion: PlanOpinion {
                    signal_score_bp: -8_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: Urgency::Normal,
                horizon_trading_days: 5,
                created_trading_day: u64::from(session.state.day),
            })
            .unwrap();
        let mut plans = std::mem::take(&mut session.state.plans);
        let view = session.build_market_view();
        let mut operations = PlanChainOperationBatch::empty();

        session
            .synchronize_owned_plan_execution(&mut plans)
            .expect("fixture plan facts must synchronize");
        let observation: &GameSession = &session;
        if let Some(cursor) = observation.prepare_plan_quotes_for_account(owner, &view, &plans) {
            operations.push_quote_plans(cursor);
        }
        session.state.plans = plans;
        crate::session::pipeline::commit_injected_plan_roots_for_test(&mut session, operations)
    }

    fn force_attention(session: &mut GameSession, id: AccountId) {
        let attention = session.state.npc_attention.get_mut(&id).unwrap();
        attention.base_probability = 1.0;
        attention.next_attention_candidate_tick = 0;
        session.state.attention_scheduler.enqueue(0, id);
    }

    #[test]
    fn restructure_rejection_preserves_old_plan_parent_order_and_reservation() {
        let mut session = seeded_buy_plan_with_child();
        let code = StockCode("000812".to_string());
        let owner = AccountId(1);
        let old = session
            .state
            .plans
            .active_plan(owner, &code)
            .unwrap()
            .clone();
        let parents = serde_json::to_value(&session.state.parent_orders).unwrap();
        let reservation = session.reserved_cash_for_account(owner).unwrap();
        let order_count = session.state.markets[&code].resting_order_count();
        let mut batch = PlanChainOperationBatch::empty();
        batch.push_restructure(
            owner,
            old.plan_id(),
            code.clone(),
            Some(OrderId(999_999)),
            false,
            PlanRevision {
                reason: RevisionReason::SignalShift,
                trading_day: u64::from(session.state.day),
                direction: Side::Sell,
                target: PlanTarget::ShareCount(100),
                opinion: PlanOpinion {
                    signal_score_bp: -6_000,
                    source: OpinionSource::Blended,
                },
                confidence_bp: 8_000,
                urgency: Urgency::Normal,
                below_filled_rationale: None,
            },
        );
        let events =
            crate::session::pipeline::commit_injected_plan_roots_for_test(&mut session, batch);

        let current = session.state.plans.plan(old.plan_id()).unwrap();
        assert_eq!(current.direction(), old.direction());
        assert_eq!(current.target(), old.target());
        assert_eq!(current.filled_qty(), old.filled_qty());
        assert_eq!(current.active_child_order_id(), old.active_child_order_id());
        assert_eq!(
            serde_json::to_value(&session.state.parent_orders).unwrap(),
            parents
        );
        assert_eq!(
            session.reserved_cash_for_account(owner).unwrap(),
            reservation
        );
        assert_eq!(
            session.state.markets[&code].resting_order_count(),
            order_count
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(
                    event,
                    Event::IntentRejected {
                        reason: RejectionReason::OrderNotFound,
                        ..
                    }
                ))
                .count(),
            1
        );
    }
}

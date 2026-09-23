//! 完整决策链（任务 26）：信念机构账户的 K5a/K6 编排。
//!
//! 链条（每次 accepted 注意力触发，按 [`AccountId`] 序串行执行）：
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
//!    `AnalysisProfile` 权重混合（`plans::blend_candidate`）。机构的成本经历
//!    路径不存在（任务 20 面向自然人）——诚实标记不可用并由可用权重重归一。
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
//! 8. **执行**：`execute_plan_observation` 经真实路由提交/认领/替换子单
//!    （只有真实成交推进 `filled_qty`；与普通意图物化路径互斥）。
//!
//! 确定性：无链内独立 RNG（发现用个体注意力流）；候选/计划/请求全部按
//! 有序集合遍历；同 seed 同事件（链在 step 的串行段执行，跨 CPU 线程数不变）。

use super::*;

use crate::information::{discovery_candidates, NpcObservationContext, PublicationId};
use crate::observation::{build_technical_observation, TechnicalDailyInput, TechnicalObservation};
use crate::plans::{
    allocate_soft_budgets, assess_urgency, blend_candidate, decide_quote,
    reverse_crosses_threshold, target_position_weight_bp, target_share_quantity, ActiveQuote,
    AllocationClass, AllocationExperience, AllocationFunds, AllocationPolicy, AllocationRequest,
    AllocationResult, BookTop, CandidateAssessment, CandidateError, CandidateSignals,
    OpinionSource, PatienceStyle, PlanBook, PlanEvent, PlanId, PlanOpen, PlanOpinion, PlanRevision,
    PlanStatus, PlanTarget, QuoteDecisionInputs, RevisionReason, SignalContribution,
    SignalUnavailableReason, TerminationReason, TradingPlan, Urgency, UrgencyInputs, UrgencyPolicy,
};
use crate::session::plan_chain_candidates::PlanChainOperationBatch;
use crate::strategy::{BeliefBook, BeliefCause, BeliefInputs, PerShareRange, ValuationOutcome};
mod quote;

/// FNV-1a(tag + account id) → 与 seed 混合的独立流种子（分析档案/信念假设等
/// per-NPC 一次性抽样专用；绝不与 self.rng 或注意力流共用）。
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

impl GameSession {
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
        if snapshot.tick() != self.tick
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
        let mut accounts = Vec::new();
        for id in accepted_due_npc_ids {
            let account = self
                .accounts
                .get(id)
                .ok_or_else(|| invariant(format!("plan-chain root account {id:?} is absent")))?;
            snapshot
                .account(*id)
                .map_err(|error| invariant(error.to_string()))?;
            if account
                .strategy
                .as_ref()
                .is_some_and(|strategy| strategy.belief_chain_params().is_some())
            {
                accounts.push(*id);
            }
        }
        if accounts.is_empty() {
            return Ok(PlanChainOperationBatch::empty());
        }
        let now = self.chain_observation_instant();
        Ok(PlanChainOperationBatch::accounts(
            accounts,
            snapshot.market().clone(),
            self.market_price_path_observations()
                .map_err(|error| invariant(error.to_string()))?,
            self.build_chain_technical_observations(),
            now,
            self.chain_exposed_stocks(now),
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
                self.accounts
                    .get(*id)
                    .and_then(|account| account.strategy.as_ref())
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
        let technical = self.build_chain_technical_observations();
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
        operations.prepare_next_account(self);
        operations
    }

    /// 会话簿同步：只应用「本簿仍存活」的计划事件；外部计划簿/已终止计划
    /// 的迟到条目保留在 pending 队列（`synchronize_plan_execution` 的容错
    /// 语义——任务 26 起会话簿与外部簿共用该队列）。
    fn sync_session_plan_book(&mut self, plans: &mut PlanBook) {
        self.synchronize_plan_execution(plans)
            .unwrap_or_else(|error| panic!("session plan synchronization failed: {error}"));
    }

    /// 日终扫描（step 的日界段调用）：先同步 accepted/fill/day-end 事实，
    /// 再对全部非终止计划补 `TradingDayEnded`（清子单引用；跨过有效期的
    /// 计划就地到期终止，不搁浅在 Active——任务 21 教训的补账路径）。
    #[cfg(test)]
    pub(super) fn sweep_decision_chain_day_end(&mut self) {
        let trading_day = u64::from(self.day);
        let mut plans = std::mem::take(&mut self.plans);
        self.sync_session_plan_book(&mut plans);
        let plan_ids: Vec<PlanId> = plans.plan_ids().collect();
        for plan_id in plan_ids {
            let terminal = plans
                .plan(plan_id)
                .expect("collected plan ids must resolve")
                .is_terminal();
            if !terminal {
                plans
                    .apply(plan_id, PlanEvent::TradingDayEnded { trading_day })
                    .unwrap_or_else(|error| {
                        panic!("day-end plan sweep failed for {plan_id:?}: {error}")
                    });
            }
        }
        self.plans = plans;
    }

    /// 逐日 K → 技术指标观测（全部股票；candle 序号即样本键，观察时点 =
    /// 「下一个序号」——预置历史先于第 0 个交易日，天然满足严格早于观察日）。
    fn build_chain_technical_observations(&self) -> BTreeMap<StockCode, TechnicalObservation> {
        self.daily_candles
            .iter()
            .map(|(code, candles)| {
                let as_of_trading_day =
                    u32::try_from(candles.len()).expect("daily candle count fits u32");
                let bars: Vec<TechnicalDailyInput> = candles
                    .iter()
                    .enumerate()
                    .map(|(index, candle)| TechnicalDailyInput {
                        trading_day: u32::try_from(index)
                            .expect("daily candle index fits u32 trading day"),
                        high: candle.high,
                        low: candle.low,
                        close: candle.close,
                        volume: candle.volume,
                    })
                    .collect();
                let observation = build_technical_observation(&bars, as_of_trading_day)
                    .unwrap_or_else(|error| {
                        panic!("technical observation for {code:?} failed: {error}")
                    });
                (code.clone(), observation)
            })
            .collect()
    }

    fn chain_observation_instant(&self) -> crate::calendar::CivilInstant {
        self.observation_civil_instant()
    }

    /// 曝光股票集合：公布/公告落在新鲜度窗口内的发行人股票（任务 25 发现
    /// 权重的公告加成输入；公司 ↔ 股票映射 = 发行人注册表）。
    fn chain_exposed_stocks(&self, now: crate::calendar::CivilInstant) -> BTreeSet<StockCode> {
        let mut threshold = now.date();
        for _ in 0..EXPOSURE_FRESHNESS_DAYS {
            threshold = threshold
                .prev()
                .expect("runtime window stays above the civil floor");
        }
        let mut exposed = BTreeSet::new();
        for stock in &self.setup.stocks {
            let Some(company) = self.company_registry.issuer_of(&stock.code) else {
                continue;
            };
            let fresh = self
                .library
                .reports_for_company(company, now)
                .iter()
                .any(|report| report.published_at.date() >= threshold)
                || self
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

    #[allow(clippy::too_many_arguments)]
    pub(in crate::session) fn run_chain_for_account(
        &mut self,
        id: AccountId,
        market_view: &MarketView,
        price_paths: &BTreeMap<StockCode, crate::observation::PricePathObservation>,
        technical: &BTreeMap<StockCode, TechnicalObservation>,
        now: crate::calendar::CivilInstant,
        exposed: &BTreeSet<StockCode>,
        plans: &mut PlanBook,
        operations: &mut PlanChainOperationBatch,
    ) {
        let held: BTreeSet<StockCode> = self.accounts[&id].positions.keys().cloned().collect();

        // 1. 个体发现（消费注意力个体流；接受即写入关注列表）。
        let mut watchlist = self
            .watchlists
            .get(&id)
            .cloned()
            .unwrap_or_else(|| panic!("belief account {id:?} must have a watchlist"));
        let discovered = self
            .npc_attention
            .get_mut(&id)
            .map(|attention| {
                attention.sample_discovery_stock(market_view, &held, &watchlist, exposed)
            })
            .unwrap_or_else(|| panic!("belief account {id:?} must have attention state"));
        let market_minute = self.current_market_minute();
        if let Some(code) = &discovered {
            watchlist
                .record_attention(code, market_minute, market_minute)
                .unwrap_or_else(|error| {
                    panic!("watchlist attention failed for {id:?} {code:?}: {error}")
                });
        }

        // 候选集：持仓 ∪ 既有信念条目 ∪ 本次发现（有序集合 → 确定序）。
        let mut candidates: BTreeSet<StockCode> = held.clone();
        candidates.extend(
            self.belief_books
                .get(&id)
                .unwrap_or_else(|| panic!("belief account {id:?} must have a belief book"))
                .entry_stocks()
                .cloned(),
        );
        if let Some(code) = discovered {
            candidates.insert(code);
        }
        let price_memory = self
            .price_memories
            .get_mut(&id)
            .unwrap_or_else(|| panic!("belief account {id:?} must have price memory"));
        for code in &candidates {
            price_memory
                .observe_price(code, self.markets[code].last_price(), market_minute)
                .unwrap_or_else(|error| {
                    panic!("price-memory observation failed for {id:?} {code:?}: {error}")
                });
        }

        // 2. 公共曝光 → 显式获知（新年报留下 id 供信念更新）。
        #[cfg(feature = "simulation-diagnostics")]
        {
            let sequence = self.causal.facts.len() as u64;
            self.causal.decision.insert(id, sequence);
            self.causal_record(crate::diagnostics::causal::CausalFactKind::Decision {
                account: id,
            });
        }
        let mut new_annual_reports: Vec<(StockCode, PublicationId)> = Vec::new();
        for code in &candidates {
            let Some(company_id) = self.company_registry.issuer_of(code).cloned() else {
                continue;
            };
            for publication_id in discovery_candidates(&self.library, &company_id, now) {
                let already = self.information[&id]
                    .records_for_company(&company_id)
                    .iter()
                    .any(|record| record.id == publication_id);
                if already {
                    continue;
                }
                let state = self
                    .information
                    .get_mut(&id)
                    .unwrap_or_else(|| panic!("belief account {id:?} must have information state"));
                state
                    .record_acquisition(id, &self.library, publication_id, now)
                    .unwrap_or_else(|error| {
                        panic!(
                            "acquisition failed for account {id:?} publication {publication_id:?}: {error}"
                        )
                    });
                let is_annual = self
                    .library
                    .report(publication_id, now)
                    .map(|report| {
                        report.reports.kind == crate::accounting::reports::ReportKind::Annual
                    })
                    .unwrap_or(false);
                #[cfg(feature = "simulation-diagnostics")]
                {
                    let published = self
                        .library
                        .report(publication_id, now)
                        .map(|report| report.published_at)
                        .or_else(|_| {
                            self.library
                                .announcement(publication_id, now)
                                .map(|announcement| announcement.published_at)
                        })
                        .expect("successful acquisition resolves a public publication");
                    self.causal_record(crate::diagnostics::causal::CausalFactKind::Acquisition {
                        account: id,
                        company: company_id.clone(),
                        publication: u64::from(publication_id.value()),
                        published,
                        acquired: now,
                    });
                }
                if is_annual {
                    new_annual_reports.push((code.clone(), publication_id));
                }
            }
        }

        // 3. 信念更新（新材料/到期 cause；估值不可用由条目自身承载）。
        // 借用纪律：观察上下文与发行人输入先装配（局部值），再独占借用信念簿
        //（information/library 只读借用与 belief_books 可变借用是不同字段）。
        let info = self
            .information
            .get(&id)
            .unwrap_or_else(|| panic!("belief account {id:?} must have information state"));
        let ctx = NpcObservationContext::new(id, info, &self.library, market_view)
            .unwrap_or_else(|error| panic!("observation context failed for {id:?}: {error}"));
        let as_of_trading_day = u64::from(self.day);
        let issuer_inputs: Vec<(StockCode, BeliefInputs<'_, MarketView>)> = candidates
            .iter()
            .filter_map(|code| {
                let company = self.company_registry.issuer_of(code).cloned()?;
                let spec = self
                    .operations
                    .company(&company)
                    .unwrap_or_else(|| panic!("issuer {company:?} must have operating books"))
                    .spec();
                Some((
                    code.clone(),
                    BeliefInputs {
                        ctx: &ctx,
                        company,
                        kind: spec.kind,
                        total_issued_shares: spec.issued_shares,
                        as_of_trading_day,
                    },
                ))
            })
            .collect();
        {
            let belief = self
                .belief_books
                .get_mut(&id)
                .unwrap_or_else(|| panic!("belief account {id:?} must have a belief book"));
            for (code, publication_id) in &new_annual_reports {
                let Some((_, inputs)) = issuer_inputs
                    .iter()
                    .find(|(candidate, _)| candidate == code)
                else {
                    panic!("annual report for {code:?} must have issuer inputs");
                };
                belief
                    .apply_cause(
                        code,
                        BeliefCause::NewMaterial {
                            report: *publication_id,
                        },
                        inputs,
                    )
                    .unwrap_or_else(|error| {
                        panic!("belief update failed for {id:?} {code:?}: {error}")
                    });
            }
            for (code, inputs) in &issuer_inputs {
                let Some(entry) = belief.entry(code) else {
                    continue;
                };
                let expiry = entry.anchor_trading_day + u64::from(entry.horizon_trading_days);
                if as_of_trading_day >= expiry {
                    belief
                        .apply_cause(code, BeliefCause::HorizonExpired, inputs)
                        .unwrap_or_else(|error| {
                            panic!("belief horizon expiry failed for {id:?} {code:?}: {error}")
                        });
                }
            }
        }

        // 4–5. K5a 聚合 + 计划生命周期。
        let assessments =
            self.assess_candidates(id, &candidates, market_view, price_paths, technical);
        operations.push_lifecycle(id, assessments, market_view.clone());

        // 6–8. 预算/紧迫度/报价/执行（覆盖账户全部活跃计划，含既有）。
        operations.push_account_execution(id, market_view.clone());

        #[cfg(feature = "simulation-diagnostics")]
        self.record_npc_decision_trace(id, &new_annual_reports, &candidates, plans, &[]);

        // 关注列表修剪：持仓 ∪ 活跃计划股票受保护（永不被驱逐）。
        let protected: BTreeSet<StockCode> = held
            .into_iter()
            .chain(self.active_plan_codes(id, plans))
            .collect();
        watchlist.prune(&protected);
        self.watchlists.insert(id, watchlist);
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
                    .is_ok_and(|plan| plan.account == account)
            })
            .collect();
        let expectation_method = self
            .belief_books
            .get(&account)
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
            .filter(|plan| plan.account == account && plan.is_terminal())
            .map(|plan| format!("{:?}", plan.status))
            .collect();
        self.npc_decision_traces.record(NpcDecisionTraceRecord {
            account,
            tick: self.tick,
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

    /// K5a 五路信号聚合（逐候选股票）。错误的输入（非正价、区间倒置等）
    /// 是编程缺陷 ⇒ 显式 panic（铁律二：不静默 fallback）。
    fn assess_candidates(
        &self,
        id: AccountId,
        candidates: &BTreeSet<StockCode>,
        market_view: &MarketView,
        price_paths: &BTreeMap<StockCode, crate::observation::PricePathObservation>,
        technical: &BTreeMap<StockCode, TechnicalObservation>,
    ) -> BTreeMap<StockCode, CandidateAssessment> {
        let belief = self
            .belief_books
            .get(&id)
            .unwrap_or_else(|| panic!("belief account {id:?} must have a belief book"));
        let weights = belief.analysis().weights();
        let mut out = BTreeMap::new();
        for code in candidates {
            let Some(view) = market_view.stocks.get(code) else {
                continue;
            };
            let signals = self
                .build_candidate_signals(
                    belief,
                    code,
                    view,
                    price_paths.get(code),
                    technical.get(code),
                )
                .unwrap_or_else(|error| {
                    panic!("candidate signals failed for {id:?} {code:?}: {error}")
                });
            let assessment = blend_candidate(&weights, &signals);
            out.insert(code.clone(), assessment);
        }
        out
    }

    fn build_candidate_signals(
        &self,
        belief: &BeliefBook,
        code: &StockCode,
        view: &StockView,
        path: Option<&crate::observation::PricePathObservation>,
        technical: Option<&TechnicalObservation>,
    ) -> Result<CandidateSignals, CandidateError> {
        use crate::plans::{
            fundamental_signal, normalized_score, price_volume_signal, technical_signal,
            trend_signal,
        };
        let current = view.last_price;
        let fundamental = match belief.entry(code) {
            Some(entry) => fundamental_signal(current, &entry.valuation)?,
            None => {
                SignalContribution::unavailable(SignalUnavailableReason::FundamentalUnavailable)
            }
        };
        let no_return = crate::observation::HorizonReturn {
            requested_span: 0,
            available_span: 0,
            return_ratio: None,
        };
        let empty_path = crate::observation::PricePathObservation {
            one_minute: no_return,
            thirty_minute: no_return,
            intraday: no_return,
            five_day: no_return,
            twenty_day: no_return,
            one_hundred_twenty_day: no_return,
            two_hundred_fifty_day: no_return,
            prior_thirty_minute_range: None,
        };
        let path = path.unwrap_or(&empty_path);
        let thirty_bp = ratio_to_bp(path.thirty_minute.return_ratio);
        let five_day_bp = ratio_to_bp(path.five_day.return_ratio);
        let trend = trend_signal(thirty_bp, five_day_bp)?;
        let relative_volume_bp = view
            .relative_volume
            .is_finite()
            .then(|| ratio_to_bp(Some(view.relative_volume)))
            .flatten();
        let imbalance_bp = view
            .order_book_imbalance
            .is_finite()
            .then(|| ratio_to_bp(Some(view.order_book_imbalance)))
            .flatten();
        let imbalance_score = imbalance_bp
            .map(|bp| normalized_score(i64::from(bp), 10_000))
            .transpose()?;
        let price_volume = price_volume_signal(thirty_bp, relative_volume_bp, imbalance_score)?;
        let no_history = |required| crate::strategy::TechnicalError::InsufficientHistory {
            available: 0,
            required,
        };
        let unavailable_technical = TechnicalObservation {
            valid_sample_count: 0,
            sma20: Err(no_history(crate::strategy::SMA_SHORT_WINDOW)),
            sma60: Err(no_history(crate::strategy::SMA_LONG_WINDOW)),
            rsi14: Err(no_history(crate::strategy::RSI_WINDOW)),
            atr14: Err(no_history(crate::strategy::ATR_WINDOW)),
        };
        let technical_observation = technical.unwrap_or(&unavailable_technical);
        let technical = technical_signal(
            &technical_observation.sma20,
            &technical_observation.sma60,
            &technical_observation.rsi14,
        )?;
        // 机构的自然人成本经历路径不存在（任务 20 面向散户）：诚实标记
        // 不可用，由 blend 按可用权重重归一（绝不零填充伪装中性）。
        let experience =
            SignalContribution::unavailable(SignalUnavailableReason::MissingObservation);
        Ok(CandidateSignals {
            fundamental,
            trend,
            price_volume,
            technical,
            experience,
        })
    }

    /// 计划生命周期驱动：新开/修订/平静观察。基本面估值不可用 ⇒ 不产生
    /// 新的方向性动作（Watch/InsufficientInformation 语义）。
    ///
    /// 日中终止/反向修订前必须先真实撤销在途子单（task-24 复核移交项）：
    /// 见 [`Self::cancel_in_flight_child_before_restructure`]。
    pub(in crate::session) fn drive_plans_for_account(
        &mut self,
        id: AccountId,
        assessments: &BTreeMap<StockCode, CandidateAssessment>,
        market_view: &MarketView,
        plans: &mut PlanBook,
        operations: &mut PlanChainOperationBatch,
    ) {
        let trading_day = u64::from(self.day);
        let lot = self.setup.config.lot_size;
        // 持仓快照先行复制：循环内的撤单/修订需要 &mut self，不能长持账户借用。
        let held_by_code: BTreeMap<StockCode, u32> = self
            .accounts
            .get(&id)
            .unwrap_or_else(|| panic!("belief account {id:?} must exist"))
            .positions
            .iter()
            .map(|(code, position)| (code.clone(), position.qty))
            .collect();
        let equity = self
            .account_equity(id)
            .unwrap_or_else(|error| panic!("equity for {id:?} failed: {error}"));
        if equity.cents() <= 0 {
            return;
        }
        // 信念快照同样先行复制（估值可用性 + 信心/期限在循环内只读；撤单
        // 需要 &mut self，不能长持 belief_books 借用）。
        let belief = self
            .belief_books
            .get(&id)
            .unwrap_or_else(|| panic!("belief account {id:?} must have a belief book"))
            .clone();
        let chain_params = self.chain_strategy_params(id);
        let max_fraction_bp = (chain_params.max_stock_fraction * 10_000.0).min(10_000.0) as u32;
        for (code, assessment) in assessments {
            let view = market_view
                .stocks
                .get(code)
                .unwrap_or_else(|| panic!("candidate {code:?} must have a market view"));
            let price = view.last_price;
            let held_qty = held_by_code.get(code).copied().unwrap_or(0);
            let position_value = price
                .mul_shares(held_qty)
                .unwrap_or_else(|error| panic!("position value failed for {id:?}: {error}"));
            let current_weight_bp = u32::try_from(
                (i128::from(position_value.cents()) * 10_000 / i128::from(equity.cents()))
                    .clamp(0, 10_000),
            )
            .expect("clamped weight fits u32");
            // 基本面可用性：聚合给出分数，且个人估值确实可用（Available）。
            let (score, fundamental_ready) = match assessment {
                CandidateAssessment::Scored { score, .. } => (Some(*score), true),
                CandidateAssessment::InsufficientInformation { .. } => (None, false),
            };
            let fundamental_ready = fundamental_ready
                && belief.entry(code).is_some_and(|entry| {
                    matches!(entry.valuation, ValuationOutcome::Available { .. })
                });
            let direction = match (fundamental_ready, score) {
                (true, Some(score)) if score.value() > 0 => Some(Side::Buy),
                (true, Some(score)) if score.value() < 0 => Some(Side::Sell),
                _ => None,
            };
            let target_qty = match (score, fundamental_ready) {
                (Some(score), true) => {
                    let mut weight =
                        target_position_weight_bp(current_weight_bp, score, max_fraction_bp)
                            .unwrap_or_else(|error| {
                                panic!("target weight failed for {id:?} {code:?}: {error}")
                            });
                    // 可表达性上界（文档化游戏边界，非静默夹取）：目标股数
                    // 不能超过该股总股本，也不能超过 u32 计数；权重按此收缩。
                    let max_holdable_shares = self
                        .setup
                        .stocks
                        .iter()
                        .find(|stock| stock.code == *code)
                        .map(|stock| stock.total_shares.min(u64::from(u32::MAX)))
                        .unwrap_or(0);
                    let max_holdable_bp = u32::try_from(
                        ((i128::from(max_holdable_shares) * i128::from(price.cents())) * 10_000
                            / i128::from(equity.cents()))
                        .clamp(0, 10_000),
                    )
                    .unwrap_or(10_000);
                    weight = weight.min(max_holdable_bp);
                    target_share_quantity(weight, equity, price, lot)
                        .map(|result| result.target_qty)
                        .unwrap_or_else(|error| {
                            panic!("target quantity failed for {id:?} {code:?}: {error}")
                        })
                }
                _ => 0,
            };
            let active = plans.active_plan(id, code).cloned();
            match active {
                Some(plan) => {
                    let plan_id = plan.plan_id;
                    if plan.is_terminal() || trading_day > plan.last_valid_trading_day() {
                        continue; // 日终扫描负责到期终止
                    }
                    let Some(score) = score.filter(|_| fundamental_ready) else {
                        Self::observe_plan(plans, plan_id, trading_day);
                        continue;
                    };
                    let new_direction = direction.unwrap_or(plan.direction);
                    let flip = new_direction != plan.direction;
                    if flip
                        && !reverse_crosses_threshold(
                            plan.direction,
                            new_direction,
                            score.value(),
                            plans.policy().reverse_revision_threshold_bp,
                        )
                    {
                        Self::observe_plan(plans, plan_id, trading_day);
                        continue;
                    }
                    let Some(delta) =
                        desired_delta_shares(new_direction, target_qty, held_qty, lot)
                    else {
                        Self::observe_plan(plans, plan_id, trading_day);
                        continue;
                    };
                    let confidence = belief
                        .entry(code)
                        .map(|entry| u32::from(entry.confidence_bp))
                        .unwrap_or(5_000);
                    let opinion = PlanOpinion {
                        signal_score_bp: score.value(),
                        source: OpinionSource::Blended,
                    };
                    let same_direction = !flip;
                    let unchanged = same_direction
                        && plan.target == PlanTarget::ShareCount(delta)
                        && plan.confidence_bp == confidence
                        && plan.opinion == opinion;
                    if unchanged {
                        Self::observe_plan(plans, plan_id, trading_day);
                        continue;
                    }
                    let revision = PlanRevision {
                        reason: RevisionReason::SignalShift,
                        trading_day,
                        direction: new_direction,
                        target: PlanTarget::ShareCount(delta),
                        opinion,
                        confidence_bp: confidence,
                        urgency: Urgency::Normal,
                        below_filled_rationale: (same_direction && delta < plan.filled_qty)
                            .then_some(TerminationReason::Cancelled),
                    };
                    // task-24 复核移交项（本轮修复）：终止/反向修订会结束旧腿，
                    // 在途子单必须先经真实路由撤销——否则子单被搁置在市场继续
                    // 成交到日终，且反向后的新子单会触发
                    // record_parent_order_submission 的「第二在途子单」断言。
                    let will_terminate = revision.below_filled_rationale.is_some();
                    if will_terminate || flip {
                        let child_order_id = self
                            .parent_orders
                            .get(&id)
                            .and_then(|parents| parents.get(code))
                            .filter(|parent| parent.linked_plan_id == Some(plan_id))
                            .and_then(|parent| parent.active_child_order_id);
                        if child_order_id.is_some() && !self.plan_child_is_cancellable_now() {
                            Self::observe_plan(plans, plan_id, trading_day);
                            continue;
                        }
                        operations.push_restructure(
                            id,
                            plan_id,
                            code.clone(),
                            child_order_id,
                            will_terminate,
                            revision,
                        );
                        continue;
                    }
                    plans
                        .apply(plan_id, PlanEvent::Revised { revision })
                        .unwrap_or_else(|error| {
                            panic!("plan revision failed for {plan_id:?}: {error}")
                        });
                }
                None => {
                    let Some(direction) = direction else {
                        continue;
                    };
                    let Some(delta) = desired_delta_shares(direction, target_qty, held_qty, lot)
                    else {
                        continue;
                    };
                    let Some(entry) = belief.entry(code) else {
                        continue;
                    };
                    let open = PlanOpen {
                        account: id,
                        code: code.clone(),
                        direction,
                        target: PlanTarget::ShareCount(delta),
                        opinion: PlanOpinion {
                            signal_score_bp: score.map(|value| value.value()).unwrap_or_default(),
                            source: OpinionSource::Blended,
                        },
                        confidence_bp: u32::from(entry.confidence_bp),
                        urgency: Urgency::Normal,
                        horizon_trading_days: u32::from(entry.horizon_trading_days).max(1),
                        created_trading_day: trading_day,
                    };
                    plans.create(open).unwrap_or_else(|error| {
                        panic!("plan creation failed for {id:?} {code:?}: {error}")
                    });
                }
            }
        }
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

    fn active_plan_codes(&self, id: AccountId, plans: &PlanBook) -> Vec<StockCode> {
        self.setup
            .stocks
            .iter()
            .filter(|stock| plans.active_plan(id, &stock.code).is_some())
            .map(|stock| stock.code.clone())
            .collect()
    }

    /// 信念机构的链参数（能力探针；非信念策略在调用方已被过滤）。
    fn chain_strategy_params(&self, id: AccountId) -> crate::strategy::BeliefChainParams {
        self.accounts
            .get(&id)
            .and_then(|account| account.strategy.as_ref())
            .and_then(|strategy| strategy.belief_chain_params())
            .unwrap_or_else(|| panic!("belief account {id:?} must expose chain params"))
    }

    /// 预算/紧迫度/报价/执行：对账户全部非终止 ShareCount 目标计划走真实
    /// 路由。事件按 plan_id 升序的确定顺序追加。
    pub(in crate::session) fn execute_plans_for_account(
        &mut self,
        id: AccountId,
        market_view: &MarketView,
        plans: &mut PlanBook,
        candidates: &mut PlanChainOperationBatch,
    ) {
        let account_equity = self
            .account_equity(id)
            .unwrap_or_else(|error| panic!("equity for {id:?} failed: {error}"));
        // 先同步在途成交事实：remaining 以同步后的计划为准（否则本循环内
        // 的提交会与 execute_plan_observation 内部的同步剩余量错位）。
        self.sync_session_plan_book(plans);
        let active_plans: Vec<TradingPlan> = plans
            .plan_ids()
            .filter_map(|plan_id| {
                let plan = plans
                    .plan(plan_id)
                    .expect("collected plan ids must resolve");
                (plan.account == id && !plan.is_terminal()).then(|| plan.clone())
            })
            .collect();
        if active_plans.is_empty() {
            return;
        }
        let chain_params = self.chain_strategy_params(id);
        // 账户事实先取局部值（后续 execute_plan_observation 需要 &mut self，
        // 不得长持账户引用）。
        let account_cash = self.accounts[&id].cash;
        let sellable_by_code: BTreeMap<StockCode, u32> = self.accounts[&id]
            .positions
            .keys()
            .map(|code| (code.clone(), self.accounts[&id].sellable_qty(code)))
            .collect();
        // 6. 软预算（权威现金 = 账户现金；在途冻结单独传入，不由分配器重复计）。
        let frozen = self
            .reserved_cash_for_account(id)
            .unwrap_or_else(|error| panic!("reserved cash for {id:?} failed: {error}"));
        let funds = AllocationFunds {
            cash: account_cash,
            frozen_cash: frozen,
            equity: account_equity,
        };
        let requests: Vec<AllocationRequest> = active_plans
            .iter()
            .filter(|plan| matches!(plan.status, PlanStatus::Active))
            .filter_map(|plan| {
                let remaining = plan.remaining_share_qty()?;
                match plan.direction {
                    Side::Buy => {
                        let stock = self
                            .setup
                            .stocks
                            .iter()
                            .find(|stock| stock.code == plan.code)
                            .unwrap_or_else(|| {
                                panic!("plan stock {:?} must have a spec", plan.code)
                            });
                        // Quotes may move to any legal buy limit up to the daily upper band.
                        // Reserve against that upper bound, but only for the next routable child;
                        // the remaining parent target is not a live order or a fee obligation.
                        let reservation_price = self
                            .markets
                            .get(&plan.code)
                            .unwrap_or_else(|| {
                                panic!("plan stock {:?} must have a market", plan.code)
                            })
                            .up_stop()
                            .unwrap_or_else(|error| {
                                panic!("up stop failed for {:?}: {error}", plan.code)
                            });
                        buy_allocation_request(
                            &self.setup.config,
                            plan.plan_id,
                            plan.code.clone(),
                            plan.confidence_bp,
                            reservation_price,
                            remaining,
                            chain_params.order_size,
                            self.setup.config.lot_size,
                            stock.category.max_order_qty(false),
                        )
                    }
                    Side::Sell => {
                        let sellable = sellable_by_code.get(&plan.code).copied().unwrap_or(0);
                        let stock = self
                            .setup
                            .stocks
                            .iter()
                            .find(|stock| stock.code == plan.code)
                            .unwrap_or_else(|| {
                                panic!("plan stock {:?} must have a spec", plan.code)
                            });
                        let qty = next_routable_sell_qty(
                            remaining,
                            sellable,
                            self.setup.config.lot_size,
                            stock.category.max_order_qty(false),
                        )?;
                        Some(AllocationRequest {
                            plan_id: plan.plan_id,
                            code: plan.code.clone(),
                            side: Side::Sell,
                            class: AllocationClass::ExistingPlan,
                            confidence_bp: plan.confidence_bp,
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
                allocate_soft_budgets(&funds, &requests, &AllocationPolicy::default())
                    .unwrap_or_else(|error| panic!("allocation failed for {id:?}: {error}")),
            )
        };
        // 7–8. 逐计划：紧迫度 → 受保护报价 → 真实路由执行。
        #[cfg(feature = "simulation-diagnostics")]
        if let Some(result) = &grants {
            self.causal_record(crate::diagnostics::causal::CausalFactKind::Budget {
                account: id,
                available_cents: result.available_cash.cents(),
                allocated_cents: result
                    .grants
                    .iter()
                    .map(|grant| grant.allocated_cash.cents())
                    .collect(),
            });
        }
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
        candidates.push_quote_plans(super::plan_chain_candidates::QuotePlans {
            account: id,
            market: market_view.clone(),
            plans: active_plans.into_iter().map(|plan| plan.plan_id).collect(),
            grants,
            sellable: sellable_by_code,
            one_minute_bp,
            thirty_minute_bp,
        });
    }

    /// 账户的机构风格（信念壳身份）。
    fn belief_style(&self, id: AccountId) -> Option<crate::strategy::InstitutionStyle> {
        self.accounts
            .get(&id)
            .and_then(|account| account.strategy.as_ref())
            .and_then(|strategy| strategy.institution_style())
    }

    /// 集合竞价委托快照（诊断/测试）：(side, price_cents, qty, owner) 列表。
    pub fn auction_orders_debug(&self, code: &StockCode) -> Vec<(Side, i64, u32, AccountId)> {
        self.auction_orders
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
        let technical = self.build_chain_technical_observations();
        let belief = self
            .belief_books
            .get(&account)
            .ok_or_else(|| format!("no belief book for {account:?}"))?;
        let weights = belief.analysis().weights();
        let Some(view) = market_view.stocks.get(code) else {
            return Ok(None);
        };
        let signals = self
            .build_candidate_signals(
                belief,
                code,
                view,
                price_paths.get(code),
                technical.get(code),
            )
            .map_err(|error| error.to_string())?;
        let assessment = blend_candidate(&weights, &signals);
        Ok(Some(format!(
            "fund={:?} trend={:?} pv={:?} tech={:?} weights={:?}x5 -> {assessment:?}",
            signals.fundamental.score().map(|s| s.value()),
            signals.trend.score().map(|s| s.value()),
            signals.price_volume.score().map(|s| s.value()),
            signals.technical.score().map(|s| s.value()),
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
        let belief = self.belief_books.get(&account)?;
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
        self.plans
            .plan_ids()
            .filter_map(|plan_id| {
                let plan = self.plans.plan(plan_id).ok()?;
                Some((
                    plan.account,
                    plan.code.0.clone(),
                    plan.direction,
                    match plan.target {
                        crate::plans::PlanTarget::ShareCount(qty) => qty,
                        crate::plans::PlanTarget::PositionFractionBp(_) => 0,
                    },
                    plan.filled_qty,
                    format!("{:?}", plan.status),
                ))
            })
            .collect()
    }

    /// 决策链诊断（只读；验收测试与离线对账用——不泄露任何隐藏市场信息，
    /// 共同 V 已不存在，这里只有链自身的状态计数与个人估值概览）。
    pub fn decision_chain_diagnostics(&self) -> DecisionChainDiagnostics {
        let mut per_stock: BTreeMap<StockCode, usize> = BTreeMap::new();
        let mut plan_count = 0_usize;
        for plan_id in self.plans.plan_ids() {
            if let Ok(plan) = self.plans.plan(plan_id) {
                plan_count += 1;
                *per_stock.entry(plan.code.clone()).or_default() += 1;
            }
        }
        let available_valuations: BTreeMap<StockCode, usize> = self
            .belief_books
            .values()
            .flat_map(|belief| belief.entry_stocks())
            .fold(BTreeMap::new(), |mut acc, code| {
                *acc.entry(code.clone()).or_default() += 1;
                acc
            });
        DecisionChainDiagnostics {
            plan_count,
            plans_per_stock: per_stock,
            belief_accounts: self.belief_books.len(),
            acquired_publications: self
                .information
                .values()
                .map(|state| state.acquired_count())
                .sum(),
            library_publications: self.library.report_count() + self.library.announcement_count(),
            available_valuation_entries: available_valuations,
        }
    }

    /// 个人每股估值区间（无条目/不可用 ⇒ None）。
    fn belief_per_share(&self, id: AccountId, code: &StockCode) -> Option<PerShareRange> {
        let entry = self.belief_books.get(&id)?.entry(code)?;
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
        session.accounts.get_mut(&owner).unwrap().strategy =
            Some(crate::account::StoredStrategy::production(Box::new(
                crate::strategy::BeliefInstitutionStrategy::new(0.05, 99)
                    .expect("strategy permits an explicit sub-board-lot size"),
            )));
        let owner_account = session
            .accounts
            .get_mut(&owner)
            .expect("fixture owner must exist");
        owner_account.cash = Money::from_cents(1_000_000);
        owner_account.positions.clear();
        let plan_id = session
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
                created_trading_day: u64::from(session.day),
            })
            .unwrap();
        let mut plans = std::mem::take(&mut session.plans);
        let view = session.build_market_view();
        let mut operations = PlanChainOperationBatch::empty();

        session.execute_plans_for_account(owner, &view, &mut plans, &mut operations);
        session.plans = plans;
        let mut events = Vec::new();
        session.consume_plan_chain_operation_batch(operations, &mut events);

        assert!(
            events.is_empty(),
            "a 99-share strategy must not emit an order event"
        );
        assert!(session.markets[&code].resting_orders_for(owner).is_empty());
        assert!(session
            .parent_orders
            .get(&owner)
            .and_then(|parents| parents.get(&code))
            .is_none());
        let plan = session.plans.plan(plan_id).unwrap();
        assert_eq!(plan.filled_qty, 0);
        assert!(plan.active_child_order_id.is_none());
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
                equity: cash,
            },
            &[request],
            &AllocationPolicy::default(),
        )
        .expect("the exact authoritative reservation is affordable");
        assert_eq!(
            allocation.grants[0].allocated_cash, authoritative_reservation,
            "the normal 10% cash reserve still leaves this child fully routable"
        );
        assert_eq!(allocation.grants[0].constraint, None);
        assert!(
            allocation.total_allocated <= allocation.available_cash,
            "soft budgets never exceed authoritative available cash"
        );
    }

    #[test]
    fn unaffordable_parent_target_routes_its_affordable_next_buy_child() {
        let mut session = probe_session();
        let owner = AccountId(1);
        let code = StockCode("000812".to_string());
        // Enter the call auction: an empty book is explicitly represented as a two-sided
        // protected quote, so the test exercises the actual submit route rather than a wait.
        session.setup.auction_ticks = 10;
        let owner_account = session
            .accounts
            .get_mut(&owner)
            .expect("fixture owner must exist");
        owner_account.cash = Money::from_cents(1_000_000);
        // `probe_session` assigns the institution a deterministic random float position.
        // This regression isolates cash budgeting, so that holding (and its T+1 state) must
        // not inflate equity and consume the complete 10% equity reserve.
        owner_account.positions.clear();
        assert!(owner_account.positions.is_empty());
        let parent_qty = 1_000_000;
        let stock = session
            .setup
            .stocks
            .iter()
            .find(|stock| stock.code == code)
            .expect("fixture stock must exist");
        let child_qty = next_routable_buy_qty(
            parent_qty,
            session.chain_strategy_params(owner).order_size,
            session.setup.config.lot_size,
            stock.category.max_order_qty(false),
        )
        .expect("fixture parent must have one routable child");
        let child_reservation = buy_order_reservation(
            &session.setup.config,
            session.markets[&code].up_stop().unwrap(),
            child_qty,
            Money::ZERO,
        )
        .unwrap();
        let parent_reservation = buy_order_reservation(
            &session.setup.config,
            session.markets[&code].up_stop().unwrap(),
            parent_qty,
            Money::ZERO,
        )
        .unwrap();
        let cash = session.accounts[&owner].cash;
        let equity = session.account_equity(owner).unwrap();
        assert_eq!(
            equity, cash,
            "cash-only fixture must have no hidden position equity"
        );
        let cash_reserve = Money::from_cents(cash.cents() / 10);
        let deployable_cash = cash.sub(cash_reserve).unwrap();
        assert!(parent_reservation > cash);
        assert!(
            child_reservation <= deployable_cash,
            "cash after the 10% reserve must cover the real child reservation"
        );
        session
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
                created_trading_day: u64::from(session.day),
            })
            .unwrap();
        let mut plans = std::mem::take(&mut session.plans);
        let view = session.build_market_view();
        let mut operations = PlanChainOperationBatch::empty();

        session.execute_plans_for_account(owner, &view, &mut plans, &mut operations);
        session.plans = plans;
        let mut events = Vec::new();
        session.consume_plan_chain_operation_batch(operations, &mut events);

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
                .parent_orders
                .get(&owner)
                .and_then(|parents| parents.get(&code))
                .and_then(|parent| parent.active_child_order_id)
                .is_some(),
            "the accepted child must be linked to its parent plan"
        );
        assert!(
            session.reserved_cash_for_account(owner).unwrap() <= session.accounts[&owner].cash,
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
            session.tick = tick;
            assert_eq!(session.chain_observation_instant().second_of_day(), seconds);
        }
        let mut setup = session.save().expect("healthy save").setup;
        setup.ticks_per_day = 240;
        setup.auction_ticks = 0;
        setup.closing_auction_ticks = 0;
        let mut session = GameSession::new(setup, 42).unwrap();
        for (tick, seconds) in [(119, 41_340), (120, 46_800), (121, 46_860), (240, 54_000)] {
            session.tick = tick;
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
        session.tick = 8_099;
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
        session.route_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockCode("000812".to_string()),
                side: Side::Buy,
                price: Money::from_cents(280),
                qty: 100,
            },
            &mut events,
        );
        let next_order_id = session.next_order_id;
        let resting_before = session
            .markets
            .values()
            .map(Market::resting_order_count)
            .sum::<usize>();

        let batch = session.run_decision_chain(&[AccountId(1)]);

        assert_eq!(batch.len(), 2);
        assert_eq!(session.next_order_id, next_order_id);
        assert_eq!(
            session
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
        let parent = session.parent_orders[&AccountId(1)][&StockCode("000812".to_string())].clone();
        session.pending_plan_events.push(PendingPlanEvent::Filled {
            plan_id: parent.linked_plan_id.unwrap(),
            order_id: parent.active_child_order_id.unwrap(),
            qty: 100,
            trading_day: u64::from(session.day),
        });
        let pending = serde_json::to_value(&session.pending_plan_events).unwrap();
        let parents = serde_json::to_value(&session.parent_orders).unwrap();
        let identities = (session.next_order_id, session.seq);

        let _batch = session.run_decision_chain(&[AccountId(1)]);

        assert_eq!(
            serde_json::to_value(&session.pending_plan_events).unwrap(),
            pending
        );
        assert_eq!(
            serde_json::to_value(&session.parent_orders).unwrap(),
            parents
        );
        assert_eq!((session.next_order_id, session.seq), identities);
    }

    use super::*;

    /// 公共场景：真实链路跑出一个带在途子单的 Buy 计划（DeepValue 机构在
    /// 低价股上信念看多 → 计划 + Patient 子单挂在玩家买一上方）。
    fn seeded_buy_plan_with_child() -> GameSession {
        let mut session = probe_session();
        force_attention(&mut session, AccountId(1));
        let mut seed_events = Vec::new();
        session.route_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockCode("000812".to_string()),
                side: Side::Buy,
                price: Money::from_cents(280),
                qty: 100,
            },
            &mut seed_events,
        );
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
                .parent_orders
                .get(&AccountId(1))
                .and_then(|parents| parents.get(&StockCode("000812".to_string())))
                .and_then(|parent| parent.active_child_order_id)
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

    fn drive_once(
        session: &mut GameSession,
        assessments: &BTreeMap<StockCode, CandidateAssessment>,
    ) -> Vec<Event> {
        let mut plans = std::mem::take(&mut session.plans);
        let view = session.build_market_view();
        let mut operations = PlanChainOperationBatch::empty();
        session.drive_plans_for_account(
            AccountId(1),
            assessments,
            &view,
            &mut plans,
            &mut operations,
        );
        session.plans = plans;
        let mut events = Vec::new();
        session.consume_plan_chain_operation_batch(operations, &mut events);
        events
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
            .parent_orders
            .get(&AccountId(1))
            .and_then(|parents| parents.get(&StockCode("000812".to_string())))
            .and_then(|parent| parent.active_child_order_id)
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
                .parent_orders
                .get(&AccountId(1))
                .and_then(|parents| parents.get(&StockCode("000812".to_string())))
                .map(|parent| parent.active_child_order_id.is_none())
                .unwrap_or(true),
            "the linked parent must no longer carry the canceled child"
        );
    }

    #[test]
    fn mid_day_termination_cancels_the_in_flight_child_first() {
        let mut session = seeded_buy_plan_with_child();
        let child_id = session
            .parent_orders
            .get(&AccountId(1))
            .and_then(|parents| parents.get(&StockCode("000812".to_string())))
            .and_then(|parent| parent.active_child_order_id)
            .expect("child must be in flight");
        // 手工推进计划进度（计划簿事件路径）：filled 500_000 / target 599_300；
        // 同向弱信号（score 1000）修订后的目标差量 ≈13.9 万股 < 已成交 50 万股
        // ⇒ below_filled_rationale ⇒ 终止（真实撤子单先行）。
        {
            let mut plans = std::mem::take(&mut session.plans);
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
                        trading_day: 0,
                    },
                )
                .unwrap();
            session.plans = plans;
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
        session
            .parent_orders
            .get_mut(&AccountId(1))
            .and_then(|parents| parents.get_mut(&code))
            .expect("seeded parent must exist")
            .active_child_order_id = None;
        session
            .parent_orders
            .get_mut(&AccountId(1))
            .and_then(|parents| parents.get_mut(&code))
            .expect("seeded parent must exist")
            .active_child_remaining_qty = None;
        {
            let mut plans = std::mem::take(&mut session.plans);
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
                        trading_day: 0,
                    },
                )
                .unwrap();
            session.plans = plans;
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
            .parent_orders
            .get(&AccountId(1))
            .and_then(|parents| parents.get(&StockCode("000812".to_string())))
            .and_then(|parent| parent.active_child_order_id)
            .expect("child must be in flight");
        // ticks_per_day=100、closing_auction_ticks=10 ⇒ tick ≥ 90 为收盘集合
        // 竞价（不可撤阶段）。
        session.tick = 95;

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
                .markets
                .get(&StockCode("000812".to_string()))
                .map(|market| market.resting_order_count() > 0)
                .unwrap_or(false),
            "the resting child must stay in the book"
        );
        assert_eq!(
            session
                .parent_orders
                .get(&AccountId(1))
                .and_then(|parents| parents.get(&StockCode("000812".to_string())))
                .and_then(|parent| parent.active_child_order_id),
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
                    tick_cents: 1,
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
        session.setup.auction_ticks = 10;
        session.accounts.get_mut(&owner).unwrap().positions.insert(
            code.clone(),
            crate::account::Position {
                qty: holding_qty,
                t1_locked: 0,
                invested_cents: i64::from(holding_qty) * 285,
                recovered_cents: 0,
            },
        );
        session
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
                created_trading_day: u64::from(session.day),
            })
            .unwrap();
        let mut plans = std::mem::take(&mut session.plans);
        let view = session.build_market_view();
        let mut operations = PlanChainOperationBatch::empty();

        session.execute_plans_for_account(owner, &view, &mut plans, &mut operations);
        session.plans = plans;
        let mut events = Vec::new();
        session.consume_plan_chain_operation_batch(operations, &mut events);
        events
    }

    fn force_attention(session: &mut GameSession, id: AccountId) {
        let attention = session.npc_attention.get_mut(&id).unwrap();
        attention.base_probability = 1.0;
        attention.next_attention_candidate_tick = 0;
        session.attention_queue.push(std::cmp::Reverse((0, id)));
    }

    #[test]
    fn two_capable_accounts_preserve_accepted_account_order() {
        let template = probe_session();
        let mut setup = template.setup.clone();
        setup.npcs.inst_count = 2;
        let mut session = GameSession::new(setup, 42).unwrap();
        for id in [AccountId(1), AccountId(2)] {
            let strategy = template.accounts[&AccountId(1)]
                .strategy
                .as_ref()
                .unwrap()
                .production_state()
                .unwrap()
                .into_strategy()
                .unwrap();
            session.accounts.get_mut(&id).unwrap().strategy =
                Some(crate::account::StoredStrategy::production(strategy));
            session
                .belief_books
                .insert(id, template.belief_books[&AccountId(1)].clone());
            session.accounts.get_mut(&id).unwrap().cash = template.accounts[&AccountId(1)].cash;
            session.accounts.get_mut(&id).unwrap().positions =
                template.accounts[&AccountId(1)].positions.clone();
            assert!(session.accounts[&id]
                .strategy
                .as_ref()
                .unwrap()
                .belief_chain_params()
                .is_some());
        }
        session.route_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockCode("000812".to_string()),
                side: Side::Buy,
                price: Money::from_cents(280),
                qty: 100,
            },
            &mut Vec::new(),
        );
        let mut events = Vec::new();

        let batch = session.run_decision_chain(&[AccountId(2), AccountId(1)]);
        session.consume_plan_chain_operation_batch(batch, &mut events);

        let owners: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::OrderAccepted { account, .. } => Some(*account),
                _ => None,
            })
            .collect();
        assert_eq!(owners, [AccountId(2), AccountId(1)]);
    }

    #[test]
    fn normal_step_routes_npc_then_player_then_plan_chain() {
        struct FixedQuote;
        impl crate::strategy::Strategy for FixedQuote {
            fn profile(&self) -> StrategyProfile {
                StrategyProfile::Institution(crate::strategy::InstitutionStyle::ActiveTrader)
            }
            fn strategy_family(&self) -> crate::strategy::StrategyFamily {
                crate::strategy::StrategyFamily::Momentum
            }
            fn decide(&mut self, _: &MarketView, _: &SelfView, _: &mut dyn Rng) -> Vec<Intent> {
                vec![Intent::PlaceLimit {
                    code: StockCode("000812".to_string()),
                    side: Side::Buy,
                    price: Money::from_cents(280),
                    qty: 100,
                }]
            }
        }
        let mut setup = probe_session().setup;
        setup.npcs.inst_count = 2;
        let mut session = GameSession::new(setup, 42).unwrap();
        session.accounts.get_mut(&AccountId(2)).unwrap().strategy = Some(
            crate::account::StoredStrategy::non_authoritative(Box::new(FixedQuote)),
        );
        force_attention(&mut session, AccountId(1));
        force_attention(&mut session, AccountId(2));
        session.pending_player.push((
            AccountId(0),
            Intent::PlaceLimit {
                code: StockCode("000812".to_string()),
                side: Side::Buy,
                price: Money::from_cents(280),
                qty: 100,
            },
        ));

        let events = session.step().unwrap();

        let owners: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                Event::OrderAccepted { account, .. } => Some(*account),
                _ => None,
            })
            .collect();
        assert_eq!(owners, [AccountId(2), AccountId(0), AccountId(1)]);
    }

    #[test]
    fn restructure_rejection_preserves_old_plan_parent_order_and_reservation() {
        let mut session = seeded_buy_plan_with_child();
        let code = StockCode("000812".to_string());
        let owner = AccountId(1);
        let old = session.plans.active_plan(owner, &code).unwrap().clone();
        let parents = serde_json::to_value(&session.parent_orders).unwrap();
        let reservation = session.reserved_cash_for_account(owner).unwrap();
        let order_count = session.markets[&code].resting_order_count();
        let mut batch = PlanChainOperationBatch::empty();
        batch.push_restructure(
            owner,
            old.plan_id,
            code.clone(),
            Some(OrderId(999_999)),
            false,
            PlanRevision {
                reason: RevisionReason::SignalShift,
                trading_day: u64::from(session.day),
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
        let mut events = Vec::new();

        session.consume_plan_chain_operation_batch(batch, &mut events);

        let current = session.plans.plan(old.plan_id).unwrap();
        assert_eq!(current.direction, old.direction);
        assert_eq!(current.target, old.target);
        assert_eq!(current.filled_qty, old.filled_qty);
        assert_eq!(current.active_child_order_id, old.active_child_order_id);
        assert_eq!(
            serde_json::to_value(&session.parent_orders).unwrap(),
            parents
        );
        assert_eq!(
            session.reserved_cash_for_account(owner).unwrap(),
            reservation
        );
        assert_eq!(session.markets[&code].resting_order_count(), order_count);
        assert!(matches!(
            events.as_slice(),
            [Event::IntentRejected {
                reason: RejectionReason::OrderNotFound,
                ..
            }]
        ));
    }
}

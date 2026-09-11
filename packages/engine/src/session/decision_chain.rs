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
use crate::strategy::{BeliefBook, BeliefCause, BeliefInputs, PerShareRange, ValuationOutcome};

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
    /// step 串行段的决策链入口：对本次 accepted 注意力中的信念机构账户执行
    /// 完整链条。事件（成交/接受/撤销/拒绝）按链内顺序追加进 step 事件流。
    pub(super) fn run_decision_chain(&mut self, npc_ids: &[AccountId], events: &mut Vec<Event>) {
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
            return;
        }
        let market_view = self.build_market_view();
        let price_paths = self
            .market_price_path_observations()
            .unwrap_or_else(|error| panic!("decision chain price paths failed: {error}"));
        let technical = self.build_chain_technical_observations();
        let now = self.chain_observation_instant();
        let exposed = self.chain_exposed_stocks(now);
        let mut plans = std::mem::take(&mut self.plans);
        for id in belief_ids {
            self.run_chain_for_account(
                id,
                &market_view,
                &price_paths,
                &technical,
                now,
                &exposed,
                &mut plans,
                events,
            );
        }
        self.plans = plans;
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

    /// 观察时点的权威 civil 瞬间：当前自然日 + 已完成交易分钟映射到 09:30
    /// 起的墙钟（获知守卫 observed_at ≥ published_at 的诚实输入）。
    fn chain_observation_instant(&self) -> crate::calendar::CivilInstant {
        let minute_of_day = self.current_market_minute()
            % u64::from(crate::observation::GAME_INTRADAY_MINUTES_PER_DAY);
        let wall_minutes = 9 * 60 + 30 + minute_of_day;
        crate::calendar::CivilInstant::from_hms(
            self.civil_clock.current_date(),
            u32::try_from(wall_minutes / 60).expect("market minute maps into wall hours"),
            u32::try_from(wall_minutes % 60).expect("market minute maps into wall minutes"),
            0,
        )
        .expect("market time maps into a valid civil instant")
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
    fn run_chain_for_account(
        &mut self,
        id: AccountId,
        market_view: &MarketView,
        price_paths: &BTreeMap<StockCode, crate::observation::PricePathObservation>,
        technical: &BTreeMap<StockCode, TechnicalObservation>,
        now: crate::calendar::CivilInstant,
        exposed: &BTreeSet<StockCode>,
        plans: &mut PlanBook,
        events: &mut Vec<Event>,
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

        // 2. 公共曝光 → 显式获知（新年报留下 id 供信念更新）。
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
        self.drive_plans_for_account(id, &assessments, market_view, plans);

        // 6–8. 预算/紧迫度/报价/执行（覆盖账户全部活跃计划，含既有）。
        self.execute_plans_for_account(id, market_view, plans, events);

        // 关注列表修剪：持仓 ∪ 活跃计划股票受保护（永不被驱逐）。
        let protected: BTreeSet<StockCode> = held
            .into_iter()
            .chain(self.active_plan_codes(id, plans))
            .collect();
        watchlist.prune(&protected);
        self.watchlists.insert(id, watchlist);
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
    fn drive_plans_for_account(
        &mut self,
        id: AccountId,
        assessments: &BTreeMap<StockCode, CandidateAssessment>,
        market_view: &MarketView,
        plans: &mut PlanBook,
    ) {
        let trading_day = u64::from(self.day);
        let lot = self.setup.config.lot_size;
        let account = self
            .accounts
            .get(&id)
            .unwrap_or_else(|| panic!("belief account {id:?} must exist"));
        let equity = self
            .account_equity(id)
            .unwrap_or_else(|error| panic!("equity for {id:?} failed: {error}"));
        if equity.cents() <= 0 {
            return;
        }
        let belief = self
            .belief_books
            .get(&id)
            .unwrap_or_else(|| panic!("belief account {id:?} must have a belief book"));
        let chain_params = self.chain_strategy_params(id);
        let max_fraction_bp = (chain_params.max_stock_fraction * 10_000.0).min(10_000.0) as u32;
        for (code, assessment) in assessments {
            let view = market_view
                .stocks
                .get(code)
                .unwrap_or_else(|| panic!("candidate {code:?} must have a market view"));
            let price = view.last_price;
            let held_qty = account
                .positions
                .get(code)
                .map_or(0, |position| position.qty);
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
    fn observe_plan(plans: &mut PlanBook, plan_id: PlanId, trading_day: u64) {
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
    fn execute_plans_for_account(
        &mut self,
        id: AccountId,
        market_view: &MarketView,
        plans: &mut PlanBook,
        events: &mut Vec<Event>,
    ) {
        let trading_day = u64::from(self.day);
        let lot = self.setup.config.lot_size;
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
                let view = market_view
                    .stocks
                    .get(&plan.code)
                    .unwrap_or_else(|| panic!("plan stock {:?} must have a view", plan.code));
                let price = view.last_price;
                match plan.direction {
                    Side::Buy => {
                        let requested_cash = price
                            .mul_shares(remaining)
                            .unwrap_or_else(|error| panic!("plan cash failed: {error}"));
                        let fee_reserve = buy_order_reservation(
                            &self.setup.config,
                            price,
                            remaining,
                            Money::ZERO,
                        )
                        .unwrap_or_else(|error| panic!("plan fee failed: {error}"));
                        Some(AllocationRequest {
                            plan_id: plan.plan_id,
                            code: plan.code.clone(),
                            side: Side::Buy,
                            class: AllocationClass::ExistingPlan,
                            confidence_bp: plan.confidence_bp,
                            requested_cash,
                            fee_reserve,
                            requested_sell_qty: 0,
                            sellable_qty: 0,
                            experience: AllocationExperience::default(),
                        })
                    }
                    Side::Sell => {
                        let sellable = sellable_by_code.get(&plan.code).copied().unwrap_or(0);
                        let qty = remaining.min(sellable);
                        let fee_reserve =
                            sell_order_fee_reservation(&self.setup.config, price, qty, Money::ZERO)
                                .unwrap_or_else(|error| panic!("plan fee failed: {error}"));
                        Some(AllocationRequest {
                            plan_id: plan.plan_id,
                            code: plan.code.clone(),
                            side: Side::Sell,
                            class: AllocationClass::ExistingPlan,
                            confidence_bp: plan.confidence_bp,
                            requested_cash: Money::ZERO,
                            fee_reserve,
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
        for plan in &active_plans {
            if !matches!(plan.status, PlanStatus::Active) {
                continue;
            }
            let Some(remaining) = plan.remaining_share_qty() else {
                continue;
            };
            let Some(allocation) = grants
                .as_ref()
                .and_then(|result| result.grants.iter().find(|g| g.plan_id == plan.plan_id))
                .cloned()
            else {
                continue;
            };
            let view = market_view
                .stocks
                .get(&plan.code)
                .unwrap_or_else(|| panic!("plan stock {:?} must have a view", plan.code));
            let price = view.last_price;
            // 集合竞价阶段连续簿已清空：报价参考回退到最新价（被删内核
            // `best_ask.unwrap_or(last_price)` 的同义实现——集合竞价限价以
            // 最新价/前收为参考是 A 股惯例；涨跌停带与路由守卫仍然全部生效）。
            let mut book_top = BookTop {
                best_bid: view.best_bid,
                best_ask: view.best_ask,
            };
            if book_top.best_bid.is_none()
                && book_top.best_ask.is_none()
                && matches!(
                    self.phase(),
                    TradingPhase::CallAuction | TradingPhase::ClosingAuction
                )
            {
                book_top.best_bid = Some(price);
                book_top.best_ask = Some(price);
            }
            let market = self
                .markets
                .get(&plan.code)
                .unwrap_or_else(|| panic!("plan stock {:?} must have a market", plan.code));
            let stock = self
                .setup
                .stocks
                .iter()
                .find(|stock| stock.code == plan.code)
                .unwrap_or_else(|| panic!("plan stock {:?} must have a spec", plan.code));
            let urgency_inputs = UrgencyInputs {
                side: plan.direction,
                return_30min_bp: thirty_minute_bp.get(&plan.code).copied().flatten(),
                return_1min_bp: one_minute_bp.get(&plan.code).copied().flatten(),
                risk_pressure_pause: false,
                adverse_selection_pause: false,
                risk_reduction_active: false,
                account_drawdown_bp: None,
                remaining_trading_days: u32::try_from(
                    plan.last_valid_trading_day().saturating_sub(trading_day),
                )
                .unwrap_or(u32::MAX),
                confidence_bp: plan.confidence_bp,
                style: match self.belief_style(id) {
                    Some(crate::strategy::InstitutionStyle::DeepValue) => PatienceStyle::DeepValue,
                    _ => PatienceStyle::Other,
                },
            };
            let urgency = assess_urgency(&urgency_inputs, &UrgencyPolicy::default())
                .unwrap_or_else(|error| panic!("urgency failed for {id:?}: {error}"));
            // 受保护限价：买 = 个人每股乐观估值；卖 = 悲观估值；无可用估值
            // 退到涨跌停带边界（绝不以整体权益量纲报价）。 Urgent 报价直接以
            // 该限价成交试探，必须再收到连续竞价价格笼子内（A 股 102%/98%）。
            let band_up = market
                .up_stop()
                .unwrap_or_else(|error| panic!("up stop failed for {:?}: {error}", plan.code));
            let band_down = market
                .down_stop()
                .unwrap_or_else(|error| panic!("down stop failed for {:?}: {error}", plan.code));
            let cage_bound = market.continuous_limit_bound(plan.direction).ok();
            let per_share = self.belief_per_share(id, &plan.code);
            let mut protection_limit = match plan.direction {
                Side::Buy => per_share.map_or(band_up, |range| range.optimistic.min(band_up)),
                Side::Sell => per_share.map_or(band_down, |range| range.pessimistic.max(band_down)),
            };
            if let Some(cage) = cage_bound {
                protection_limit = match plan.direction {
                    Side::Buy => protection_limit.min(cage),
                    Side::Sell => protection_limit.max(cage),
                };
            }
            let sellable = sellable_by_code.get(&plan.code).copied().unwrap_or(0);
            let max_order_qty = stock.category.max_order_qty(false);
            let desired_qty = match plan.direction {
                Side::Buy => {
                    let capped = remaining.min(chain_params.order_size.max(lot));
                    capped - capped % lot
                }
                // 卖出按可卖量与 A 股单笔申报上限孰小（零股规则由路由守卫校验）。
                Side::Sell => remaining.min(sellable).min(max_order_qty),
            };
            if desired_qty == 0 {
                continue;
            }
            // 预算约束：拨款不足以覆盖该子单（软预算的 CashReserve/
            // InsufficientAvailableCash 结果）时本轮不提交——计划保留，
            // 下一次观察随预算重估（K6 无资金语义由分配器承载）。
            let child_required = match plan.direction {
                Side::Buy => buy_order_reservation(
                    &self.setup.config,
                    protection_limit,
                    desired_qty,
                    Money::ZERO,
                ),
                Side::Sell => sell_order_fee_reservation(
                    &self.setup.config,
                    protection_limit,
                    desired_qty,
                    Money::ZERO,
                ),
            }
            .unwrap_or_else(|error| panic!("child reservation failed for {id:?}: {error}"));
            if child_required > allocation.allocated_cash {
                continue;
            }
            let active_child = self
                .parent_orders
                .get(&id)
                .and_then(|parents| parents.get(&plan.code))
                .filter(|parent| parent.linked_plan_id == Some(plan.plan_id))
                .and_then(|parent| {
                    parent.active_child_order_id.map(|order_id| ActiveQuote {
                        order_id,
                        price: parent.limit_price,
                        qty: parent
                            .active_child_remaining_qty
                            .unwrap_or(parent.child_qty),
                    })
                });
            let quote_inputs = QuoteDecisionInputs {
                side: plan.direction,
                urgency: urgency.urgency,
                pause: urgency.pause,
                book: book_top,
                protection_limit,
                band_down,
                band_up,
                tick: stock.tick,
                cage_bound,
                desired_qty,
                lot_size: lot,
                max_order_qty: stock.category.max_order_qty(false),
                available_sell_qty: sellable,
                active_order: active_child,
                cancellable_now: self.plan_child_is_cancellable_now(),
            };
            let decision = decide_quote(&quote_inputs)
                .unwrap_or_else(|error| panic!("quote decision failed for {id:?}: {error}"));
            let request = PlanExecutionRequest {
                plan_id: plan.plan_id,
                allocation,
                decision,
                trading_day,
            };
            let report = self
                .execute_plan_observation(plans, request)
                .unwrap_or_else(|error| {
                    panic!(
                        "plan execution failed for {id:?} {:?}: {error}",
                        plan.plan_id
                    )
                });
            events.extend(report.events);
        }
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

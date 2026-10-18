use super::*;
use crate::behavior::BehaviorMarketObservation;
use crate::observation::{
    build_account_risk_observation, build_equal_weight_market_observation, RiskPositionInput,
};
use crate::strategy::{PositionView, SelfView, Strategy, StrategyState};

/// D5 #4 retail 链：零售分析参数化上下文（观察/信号/成本核的共享输入
/// ——session 读视图与 runtime 组线程各自组装，同核消费保证双实现不
/// 漂移；view 依赖面的一次性提取）。
pub(crate) struct RetailAnalysisCtx<'a> {
    /// 分析账户。
    pub account: AccountId,
    /// 持仓键集（含零量条目——观察的 held/protected 基）。
    pub held: BTreeSet<StockId>,
    /// 现金（risk/own 组装）。
    pub cash: Money,
    /// 持仓映射（risk/own 组装）。
    pub positions: BTreeMap<StockId, crate::Position>,
    /// 当前市场分钟。
    pub minute: u64,
    /// 交易日（day 域）。
    pub day: u64,
    /// 自然日（经验时刻组装）。
    pub civil_date: crate::calendar::CivilDate,
    /// 会话种子（成本信号派生流输入）。
    pub seed: u64,
    /// tick。
    pub tick: u64,
    /// 公共信息库。
    pub library: &'a crate::information::PublicLibrary,
    /// 数值 id → 发行人（缺发行人的股 None）。
    pub issuer_of: BTreeMap<StockId, Option<crate::company::CompanyId>>,
    /// 公司域经营（spec 面）。
    pub operations: &'a crate::company::operations::CompanyOperations,
    /// 零售执行策略实例（data+style——成本信号与决策共用）。
    pub retail: RetailStrategyInstance,
}

/// D5 #4：零售执行策略的最小实例面（ZiNoise 生产态的组内可达投影）。
pub(crate) struct RetailStrategyInstance {
    /// 数据面（strategy_data 等价）。
    pub data: crate::strategy::StrategyData,
    /// 风格（决策内核输入）。
    pub style: crate::strategy::RetailStyle,
}

impl GameSession {
    pub(in crate::session) fn capture_retail_analysis(
        &mut self,
        observed: &[AccountId],
        market: &MarketView,
    ) -> Result<BTreeMap<AccountId, BTreeMap<StockId, CandidateAssessment>>, StepFatal> {
        let (view, mut writes) = crate::session::pipeline::npc_split::npc_split(self);
        Self::capture_retail_analysis_on(&view, &mut writes, observed, market)
    }

    /// 读视图+写组版本（queue 链转换目标形态）。
    pub(in crate::session) fn capture_retail_analysis_on(
        view: &crate::session::pipeline::npc_split::SessionReadView<'_>,
        writes: &mut crate::session::pipeline::npc_split::NpcWriteFields<'_>,
        observed: &[AccountId],
        market: &MarketView,
    ) -> Result<BTreeMap<AccountId, BTreeMap<StockId, CandidateAssessment>>, StepFatal> {
        let accounts = observed
            .iter()
            .copied()
            .filter(|id| {
                view.accounts
                    .get(id)
                    .is_some_and(|account| account.kind() == AccountKind::Retail)
            })
            .collect::<Vec<_>>();
        if accounts.is_empty() {
            return Ok(BTreeMap::new());
        }
        let observation = DecisionChainObservation::capture_for_roots_on(view, market.clone())?;
        let returns = observation
            .paths
            .iter()
            .map(|(stock, path)| (stock.clone(), path.thirty_minute.return_ratio))
            .collect();
        let thirty_minute_market =
            build_equal_weight_market_observation(&returns).map_err(|error| {
                StepFatal::InvariantViolation {
                    description: error.to_string(),
                    location: "decision_chain::retail_analysis_market".to_owned(),
                }
            })?;
        let work = accounts
            .into_iter()
            .map(|id| {
                // 批处理路径：行已在观察批安装，经写组读出（与任务路径同一
                // `observe_retail_analysis_on`）。
                let row = writes
                    .retail_experience
                    .get(&id)
                    .cloned()
                    .expect("observed retail must have an experience row");
                (id, PlanPersonalState::take_on(writes, id), row)
            })
            .collect::<Vec<_>>();
        let plans = &*writes.plans;
        let results = view.compute.parallel(|| {
            work.into_par_iter()
                .map(|(id, mut personal, row)| {
                    view.compute.record(crate::compute::ComputeWork::Analyse);
                    let assessments = Self::observe_retail_analysis_on(
                        view,
                        plans,
                        &row,
                        id,
                        &mut personal,
                        &observation,
                        &thirty_minute_market,
                    )?;
                    Ok((id, personal, assessments))
                })
                .collect::<Vec<Result<_, StepFatal>>>()
                .into_iter()
                .collect::<Result<Vec<_>, _>>()
        })?;
        let mut output = BTreeMap::new();
        for (id, personal, assessments) in results {
            personal.install_on(writes, id);
            output.insert(id, assessments);
        }
        Ok(output)
    }

    fn observe_retail_analysis(
        &self,
        id: AccountId,
        personal: &mut PlanPersonalState,
        observation: &DecisionChainObservation,
        thirty_minute_market: &crate::observation::EqualWeightMarketObservation,
    ) -> Result<BTreeMap<StockId, CandidateAssessment>, StepFatal> {
        let row = self
            .state
            .retail_experience
            .get(&id)
            .expect("observed retail must have an experience row");
        Self::observe_retail_analysis_on(
            &self.read_view(),
            &self.state.plans,
            row,
            id,
            personal,
            observation,
            thirty_minute_market,
        )
    }

    /// 读视图+写组再借用版本；T1·两阶段（ADR-0042 §T1）：经验行经参数传入
    /// （观察更新后的 owned 行），queue 账户任务与批处理捕获共用本实现。
    /// D5 #4：上下文自 view 组装后走 core（组线程同核）。
    pub(in crate::session) fn observe_retail_analysis_on(
        view: &crate::session::pipeline::npc_split::SessionReadView<'_>,
        plans: &crate::plans::PlanBook,
        experience: &crate::RetailExperienceState,
        id: AccountId,
        personal: &mut PlanPersonalState,
        observation: &DecisionChainObservation,
        thirty_minute_market: &crate::observation::EqualWeightMarketObservation,
    ) -> Result<BTreeMap<StockId, CandidateAssessment>, StepFatal> {
        let ctx = Self::retail_analysis_ctx_from_view(view, id);
        Self::observe_retail_analysis_core(
            &ctx,
            plans,
            experience,
            personal,
            observation,
            thirty_minute_market,
        )
    }

    /// D5 #4：读视图 → 零售分析上下文（view 依赖面的一次性提取）。
    pub(in crate::session) fn retail_analysis_ctx_from_view<'a>(
        view: &'a crate::session::pipeline::npc_split::SessionReadView<'_>,
        id: AccountId,
    ) -> RetailAnalysisCtx<'a> {
        let account = &view.accounts[&id];
        let positions: BTreeMap<StockId, crate::Position> = account.positions().clone();
        let held = positions.keys().cloned().collect();
        let StrategyState::ZiNoise(retail) = account
            .strategy()
            .expect("retail strategy exists")
            .production_state()
            .expect("retail strategy state decodes")
        else {
            panic!("retail {id:?} has no retail execution strategy");
        };
        let issuer_of = view
            .markets
            .keys()
            .map(|code| (*code, view.issuer_of_stock(*code)))
            .collect();
        RetailAnalysisCtx {
            account: id,
            held,
            cash: account.cash(),
            positions,
            minute: view.current_market_minute(),
            day: u64::from(view.day),
            civil_date: view.civil_date(),
            seed: view.seed,
            tick: view.tick,
            library: view.library,
            issuer_of,
            operations: view.operations,
            retail: RetailStrategyInstance {
                data: retail.strategy_data(),
                style: retail.retail_style().expect("retail style exists"),
            },
        }
    }

    /// D5 #4 零售分析核（观察主体——组线程/session 共用；输入经 ctx）。
    pub(crate) fn observe_retail_analysis_core(
        ctx: &RetailAnalysisCtx<'_>,
        plans: &crate::plans::PlanBook,
        experience: &crate::RetailExperienceState,
        personal: &mut PlanPersonalState,
        observation: &DecisionChainObservation,
        thirty_minute_market: &crate::observation::EqualWeightMarketObservation,
    ) -> Result<BTreeMap<StockId, CandidateAssessment>, StepFatal> {
        let invariant = |description: String| StepFatal::InvariantViolation {
            description,
            location: "decision_chain::retail_analysis".to_owned(),
        };
        let id = ctx.account;
        let held = ctx.held.clone();
        let mut protected = held.clone();
        protected.extend(plans.active_codes(id).cloned());
        let discovered = personal.attention.sample_discovery_stock(
            &observation.market,
            &held,
            &personal.watchlist,
            &observation.exposed,
        );
        let minute = ctx.minute;
        if let Some(code) = &discovered {
            personal
                .watchlist
                .record_attention(code, minute, minute)
                .map_err(|error| invariant(error.to_string()))?;
        }
        personal.watchlist.prune(&protected);
        let candidates = root_candidate_codes(id, &held, &personal.watchlist, &plans, discovered);
        let day = ctx.day;
        let mut reports: BTreeMap<StockId, PublicationId> = BTreeMap::new();
        let mut credit_defaults = Vec::new();
        let mut corrections = Vec::new();
        let checks_information = personal.attention.information_check_due(observation.now);
        for code in &candidates {
            let stock = observation.market.stocks.get(code).ok_or_else(|| {
                invariant(format!("retail {id:?} candidate {code:?} has no market"))
            })?;
            personal
                .price_memory
                .observe_price(code, stock.last_price, minute)
                .map_err(|error| invariant(error.to_string()))?;
            let Some(company) = ctx.issuer_of.get(code).cloned().flatten() else {
                continue;
            };
            if !checks_information {
                continue;
            }
            for report in discovery_candidates(ctx.library, &company, observation.now) {
                if personal
                    .information
                    .records_for_company(&company)
                    .iter()
                    .any(|record| record.id == report)
                {
                    continue;
                }
                personal
                    .information
                    .record_acquisition(id, ctx.library, report, observation.now)
                    .map_err(|error| invariant(error.to_string()))?;
                if let Ok(material) = ctx.library.report(report, observation.now) {
                    if matches!(
                        material.origin,
                        crate::information::PublicationOrigin::Correction
                    ) {
                        corrections.push((code.clone(), report));
                    }
                    let newer = match reports.get(code) {
                        Some(prior) => {
                            let prior = ctx
                                .library
                                .report(*prior, observation.now)
                                .map_err(|error| invariant(error.to_string()))?;
                            crate::strategy::own_known_report_priority(material)
                                > crate::strategy::own_known_report_priority(prior)
                        }
                        None => true,
                    };
                    if newer {
                        reports.insert(code.clone(), report);
                    }
                } else if let Some(cause) = crate::session::notices::credit_default_cause(
                    ctx.library,
                    report,
                    observation.now,
                )
                .map_err(|error| invariant(error.to_string()))?
                {
                    credit_defaults.push((code.clone(), cause));
                }
            }
        }
        let ctx_info = NpcObservationContext::new(
            id,
            &personal.information,
            ctx.library,
            &observation.market,
        )
        .map_err(|error| invariant(error.to_string()))?;
        for code in &candidates {
            let Some(company) = ctx.issuer_of.get(code).cloned().flatten() else {
                continue;
            };
            let spec = ctx
                .operations
                .company(&company)
                .ok_or_else(|| {
                    invariant(format!("retail issuer {company:?} has no operating books"))
                })?
                .spec();
            let inputs = BeliefInputs {
                ctx: &ctx_info,
                company: company.clone(),
                kind: spec.kind,
                total_issued_shares: spec.issued_shares,
                as_of_trading_day: day,
            };
            if reports.contains_key(code) {
                let report = crate::strategy::preferred_own_report(&ctx_info, &inputs.company)
                    .map_err(|error| invariant(format!("retail {id:?} {code:?}: {error}")))?
                    .ok_or_else(|| {
                        invariant(format!(
                            "retail {id:?} {code:?} acquired a report but has no own-known report"
                        ))
                    })?;
                personal
                    .belief
                    .apply_cause(
                        code,
                        crate::session::notices::material_cause(
                            ctx.library,
                            report,
                            observation.now,
                        )
                        .map_err(|error| invariant(error.to_string()))?,
                        &inputs,
                    )
                    .map_err(|error| invariant(format!("retail {id:?} {code:?}: {error}")))?;
            }
            for (_, report) in corrections.iter().filter(|(stock, _)| stock == code) {
                personal
                    .belief
                    .apply_cause(code, BeliefCause::Correction { report: *report }, &inputs)
                    .map_err(|error| invariant(error.to_string()))?;
            }
            for (_, cause) in credit_defaults.iter().filter(|(stock, _)| stock == code) {
                if personal.belief.entry(code).is_some() {
                    personal
                        .belief
                        .apply_cause(code, cause.clone(), &inputs)
                        .map_err(|error| invariant(error.to_string()))?;
                }
            }
            if personal.belief.entry(code).is_some_and(|entry| {
                day >= entry.anchor_trading_day + u64::from(entry.horizon_trading_days)
            }) {
                personal
                    .belief
                    .apply_cause(code, BeliefCause::HorizonExpired, &inputs)
                    .map_err(|error| invariant(error.to_string()))?;
            }
        }
        let account_experience = experience;
        if checks_information {
            personal
                .attention
                .record_information_check(observation.now)
                .map_err(|error| invariant(error.to_string()))?;
        }
        apply_personal_experience_feedback(&mut personal.belief, account_experience, day).map_err(
            |error| {
                invariant(format!(
                    "retail {id:?} experience confidence failed: {error}"
                ))
            },
        )?;
        let mut assessments = BTreeMap::new();
        for code in &candidates {
            let signals = Self::retail_candidate_signals_core(
                ctx,
                plans,
                account_experience,
                code,
                personal,
                observation,
                thirty_minute_market,
            )
            .map_err(|error| invariant(format!("retail {id:?} {code:?}: {error}")))?;
            personal
                .price_memory
                .record_public_history_read(code, minute, &mut personal.history_reads)
                .map_err(|error| invariant(error.to_string()))?;
            assessments.insert(
                code.clone(),
                blend_candidate(&personal.belief.analysis().weights(), &signals),
            );
        }
        personal.watchlist.prune(&protected);
        personal.price_memory.prune(&protected);
        Ok(assessments)
    }

    fn retail_candidate_signals(
        &self,
        id: AccountId,
        code: &StockId,
        personal: &PlanPersonalState,
        observation: &DecisionChainObservation,
        thirty_minute_market: &crate::observation::EqualWeightMarketObservation,
    ) -> Result<CandidateSignals, String> {
        let row = self
            .state
            .retail_experience
            .get(&id)
            .expect("retail must have an experience row");
        Self::retail_candidate_signals_on(
            &self.read_view(),
            &self.state.plans,
            row,
            id,
            code,
            personal,
            observation,
            thirty_minute_market,
        )
    }

    /// 读视图+写组再借用版本（D5 #4：经 ctx 走 core）。
    fn retail_candidate_signals_on(
        view: &crate::session::pipeline::npc_split::SessionReadView<'_>,
        plans: &crate::plans::PlanBook,
        experience: &crate::RetailExperienceState,
        id: AccountId,
        code: &StockId,
        personal: &PlanPersonalState,
        observation: &DecisionChainObservation,
        thirty_minute_market: &crate::observation::EqualWeightMarketObservation,
    ) -> Result<CandidateSignals, String> {
        let ctx = Self::retail_analysis_ctx_from_view(view, id);
        Self::retail_candidate_signals_core(
            &ctx,
            plans,
            experience,
            code,
            personal,
            observation,
            thirty_minute_market,
        )
    }

    /// D5 #4 候选信号核（组线程/session 共用）。
    fn retail_candidate_signals_core(
        ctx: &RetailAnalysisCtx<'_>,
        plans: &crate::plans::PlanBook,
        experience: &crate::RetailExperienceState,
        code: &StockId,
        personal: &PlanPersonalState,
        observation: &DecisionChainObservation,
        thirty_minute_market: &crate::observation::EqualWeightMarketObservation,
    ) -> Result<CandidateSignals, String> {
        use crate::plans::{
            fundamental_signal, normalized_score, price_volume_signal, technical_signal,
            trend_signal,
        };
        let stock = observation
            .market
            .stocks
            .get(code)
            .ok_or_else(|| format!("candidate {code:?} has no stock observation"))?;
        let path = observation
            .paths
            .get(code)
            .ok_or_else(|| format!("candidate {code:?} has no price path"))?;
        let fundamental = personal
            .belief
            .entry(code)
            .map(|entry| fundamental_signal(stock.last_price, &entry.valuation))
            .transpose()
            .map_err(|error| error.to_string())?
            .unwrap_or_else(|| {
                SignalContribution::unavailable(SignalUnavailableReason::FundamentalUnavailable)
            });
        let thirty = ratio_to_bp(path.thirty_minute.return_ratio);
        let trend = trend_signal(thirty, ratio_to_bp(path.five_day.return_ratio))
            .map_err(|error| error.to_string())?;
        let imbalance = normalized_score(
            i64::from(
                ratio_to_bp(Some(stock.order_book_imbalance))
                    .ok_or_else(|| "nonfinite retail imbalance".to_owned())?,
            ),
            10_000,
        )
        .map_err(|error| error.to_string())?;
        let price_volume = price_volume_signal(
            thirty,
            ratio_to_bp(Some(stock.relative_volume)),
            Some(imbalance),
        )
        .map_err(|error| error.to_string())?;
        let technical = match observation.technical.get(code) {
            Some(technical) => {
                technical_signal(&technical.sma20, &technical.sma60, &technical.rsi14)
                    .map_err(|error| error.to_string())?
            }
            None => SignalContribution::unavailable(SignalUnavailableReason::MissingObservation),
        };
        let experience = if personal.belief.analysis().weights().experience_cost_bp() == 0 {
            SignalContribution::unavailable(SignalUnavailableReason::ZeroWeight)
        } else {
            Self::retail_cost_signal_core(
                ctx,
                experience,
                code,
                observation,
                thirty_minute_market,
            )?
        };
        Ok(CandidateSignals {
            fundamental,
            trend,
            price_volume,
            technical,
            experience,
        })
    }

    fn retail_cost_signal(
        &self,
        id: AccountId,
        code: &StockId,
        observation: &DecisionChainObservation,
        thirty_minute_market: &crate::observation::EqualWeightMarketObservation,
    ) -> Result<SignalContribution, String> {
        let row = self
            .state
            .retail_experience
            .get(&id)
            .expect("retail must have an experience row");
        Self::retail_cost_signal_on(
            &self.read_view(),
            row,
            id,
            code,
            observation,
            thirty_minute_market,
        )
    }

    /// 读视图+写组再借用版本（D5 #4：经 ctx 走 core）。
    fn retail_cost_signal_on(
        view: &crate::session::pipeline::npc_split::SessionReadView<'_>,
        experience: &crate::RetailExperienceState,
        id: AccountId,
        code: &StockId,
        observation: &DecisionChainObservation,
        thirty_minute_market: &crate::observation::EqualWeightMarketObservation,
    ) -> Result<SignalContribution, String> {
        let ctx = Self::retail_analysis_ctx_from_view(view, id);
        Self::retail_cost_signal_core(&ctx, experience, code, observation, thirty_minute_market)
    }

    /// D5 #4 成本信号核（组线程/session 共用）。
    fn retail_cost_signal_core(
        ctx: &RetailAnalysisCtx<'_>,
        experience: &crate::RetailExperienceState,
        code: &StockId,
        observation: &DecisionChainObservation,
        thirty_minute_market: &crate::observation::EqualWeightMarketObservation,
    ) -> Result<SignalContribution, String> {
        let account_experience = experience;
        let inputs = ctx
            .positions
            .iter()
            .map(|(stock_code, position)| {
                let stock = observation
                    .market
                    .stocks
                    .get(stock_code)
                    .ok_or_else(|| format!("held {stock_code:?} has no market"))?;
                Ok((
                    stock_code.clone(),
                    RiskPositionInput {
                        qty: position.qty(),
                        cost_price: position.cost_price(),
                        last_price: stock.last_price,
                        peak_price_since_entry: account_experience
                            .stocks
                            .get(stock_code)
                            .and_then(|stock| stock.peak_price_since_entry),
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let mut risk = build_account_risk_observation(
            ctx.cash,
            &inputs,
            account_experience.reference_equity,
            account_experience.peak_equity,
        )
        .map_err(|error| error.to_string())?;
        risk.positions.retain(|stock, _| stock == code);
        let own = SelfView {
            cash: ctx.cash,
            positions: ctx
                .positions
                .get(code)
                .map(|position| {
                    BTreeMap::from([(
                        code.clone(),
                        PositionView {
                            qty: position.qty(),
                            sellable_qty: position.sellable(),
                            cost_price: position.cost_price(),
                        },
                    )])
                })
                .unwrap_or_default(),
        };
        let market = MarketView {
            stocks: BTreeMap::from([(code.clone(), observation.market.stocks[code].clone())]),
            tick: observation.market.tick,
            market_minute: observation.market.market_minute,
        };
        let paths = BTreeMap::from([(code.clone(), observation.paths[code].clone())]);
        let behavior = BehaviorMarketObservation {
            price_paths: paths,
            thirty_minute_market: thirty_minute_market.clone(),
        };
        let mut dated = account_experience.clone();
        dated.consecutive_failed_buys = dated
            .failure_influence(&crate::experience::ExperienceMoment {
                civil_date: ctx.civil_date,
                market_minute: ctx.minute,
                trading_day: ctx.day,
            })
            .map_err(|error| error.to_string())?;
        let mut rng = SplitMix64::new(derived_stream(
            ctx.seed,
            &format!("retail-cost-{}-{}", code.0, ctx.tick),
            ctx.account,
        ));
        let decision = crate::behavior::decide_retail_position_with_experience(
            &ctx.retail.data,
            ctx.retail.style,
            &market,
            &own,
            &behavior,
            &risk,
            &dated,
            ctx.minute,
            &mut rng,
        );
        let contribution = match decision.reason {
            crate::behavior::DecisionReason::PositionRisk
            | crate::behavior::DecisionReason::AccountDrawdown
            | crate::behavior::DecisionReason::TakeProfit
            | crate::behavior::DecisionReason::ProfitGiveback
            | crate::behavior::DecisionReason::BreakEvenRelief
            | crate::behavior::DecisionReason::LowConfidence
            | crate::behavior::DecisionReason::PostExitCooldown
            | crate::behavior::DecisionReason::T1Locked => {
                crate::plans::experience_cost_signal(decision.action)
            }
            _ if own
                .positions
                .get(code)
                .is_some_and(|position| position.cost_price.is_some()) =>
            {
                crate::plans::experience_cost_signal(crate::behavior::PositionAction::Hold)
            }
            _ => SignalContribution::unavailable(SignalUnavailableReason::MissingObservation),
        };
        Ok(contribution)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::strategy::{AnalysisProfile, AnalysisWeights, FundamentalMethod};

    fn session() -> GameSession {
        GameSession::new(
            crate::session::npc_working_quote_tests::retail_quote_setup(),
            41,
        )
        .unwrap()
    }

    #[test]
    fn retail_analysis_accepts_recovered_nonpositive_position_cost() {
        for (recovered_price, expected_cost) in [(1_500, 0), (2_000, -1_000)] {
            let mut session = session();
            let id = AccountId(1);
            let code = session
                .build_market_view()
                .stocks
                .keys()
                .next()
                .unwrap()
                .clone();
            let config = session.state.setup.config.clone();
            let account = session.fixture_account_mut(&id).unwrap();
            account
                .grant_position(code.clone(), 300, Money::from_cents(1_000))
                .unwrap();
            account
                .apply_sell(
                    &config,
                    code.clone(),
                    Money::from_cents(recovered_price),
                    200,
                )
                .unwrap();
            let cost = account.position(&code).unwrap().cost_price().unwrap();
            assert_eq!(cost.cents(), expected_cost);
            assert_eq!(account.position(&code).unwrap().qty(), 100);
            let market = session.build_market_view();
            let account = session.account(id).unwrap();
            let equity = account
                .cash()
                .add(market.stocks[&code].last_price.mul_shares(100).unwrap())
                .unwrap();
            session
                .state
                .retail_experience
                .get_mut(&id)
                .unwrap()
                .observe_equity(equity)
                .unwrap();
            let assessments = session
                .capture_retail_analysis(&[id], &market)
                .expect("回收投入后的非正成本不能中止本人分析");
            assert!(assessments[&id].contains_key(&code));
            assert_eq!(
                session
                    .account(id)
                    .unwrap()
                    .position(&code)
                    .unwrap()
                    .cost_price(),
                Some(cost)
            );
        }
    }

    #[test]
    fn retail_belief_state_is_created_without_institution_policy() {
        let session = session();
        let participant = session
            .state
            .belief_participants
            .get(&AccountId(1))
            .expect("retail must own a personal BeliefBook");
        assert!(matches!(
            participant.belief().profile(),
            StrategyProfile::Retail(_)
        ));
        assert!(participant.belief().institution_policy().is_none());
    }

    #[test]
    fn observed_retail_acquires_reports_and_persists_personal_valuation() {
        let mut session = session();
        let id = AccountId(1);
        let profile = session.state.accounts[&id].strategy().unwrap().profile();
        let analysis = AnalysisProfile::new(
            AnalysisWeights::new(10_000, 0, 0, 0, 0).unwrap(),
            Some(FundamentalMethod::EarningsMultiple),
        )
        .unwrap();
        let mut rng = SplitMix64::new(99);
        *session
            .state
            .belief_participants
            .get_mut(&id)
            .expect("retail belief exists")
            .belief_mut() = BeliefBook::new(id, profile, analysis, &mut rng);
        let market = session.build_market_view();
        let assessments = session.capture_retail_analysis(&[id], &market).unwrap();
        assert!(!assessments[&id].is_empty());
        let participant = &session.state.belief_participants[&id];
        assert!(participant
            .information()
            .companies()
            .any(|(_, records)| !records.is_empty()));
        assert!(participant.belief().entry_stocks().any(|code| participant
            .belief()
            .entry(code)
            .unwrap()
            .method
            == Some(FundamentalMethod::EarningsMultiple)));
        let saved = session.save().unwrap();
        let restored = GameSession::restore(&saved).unwrap();
        assert_eq!(
            restored.state.belief_participants[&id].belief(),
            participant.belief()
        );
    }

    #[test]
    fn unobserved_retail_does_not_acquire_public_information() {
        let mut session = session();
        let before =
            serde_json::to_value(session.state.belief_participants[&AccountId(1)].belief())
                .unwrap();
        let market = session.build_market_view();
        assert!(session
            .capture_retail_analysis(&[], &market)
            .unwrap()
            .is_empty());
        assert_eq!(
            serde_json::to_value(session.state.belief_participants[&AccountId(1)].belief())
                .unwrap(),
            before
        );
        assert!(session.state.belief_participants[&AccountId(1)]
            .information()
            .companies()
            .next()
            .is_none());
    }

    #[test]
    fn public_step_consumes_retail_beliefs_and_replays_after_restore() {
        let mut session = session();
        let id = AccountId(1);
        let profile = session.state.accounts[&id].strategy().unwrap().profile();
        let analysis = AnalysisProfile::new(
            AnalysisWeights::new(10_000, 0, 0, 0, 0).unwrap(),
            Some(FundamentalMethod::EarningsMultiple),
        )
        .unwrap();
        let mut rng = SplitMix64::new(99);
        *session
            .state
            .belief_participants
            .get_mut(&id)
            .unwrap()
            .belief_mut() = BeliefBook::new(id, profile, analysis, &mut rng);
        let attention = session.state.npc_attention.get_mut(&id).unwrap();
        attention.next_attention_candidate_tick = 0;
        attention.rng_state = 3;
        session.state.attention_scheduler.clear();
        session.state.attention_scheduler.enqueue(0, id);
        // D5 #4 fixture 适配（B 类：observe 门采样非断言面）：runtime 下
        /// 首拍快照来自构造期 desk（空簿起步，signal 面与旧管线预置簿
        /// 不同——observe 门采样可能翻转为不观察）。首拍构造常驻驱动器
        /// 后经 ForceDue（ensure_observe=true——组内绕门，session 侧
        /// 策略/关注概率不动，save 的 fault2210 校验面不受触碰）在下一
        /// 拍强制到期；断言面=entry 创建与 restore 双轨对拍。
        if crate::runtime::step_runtime_enabled() {
            session.step().expect("runtime 首拍构造驱动器");
            session.force_due_via_runtime(&[id], session.state.tick + 1);
        }
        session.step().unwrap();
        let book = session.state.belief_participants[&id].belief();
        assert!(book.entry_stocks().next().is_some());
        assert!(session
            .state
            .plans
            .active_plan_ids_for_account(id)
            .is_empty());
        let mut restored = GameSession::restore(&session.save().unwrap()).unwrap();
        let events = session.step().unwrap();
        let restored_events = restored.step().unwrap();
        assert_eq!(
            serde_json::to_value(events).unwrap(),
            serde_json::to_value(restored_events).unwrap()
        );
        assert_eq!(
            session.state.belief_participants[&id].belief(),
            restored.state.belief_participants[&id].belief()
        );
    }

    #[test]
    fn retail_zero_fundamental_weight_does_not_gain_a_valuation_method() {
        let mut session = session();
        let id = AccountId(1);
        let profile = session.state.accounts[&id].strategy().unwrap().profile();
        let analysis =
            AnalysisProfile::new(AnalysisWeights::new(0, 0, 10_000, 0, 0).unwrap(), None).unwrap();
        *session
            .state
            .belief_participants
            .get_mut(&id)
            .unwrap()
            .belief_mut() = BeliefBook::new(id, profile, analysis, &mut SplitMix64::new(99));
        let market = session.build_market_view();
        let assessments = session.capture_retail_analysis(&[id], &market).unwrap();
        assert!(!assessments[&id].is_empty());
        let book = session.state.belief_participants[&id].belief();
        assert!(book.entry_stocks().next().is_some());
        for code in book.entry_stocks() {
            let entry = book.entry(code).unwrap();
            assert_eq!(entry.method, None);
            assert!(matches!(
                entry.valuation,
                ValuationOutcome::Unavailable {
                    reason: crate::strategy::ValuationUnavailable::MethodDisabled
                }
            ));
        }
    }
}

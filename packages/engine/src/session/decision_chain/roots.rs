//! 封存的 root 输入与单账户个人状态观察；不持有路由或可写 GameSession。

use super::*;

/// 同批 root 共享的只读事实；不包含市场簿、历史、个人状态 map 或委托游标。
pub(in crate::session) struct RootReadContext {
    accounts: account_book::AccountBook,
    company_registry: std::sync::Arc<crate::company::CompanyRegistry>,
    operations: std::sync::Arc<crate::company::operations::CompanyOperations>,
    library: crate::information::PublicLibrary,
    pub(in crate::session) plans: PlanBook,
    civil_date: crate::CivilDate,
    market_minute: u64,
    day: u32,
}

impl RootReadContext {
    pub(in crate::session) fn capture(session: &GameSession) -> Result<Self, StepFatal> {
        let accounts = session.state.accounts.clone();
        Ok(Self {
            accounts,
            company_registry: session.state.company_registry.clone(),
            operations: session.state.operations.clone(),
            library: session.state.library.clone(),
            plans: session.state.plans.clone(),
            civil_date: session.civil_date(),
            market_minute: session.current_market_minute(),
            day: session.state.day,
        })
    }

    fn institution_behavior(
        &self,
        id: AccountId,
        belief: &BeliefBook,
        code: &StockCode,
        market: &MarketView,
        paused: Option<crate::plans::PauseReason>,
    ) -> crate::session::institutional_behavior::InstitutionBehavior {
        crate::session::institutional_behavior::assess_institution_behavior(
            belief
                .institution_policy()
                .expect("institution book has its frozen behavior policy"),
            belief.experience(),
            code,
            self.accounts[&id]
                .positions()
                .get(code)
                .and_then(crate::Position::cost_price),
            market.stocks[code].last_price,
            crate::experience::ExperienceMoment {
                civil_date: self.civil_date,
                market_minute: self.market_minute,
                trading_day: u64::from(self.day),
            },
            paused,
            if belief.institution_account_risk_paused() {
                Some(true)
            } else {
                belief.experience().peak_equity.map(|_| false)
            },
        )
        .unwrap_or_else(|error| panic!("institution {id:?} personal behavior failed: {error}"))
    }
    fn assess_candidates(
        &self,
        id: AccountId,
        belief: &BeliefBook,
        candidates: &BTreeSet<StockCode>,
        market_view: &MarketView,
        price_paths: &BTreeMap<StockCode, crate::observation::PricePathObservation>,
        technical: &BTreeMap<StockCode, TechnicalObservation>,
    ) -> BTreeMap<StockCode, CandidateAssessment> {
        let weights = belief.analysis().weights();
        let mut out = BTreeMap::new();
        for code in candidates {
            let Some(view) = market_view.stocks.get(code) else {
                continue;
            };
            let signals = self
                .build_candidate_signals(
                    id,
                    belief,
                    code,
                    market_view,
                    CandidateObservations {
                        stock: view,
                        price_path: price_paths.get(code),
                        technical: technical.get(code),
                    },
                )
                .unwrap_or_else(|error| {
                    panic!("candidate signals failed for {id:?} {code:?}: {error}")
                });
            let assessment = blend_candidate(&weights, &signals);
            out.insert(code.clone(), assessment);
        }
        out
    }
    pub(super) fn build_candidate_signals(
        &self,
        id: AccountId,
        belief: &BeliefBook,
        code: &StockCode,
        market_view: &MarketView,
        observations: CandidateObservations<'_>,
    ) -> Result<CandidateSignals, CandidateError> {
        use crate::plans::{
            fundamental_signal, normalized_score, price_volume_signal, technical_signal,
            trend_signal,
        };
        let CandidateObservations {
            stock: view,
            price_path: path,
            technical,
        } = observations;
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
        let experience = self
            .institution_behavior(id, belief, code, market_view, None)
            .cost_signal;
        Ok(CandidateSignals {
            fundamental,
            trend,
            price_volume,
            technical,
            experience,
        })
    }
}

/// 仅暂存一个账户的个人工作，输出 typed 操作增项；不预先决定生命周期动作。
pub(in crate::session) struct InstitutionDecisionRoot {
    account: AccountId,
    personal: PlanPersonalState,
}

impl InstitutionDecisionRoot {
    pub(in crate::session) fn new(account: AccountId, personal: PlanPersonalState) -> Self {
        Self { account, personal }
    }

    pub(in crate::session) fn observe(
        mut self,
        context: &RootReadContext,
        observation: &DecisionChainObservation,
    ) -> (
        AccountId,
        PlanPersonalState,
        PlanChainOperationBatch,
        PlanRootDiagnostics,
    ) {
        let mut operations = PlanChainOperationBatch::empty();
        let diagnostics = Self::observe_personal(
            context,
            self.account,
            &mut self.personal,
            &observation.market,
            &observation.paths,
            &observation.technical,
            observation.now,
            &observation.exposed,
            &context.plans,
            &mut operations,
        );
        (self.account, self.personal, operations, diagnostics)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn observe_personal(
        context: &RootReadContext,
        id: AccountId,
        personal: &mut PlanPersonalState,
        market_view: &MarketView,
        price_paths: &BTreeMap<StockCode, crate::observation::PricePathObservation>,
        technical: &BTreeMap<StockCode, TechnicalObservation>,
        now: crate::calendar::CivilInstant,
        exposed: &BTreeSet<StockCode>,
        plans: &PlanBook,
        operations: &mut PlanChainOperationBatch,
    ) -> PlanRootDiagnostics {
        let held: BTreeSet<StockCode> = context.accounts[&id].positions().keys().cloned().collect();
        let moment = crate::experience::ExperienceMoment {
            civil_date: context.civil_date,
            market_minute: context.market_minute,
            trading_day: u64::from(context.day),
        };
        // 股票簿可能已交给并行撮合线程；根观察只能读取它接受时固定的行情视图。
        let equity = context.accounts[&id]
            .positions()
            .iter()
            .try_fold(context.accounts[&id].cash(), |equity, (code, position)| {
                equity.add(
                    market_view.stocks[code]
                        .last_price
                        .mul_shares(position.qty())?,
                )
            })
            .unwrap_or_else(|error| panic!("institution own equity failed for {id:?}: {error}"));
        let institution_policy = *personal
            .belief
            .institution_policy()
            .expect("institution observation has a frozen policy");
        {
            let experience = personal.belief.experience_mut();
            if equity.cents() > 0 {
                experience.observe_equity(equity).unwrap_or_else(|error| {
                    panic!("institution equity memory failed for {id:?}: {error}")
                });
            }
            for code in &held {
                let price = market_view.stocks[code].last_price;
                if !experience.feedback.stocks.contains_key(code) {
                    // 开局分配的仓位只建立观察参照，不伪造买入订单或历史成交。
                    experience
                        .initialize_institutional_holding_dated(
                            code,
                            context.accounts[&id].positions()[code]
                                .cost_price()
                                .filter(|cost| cost.cents() > 0),
                            price,
                            moment,
                        )
                        .unwrap_or_else(|error| {
                            panic!("institution initial holding failed for {id:?}: {error}")
                        });
                } else {
                    crate::session::institutional_behavior::observe_institution_position(
                        experience,
                        code,
                        price,
                        moment,
                        &institution_policy,
                    )
                    .unwrap_or_else(|error| {
                        panic!("institution own position observation failed for {id:?}: {error}")
                    });
                }
            }
            experience.prune_watchlist(&held);
        }
        crate::session::institutional_behavior::observe_institution_account_risk(
            &mut personal.belief,
            equity,
            moment,
        )
        .unwrap_or_else(|error| {
            panic!("institution account risk observation failed for {id:?}: {error}")
        });

        // 1. 个体发现（消费注意力个体流；接受即写入关注列表）。
        let watchlist = &mut personal.watchlist;
        let discovered =
            personal
                .attention
                .sample_discovery_stock(market_view, &held, watchlist, exposed);
        let market_minute = context.market_minute;
        if let Some(code) = &discovered {
            watchlist
                .record_attention(code, market_minute, market_minute)
                .unwrap_or_else(|error| {
                    panic!("watchlist attention failed for {id:?} {code:?}: {error}")
                });
        }

        // 原持仓与信念条目消失后，活动计划仍须进入本人的候选集，保留复核与委托观察。
        let candidates = root_candidate_codes(id, &held, &personal.belief, plans, discovered);
        for code in &candidates {
            personal
                .price_memory
                .observe_price(
                    code,
                    market_view
                        .stocks
                        .get(code)
                        .unwrap_or_else(|| panic!("root candidate {code:?} has no market view"))
                        .last_price,
                    market_minute,
                )
                .unwrap_or_else(|error| {
                    panic!("price-memory observation failed for {id:?} {code:?}: {error}")
                });
        }

        // 2. 公共曝光 → 显式获知（新年报留下 id 供信念更新）。
        #[cfg(feature = "simulation-diagnostics")]
        let mut causal_facts =
            vec![crate::diagnostics::causal::CausalFactKind::Decision { account: id }];
        let mut new_annual_reports: Vec<(StockCode, PublicationId)> = Vec::new();
        for code in &candidates {
            let Some(company_id) = context.company_registry.issuer_of(code).cloned() else {
                continue;
            };
            for publication_id in discovery_candidates(&context.library, &company_id, now) {
                let already = personal
                    .information
                    .records_for_company(&company_id)
                    .iter()
                    .any(|record| record.id == publication_id);
                if already {
                    continue;
                }
                personal
                    .information
                    .record_acquisition(id, &context.library, publication_id, now)
                    .unwrap_or_else(|error| {
                        panic!(
                            "acquisition failed for account {id:?} publication {publication_id:?}: {error}"
                        )
                    });
                let is_annual = context
                    .library
                    .report(publication_id, now)
                    .map(|report| {
                        report.reports.kind == crate::accounting::reports::ReportKind::Annual
                    })
                    .unwrap_or(false);
                #[cfg(feature = "simulation-diagnostics")]
                {
                    let published = context
                        .library
                        .report(publication_id, now)
                        .map(|report| report.published_at)
                        .or_else(|_| {
                            context
                                .library
                                .announcement(publication_id, now)
                                .map(|announcement| announcement.published_at)
                        })
                        .expect("successful acquisition resolves a public publication");
                    causal_facts.push(crate::diagnostics::causal::CausalFactKind::Acquisition {
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
        //（information/library 只读借用与 belief 可变借用是不同字段）。
        let ctx =
            NpcObservationContext::new(id, &personal.information, &context.library, market_view)
                .unwrap_or_else(|error| panic!("observation context failed for {id:?}: {error}"));
        let as_of_trading_day = u64::from(context.day);
        let issuer_inputs: Vec<(StockCode, BeliefInputs<'_, MarketView>)> = candidates
            .iter()
            .filter_map(|code| {
                let company = context.company_registry.issuer_of(code).cloned()?;
                let spec = context
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
            let belief = &mut personal.belief;
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

        // 先形成或更新本人信念，再按本次真实失败订单调整信心；新阅读不会吞掉受挫事件。
        apply_institution_experience_feedback(&mut personal.belief, moment.trading_day);

        // 4–5. 混合分析与方向迟滞 聚合 + 计划生命周期。
        let assessments = context.assess_candidates(
            id,
            &personal.belief,
            &candidates,
            market_view,
            price_paths,
            technical,
        );
        operations.push_lifecycle(id, assessments, market_view.clone());

        // 6–8. 预算/紧迫度/报价/执行（覆盖账户全部活跃计划，含既有）。
        operations.push_account_execution(id, market_view.clone());

        // 关注列表修剪：持仓 ∪ 活跃计划股票受保护（永不被驱逐）。
        let protected: BTreeSet<StockCode> = held
            .into_iter()
            .chain(plans.active_codes(id).cloned())
            .collect();
        watchlist.prune(&protected);
        #[cfg(feature = "simulation-diagnostics")]
        return PlanRootDiagnostics {
            facts: causal_facts,
            reports: new_annual_reports,
            candidates,
        };
        #[cfg(not(feature = "simulation-diagnostics"))]
        PlanRootDiagnostics
    }
}

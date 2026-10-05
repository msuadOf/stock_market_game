use super::*;
use crate::behavior::BehaviorMarketObservation;
use crate::observation::{
    build_account_risk_observation, build_equal_weight_market_observation, RiskPositionInput,
};
use crate::strategy::{PositionView, SelfView, Strategy, StrategyState};

impl GameSession {
    pub(in crate::session) fn capture_retail_analysis(
        &mut self,
        observed: &[AccountId],
        market: &MarketView,
    ) -> Result<BTreeMap<AccountId, BTreeMap<StockCode, CandidateAssessment>>, StepFatal> {
        let accounts = observed
            .iter()
            .copied()
            .filter(|id| {
                self.state
                    .accounts
                    .get(id)
                    .is_some_and(|account| account.kind() == AccountKind::Retail)
            })
            .collect::<Vec<_>>();
        if accounts.is_empty() {
            return Ok(BTreeMap::new());
        }
        let observation = DecisionChainObservation::capture_for_roots(self, market.clone())?;
        let returns = observation
            .paths
            .iter()
            .filter(|(stock, _)| observation.market.stocks[*stock].is_trading)
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
            .map(|id| (id, PlanPersonalState::take(self, id)))
            .collect::<Vec<_>>();
        let results = work
            .into_par_iter()
            .map(|(id, mut personal)| {
                let assessments = self.observe_retail_analysis(
                    id,
                    &mut personal,
                    &observation,
                    &thirty_minute_market,
                )?;
                Ok((id, personal, assessments))
            })
            .collect::<Vec<Result<_, StepFatal>>>()
            .into_iter()
            .collect::<Result<Vec<_>, _>>()?;
        let mut output = BTreeMap::new();
        for (id, personal, assessments) in results {
            personal.install(self, id);
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
    ) -> Result<BTreeMap<StockCode, CandidateAssessment>, StepFatal> {
        let invariant = |description: String| StepFatal::InvariantViolation {
            description,
            location: "decision_chain::retail_analysis".to_owned(),
        };
        let account = &self.state.accounts[&id];
        let held = account.positions().keys().cloned().collect::<BTreeSet<_>>();
        let mut protected = held.clone();
        protected.extend(self.state.plans.active_codes(id).cloned());
        let discovered = personal.attention.sample_discovery_stock(
            &observation.market,
            &held,
            &personal.watchlist,
            &observation.exposed,
        );
        let minute = self.current_market_minute();
        if let Some(code) = &discovered {
            personal
                .watchlist
                .record_attention(code, minute, minute)
                .map_err(|error| invariant(error.to_string()))?;
        }
        personal.watchlist.prune(&protected);
        let candidates = root_candidate_codes(
            id,
            &held,
            &personal.watchlist,
            &self.state.plans,
            discovered,
        );
        let mut reports: BTreeMap<StockCode, PublicationId> = BTreeMap::new();
        let mut credit_defaults = Vec::new();
        let mut corrections = Vec::new();
        let checks_information = personal.attention.information_check_due(observation.now);
        for code in &candidates {
            let stock = observation.market.stocks.get(code).ok_or_else(|| {
                invariant(format!("retail {id:?} candidate {code:?} has no market"))
            })?;
            if stock.is_trading {
                personal
                    .price_memory
                    .observe_price(
                        code,
                        stock.last_price,
                        self.stock_market_minute(code)
                            .map_err(|error| invariant(error.to_string()))?,
                        minute,
                    )
                    .map_err(|error| invariant(error.to_string()))?;
            }
            let Some(company) = self.state.company_system.issuers().issuer_of(code) else {
                continue;
            };
            if !checks_information {
                continue;
            }
            for report in discovery_candidates(&self.state.library, company, observation.now) {
                if personal
                    .information
                    .records_for_company(company)
                    .iter()
                    .any(|record| record.id == report)
                {
                    continue;
                }
                personal
                    .information
                    .record_acquisition(id, &self.state.library, report, observation.now)
                    .map_err(|error| invariant(error.to_string()))?;
                if let Ok(material) = self.state.library.report(report, observation.now) {
                    if matches!(
                        material.origin,
                        crate::information::PublicationOrigin::Correction
                    ) {
                        corrections.push((code.clone(), report));
                    }
                    let newer = match reports.get(code) {
                        Some(prior) => {
                            let prior = self
                                .state
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
                    &self.state.library,
                    report,
                    observation.now,
                )
                .map_err(|error| invariant(error.to_string()))?
                {
                    credit_defaults.push((code.clone(), cause));
                }
            }
        }
        let ctx = NpcObservationContext::new(
            id,
            &personal.information,
            &self.state.library,
            &observation.market,
        )
        .map_err(|error| invariant(error.to_string()))?;
        for code in &candidates {
            let Some(company) = self.state.company_system.issuers().issuer_of(code) else {
                continue;
            };
            let stock_day = u64::from(
                self.stock_trading_day(code)
                    .map_err(|error| invariant(error.to_string()))?,
            );
            let spec = self.state.company_system.issuers().get(company)
                .ok_or_else(|| {
                    invariant(format!("retail issuer {company:?} has no registered identity"))
                })?;
            let inputs = BeliefInputs {
                ctx: &ctx,
                company: company.clone(),
                kind: spec.kind,
                total_issued_shares: spec.issued_shares,
                as_of_trading_day: stock_day,
            };
            if reports.contains_key(code) {
                let report = crate::strategy::preferred_own_report(&ctx, &inputs.company)
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
                            &self.state.library,
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
                stock_day >= entry.anchor_trading_day + u64::from(entry.horizon_trading_days)
            }) {
                personal
                    .belief
                    .apply_cause(code, BeliefCause::HorizonExpired, &inputs)
                    .map_err(|error| invariant(error.to_string()))?;
            }
        }
        let experience = self
            .state
            .retail_experience
            .get(&id)
            .ok_or_else(|| invariant(format!("retail {id:?} has no personal experience")))?;
        if checks_information {
            personal
                .attention
                .record_information_check(observation.now)
                .map_err(|error| invariant(error.to_string()))?;
        }
        let stock_days = self
            .state
            .markets
            .keys()
            .map(|code| {
                self.stock_trading_day(code)
                    .map(|day| (code.clone(), u64::from(day)))
                    .map_err(|error| invariant(error.to_string()))
            })
            .collect::<Result<BTreeMap<_, _>, _>>()?;
        apply_personal_experience_feedback(&mut personal.belief, experience, |code| {
            stock_days[code]
        })
        .map_err(|error| {
            invariant(format!(
                "retail {id:?} experience confidence failed: {error}"
            ))
        })?;
        let mut assessments = BTreeMap::new();
        for code in &candidates {
            if !observation.market.stocks[code].is_trading {
                continue;
            }
            let signals = self
                .retail_candidate_signals(id, code, personal, observation, thirty_minute_market)
                .map_err(|error| invariant(format!("retail {id:?} {code:?}: {error}")))?;
            personal
                .price_memory
                .record_public_history_read(
                    code,
                    self.stock_market_minute(code)
                        .map_err(|error| invariant(error.to_string()))?,
                    minute,
                    &mut personal.history_reads,
                )
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
        code: &StockCode,
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
            self.retail_cost_signal(id, code, observation, thirty_minute_market)?
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
        code: &StockCode,
        observation: &DecisionChainObservation,
        thirty_minute_market: &crate::observation::EqualWeightMarketObservation,
    ) -> Result<SignalContribution, String> {
        let account = &self.state.accounts[&id];
        let StrategyState::ZiNoise(retail) = account
            .strategy()
            .expect("retail strategy exists")
            .production_state()
            .map_err(|error| error.to_string())?
        else {
            return Err(format!("retail {id:?} has no retail execution strategy"));
        };
        let experience = self
            .state
            .retail_experience
            .get(&id)
            .ok_or_else(|| format!("retail {id:?} has no personal experience"))?;
        let inputs = account
            .positions()
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
                        peak_price_since_entry: experience
                            .stocks
                            .get(stock_code)
                            .and_then(|stock| stock.peak_price_since_entry),
                    },
                ))
            })
            .collect::<Result<BTreeMap<_, _>, String>>()?;
        let mut risk = build_account_risk_observation(
            account.cash(),
            &inputs,
            experience.reference_equity,
            experience.peak_equity,
        )
        .map_err(|error| error.to_string())?;
        risk.positions.retain(|stock, _| stock == code);
        let own = SelfView {
            cash: account.cash(),
            positions: account
                .positions()
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
        let mut dated = experience.clone();
        dated.consecutive_failed_buys = dated
            .failure_influence(&crate::experience::ExperienceMoment {
                civil_date: self.civil_date(),
                market_minute: self.current_market_minute(),
                trading_day: u64::from(self.state.day),
            })
            .map_err(|error| error.to_string())?;
        let mut rng = SplitMix64::new(derived_stream(
            self.state.seed,
            &format!("retail-cost-{}-{}", code.0, self.state.tick),
            id,
        ));
        let decision = crate::behavior::decide_retail_position_with_experience(
            &retail.strategy_data(),
            retail.retail_style().expect("retail style exists"),
            &market,
            &own,
            &behavior,
            &risk,
            &dated,
            self.current_market_minute(),
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
            let account = session.state.accounts.get_mut(&id).unwrap();
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

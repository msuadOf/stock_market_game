use super::*;
use rayon::prelude::*;

pub(in crate::session) struct NpcDecisionBatch {
    pub(in crate::session) accepted_due_npc_ids: Vec<AccountId>,
    pub(in crate::session) intents: Vec<(AccountId, Intent)>,
}

struct StrategyEvaluationResult {
    account: AccountId,
    updates_working_quotes: bool,
    uses_parent_order_execution: bool,
    position_decision: Option<PositionDecision>,
    reviewed_stocks: BTreeSet<StockCode>,
    intents: Vec<Intent>,
}

impl GameSession {
    pub(in crate::session) fn generate_npc_decision_batch(
        &mut self,
        events: &mut Vec<Event>,
    ) -> NpcDecisionBatch {
        let phase = self.phase();
        let tick = self.tick;
        let attention_candidates = self.pop_due_npc_ids(tick);
        let market_view = self.build_market_view();
        let (working_continuous, working_auction) = self.working_orders_by_account();
        let mut accepted_due_npc_ids = Vec::with_capacity(attention_candidates.len());
        for id in attention_candidates {
            if self.evaluate_attention_candidate(id, &market_view) {
                accepted_due_npc_ids.push(id);
            }
        }
        self.observe_retail_experience(&accepted_due_npc_ids)
            .unwrap_or_else(|error| panic!("retail experience observation failed: {error}"));
        let market_minute = self.current_market_minute();
        let self_views = self.build_self_views_for(
            &accepted_due_npc_ids,
            phase,
            &working_continuous,
            &working_auction,
        );
        let has_retail_observer = accepted_due_npc_ids.iter().any(|id| {
            self.accounts
                .get(id)
                .is_some_and(|account| account.kind == AccountKind::Retail)
        });
        let behavior_market = has_retail_observer.then(|| self.behavior_market_observation());
        let account_risks = if has_retail_observer {
            self.account_risk_observations_for(&accepted_due_npc_ids)
        } else {
            BTreeMap::new()
        };

        let mut strategies = Vec::new();
        for &id in &accepted_due_npc_ids {
            if let Some(account) = self.accounts.get_mut(&id) {
                if let Some(strategy) = account.strategy.take() {
                    strategies.push((id, strategy));
                }
            }
        }

        let seed = self.seed;
        let results: Vec<StrategyEvaluationResult> = strategies
            .par_iter_mut()
            .map(|(id, strategy)| {
                let self_view = self_views
                    .get(id)
                    .expect("every strategy account must have a self view");
                let npc_seed = seed
                    ^ tick.wrapping_mul(0x9E3779B97F4A7C15)
                    ^ id.0.wrapping_mul(0x6A09E667F3BCC908);
                let mut npc_rng = SplitMix64::new(npc_seed);
                let behavior = self
                    .accounts
                    .get(id)
                    .is_some_and(|account| account.kind == AccountKind::Retail)
                    .then(|| {
                        (
                            behavior_market
                                .as_ref()
                                .expect("retail observer requires shared behavior market"),
                            account_risks
                                .get(id)
                                .expect("retail observer requires account-risk observation"),
                        )
                    });
                let decision = strategy.decide_with_experience(
                    &market_view,
                    self_view,
                    behavior.map(|(market, _)| market),
                    behavior.map(|(_, risk)| risk),
                    self.retail_experience.get(id),
                    market_minute,
                    &mut npc_rng,
                );
                StrategyEvaluationResult {
                    account: *id,
                    updates_working_quotes: !decision.intents.is_empty()
                        || !decision.reviewed_stocks.is_empty()
                        || strategy.updates_working_quotes_on_empty_decision(),
                    uses_parent_order_execution: strategy.uses_parent_order_execution(),
                    position_decision: decision.position_decision,
                    reviewed_stocks: decision.reviewed_stocks,
                    intents: decision.intents,
                }
            })
            .collect();

        for (id, strategy) in strategies {
            if let Some(account) = self.accounts.get_mut(&id) {
                account.strategy = Some(strategy);
            }
        }

        let mut sorted = results;
        sorted.sort_by_key(|result| result.account);
        let mut pending = Vec::new();
        for result in sorted {
            let StrategyEvaluationResult {
                account,
                updates_working_quotes,
                uses_parent_order_execution,
                position_decision,
                reviewed_stocks,
                intents,
            } = result;
            if self.accounts[&account].kind == AccountKind::Retail {
                if let Some(decision) = position_decision {
                    self.last_retail_decisions
                        .push(RetailDecisionTrace { account, decision });
                }
            }
            if self.accounts[&account].kind == AccountKind::Retail {
                let held = self.accounts[&account].positions.keys().cloned().collect();
                let experience = self.retail_experience.get_mut(&account).unwrap_or_else(|| {
                    panic!("retail account {} is missing experience state", account.0)
                });
                for code in &reviewed_stocks {
                    experience.observe_stock(code, market_minute);
                }
                experience.prune_watchlist(&held);
            }
            let working = WorkingOrderSlices {
                continuous: working_continuous
                    .get(&account)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
                auction: working_auction
                    .get(&account)
                    .map(Vec::as_slice)
                    .unwrap_or(&[]),
            };
            let intents = if updates_working_quotes {
                let intents = if uses_parent_order_execution
                    && self.accounts[&account].kind == AccountKind::Inst
                {
                    self.materialize_parent_order_intents(account, intents, market_minute, working)
                } else {
                    intents
                };
                let scope = if !reviewed_stocks.is_empty() {
                    ReconcileScope::ReviewedStocks(reviewed_stocks)
                } else if self.accounts[&account].kind == AccountKind::Retail {
                    ReconcileScope::DesiredStockSides
                } else {
                    ReconcileScope::AllWorkingOrders
                };
                self.reconcile_npc_working_orders_from_index(
                    account, intents, phase, scope, working, events,
                )
            } else {
                intents
            };
            pending.extend(intents.into_iter().map(|intent| (account, intent)));
        }
        NpcDecisionBatch {
            accepted_due_npc_ids,
            intents: self.cap_npc_intents_to_available_cash(pending),
        }
    }
}

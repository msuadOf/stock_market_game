//! Pure NPC P2 source runner. Implementation owned by the L1 batch.
use super::{DecisionSnapshot, DecisionSnapshotError, IntentCandidateKey};
use crate::account::StoredStrategy;
use crate::strategy::{Intent, StrategyDecision, StrategyState, StrategyStateError};
use crate::{AccountId, SplitMix64};
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::sync::Arc;

/// An NPC raw intent with the identity that P2 composition must preserve.
#[derive(Clone, Debug, serde::Serialize)]
pub(in crate::session) struct NpcDecisionIntent {
    key: IntentCandidateKey,
    intent: Intent,
}

impl NpcDecisionIntent {
    pub(in crate::session) const fn key(&self) -> &IntentCandidateKey {
        &self.key
    }

    pub(in crate::session) const fn intent(&self) -> &Intent {
        &self.intent
    }
}

/// The prospective, non-routing decision result for one NPC account.
#[derive(Clone, Debug)]
pub(in crate::session) struct NpcDecisionAccountOutput {
    account: AccountId,
    #[cfg(test)]
    strategy_state: Option<StrategyState>,
    strategy: Option<NpcStrategyUpdate>,
    reviewed_stocks: BTreeSet<crate::StockCode>,
    position_decision: Option<crate::behavior::PositionDecision>,
    execution_urgency: crate::plans::urgency::risk::RiskUrgencyAssessment,
    updates_working_quotes: bool,
    uses_parent_order_execution: bool,
}

#[derive(Clone, Debug)]
pub(in crate::session) enum NpcStrategyUpdate {
    Unchanged,
    Replace(StoredStrategy),
}

impl NpcDecisionAccountOutput {
    pub(in crate::session) const fn account(&self) -> AccountId {
        self.account
    }

    #[cfg(test)]
    pub(in crate::session) const fn strategy_state(&self) -> Option<&StrategyState> {
        self.strategy_state.as_ref()
    }

    pub(in crate::session) fn take_strategy(&mut self) -> Option<NpcStrategyUpdate> {
        #[cfg(test)]
        self.strategy_state.take()?;
        self.strategy.take()
    }

    pub(in crate::session) const fn reviewed_stocks(&self) -> &BTreeSet<crate::StockCode> {
        &self.reviewed_stocks
    }

    pub(in crate::session) const fn position_decision(
        &self,
    ) -> Option<&crate::behavior::PositionDecision> {
        self.position_decision.as_ref()
    }

    pub(in crate::session) fn execution_urgency(
        &self,
    ) -> &crate::plans::urgency::risk::RiskUrgencyAssessment {
        &self.execution_urgency
    }

    pub(in crate::session) const fn updates_working_quotes(&self) -> bool {
        self.updates_working_quotes
    }

    pub(in crate::session) const fn uses_parent_order_execution(&self) -> bool {
        self.uses_parent_order_execution
    }
}

/// Pure P2 result: prospective strategy states and raw intents only.
///
/// It deliberately contains no event, router, order-book, envelope, or session handle.
#[derive(Clone, Debug)]
pub(in crate::session) struct NpcDecisionSourceOutput {
    account_ids: Vec<AccountId>,
    accounts: Vec<NpcDecisionAccountOutput>,
    intents: Vec<NpcDecisionIntent>,
}

impl NpcDecisionSourceOutput {
    pub(in crate::session) fn accounts(&self) -> &[AccountId] {
        &self.account_ids
    }

    pub(in crate::session) fn intents(&self) -> &[NpcDecisionIntent] {
        &self.intents
    }

    pub(in crate::session) fn account_outputs(&self) -> &[NpcDecisionAccountOutput] {
        &self.accounts
    }

    pub(in crate::session) fn projection_parts(
        &mut self,
    ) -> (&mut [NpcDecisionAccountOutput], &[NpcDecisionIntent]) {
        (&mut self.accounts, &self.intents)
    }

    #[cfg(test)]
    pub(in crate::session) fn strategy_state(
        &self,
        account: AccountId,
    ) -> Result<&StrategyState, NpcDecisionSourceError> {
        self.accounts
            .iter()
            .find(|output| output.account == account)
            .and_then(NpcDecisionAccountOutput::strategy_state)
            .ok_or(NpcDecisionSourceError::MissingOutputState(account))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(in crate::session) enum NpcDecisionSourceError {
    #[error("P2 NPC risk urgency failed for account {account:?}: {source}")]
    RiskUrgency {
        account: AccountId,
        source: crate::plans::UrgencyError,
    },
    #[error("P2 NPC source snapshot error: {0}")]
    Snapshot(DecisionSnapshotError),
    #[error("P2 NPC source cannot hydrate strategy for account {account:?}: {source}")]
    StrategyHydration {
        account: AccountId,
        source: StrategyStateError,
    },
    #[error("P2 NPC source intent ordinal overflow for account {account:?}")]
    IntentOrdinalOverflow { account: AccountId },
    #[error("P2 NPC source has no output strategy state for account {0:?}")]
    #[cfg(test)]
    MissingOutputState(AccountId),
}

pub(in crate::session) fn npc_rng_seed(base: u64, tick: u64, account: AccountId) -> u64 {
    base ^ tick.wrapping_mul(0x9E3779B97F4A7C15) ^ account.0.wrapping_mul(0x6A09E667F3BCC908)
}

/// Evaluates sealed NPC strategies without mutating authority.
///
/// Hydration and decision share one per-account parallel task. Results are read
/// in canonical account order, so an invalid state has a stable first error and
/// no partial output escapes. Per-account RNG is independent of task scheduling.
pub(in crate::session) fn run_npc_decisions(
    snapshot: Arc<DecisionSnapshot>,
    config: &crate::GameConfig,
) -> Result<NpcDecisionSourceOutput, NpcDecisionSourceError> {
    let results = snapshot
        .due_npc_ids()
        .par_iter()
        .copied()
        .map(|account| {
            let input = snapshot
                .account(account)
                .map_err(NpcDecisionSourceError::Snapshot)?;
            let mut strategy = input
                .strategy_state()
                .clone()
                .into_strategy()
                .map_err(|source| NpcDecisionSourceError::StrategyHydration { account, source })?;
            let npc_seed = npc_rng_seed(snapshot.npc_seed_base(), snapshot.tick(), account);
            let mut rng = SplitMix64::new(npc_seed);
            let decision = strategy.decide_with_experience(
                snapshot.market(),
                input.self_view(),
                (input.kind() == crate::AccountKind::Retail)
                    .then(|| snapshot.behavior_market())
                    .flatten(),
                (input.kind() == crate::AccountKind::Retail)
                    .then(|| input.account_risk())
                    .flatten(),
                input.retail_experience(),
                snapshot.market_minute(),
                &mut rng,
                config,
            );
            let execution_urgency = crate::plans::urgency::risk::assess_personal_risk_urgency(
                decision.position_decision.as_ref(),
                input.account_risk(),
                snapshot.urgency_policy(),
            )
            .map_err(|source| NpcDecisionSourceError::RiskUrgency { account, source })?;
            let strategy_state = StrategyState::from_strategy(strategy.as_ref())
                .map_err(|source| NpcDecisionSourceError::StrategyHydration { account, source })?;
            // StrategyState is the complete authoritative decision state. A
            // decision that leaves it unchanged need not detach the account page.
            let unchanged = &strategy_state == input.strategy_state();
            Ok((
                account,
                strategy_state,
                decision,
                strategy,
                unchanged,
                execution_urgency,
            ))
        })
        .collect::<Vec<Result<_, _>>>()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    let mut accounts = Vec::with_capacity(results.len());
    let mut intents = Vec::new();
    for (account, strategy_state, decision, strategy, unchanged, execution_urgency) in results {
        let StrategyDecision {
            intents: local_intents,
            reviewed_stocks,
            position_decision,
        } = decision;
        let updates_working_quotes = !local_intents.is_empty()
            || !reviewed_stocks.is_empty()
            || strategy.updates_working_quotes_on_empty_decision();
        let uses_parent_order_execution = strategy.uses_parent_order_execution();
        for (npc_local_index, intent) in local_intents.into_iter().enumerate() {
            let npc_local_index = u64::try_from(npc_local_index)
                .map_err(|_| NpcDecisionSourceError::IntentOrdinalOverflow { account })?;
            intents.push(NpcDecisionIntent {
                key: IntentCandidateKey::npc(account, npc_local_index),
                intent,
            });
        }
        accounts.push(NpcDecisionAccountOutput {
            account,
            #[cfg(test)]
            strategy_state: Some(strategy_state.clone()),
            strategy: Some(if unchanged {
                NpcStrategyUpdate::Unchanged
            } else {
                NpcStrategyUpdate::Replace(StoredStrategy::production_validated(
                    strategy,
                    strategy_state,
                ))
            }),
            reviewed_stocks,
            position_decision,
            execution_urgency,
            updates_working_quotes,
            uses_parent_order_execution,
        });
    }
    let account_ids = accounts
        .iter()
        .map(NpcDecisionAccountOutput::account)
        .collect();
    Ok(NpcDecisionSourceOutput {
        account_ids,
        accounts,
        intents,
    })
}

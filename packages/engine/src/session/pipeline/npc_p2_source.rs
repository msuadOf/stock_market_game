//! Pure NPC P2 source runner. Implementation owned by the L1 batch.
use super::{DecisionSnapshot, DecisionSnapshotError, P2CandidateKey};
use crate::strategy::{Intent, StrategyDecision, StrategyState, StrategyStateError};
use crate::{AccountId, SplitMix64};
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::sync::Arc;

/// An NPC raw intent with the identity that P2 composition must preserve.
#[derive(Clone, Debug, serde::Serialize)]
pub(in crate::session) struct NpcP2Intent {
    key: P2CandidateKey,
    intent: Intent,
}

impl NpcP2Intent {
    pub(in crate::session) const fn key(&self) -> &P2CandidateKey {
        &self.key
    }

    pub(in crate::session) const fn intent(&self) -> &Intent {
        &self.intent
    }
}

/// The prospective, non-routing decision result for one NPC account.
#[derive(Clone, Debug)]
pub(in crate::session) struct NpcP2AccountOutput {
    account: AccountId,
    strategy_state: StrategyState,
    reviewed_stocks: BTreeSet<crate::StockCode>,
    position_decision: Option<crate::behavior::PositionDecision>,
    updates_working_quotes: bool,
    uses_parent_order_execution: bool,
}

impl NpcP2AccountOutput {
    pub(in crate::session) const fn account(&self) -> AccountId {
        self.account
    }

    pub(in crate::session) const fn strategy_state(&self) -> &StrategyState {
        &self.strategy_state
    }

    pub(in crate::session) const fn reviewed_stocks(&self) -> &BTreeSet<crate::StockCode> {
        &self.reviewed_stocks
    }

    pub(in crate::session) const fn position_decision(
        &self,
    ) -> Option<&crate::behavior::PositionDecision> {
        self.position_decision.as_ref()
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
pub(in crate::session) struct NpcP2SourceOutput {
    account_ids: Vec<AccountId>,
    accounts: Vec<NpcP2AccountOutput>,
    intents: Vec<NpcP2Intent>,
}

impl NpcP2SourceOutput {
    pub(in crate::session) fn accounts(&self) -> &[AccountId] {
        &self.account_ids
    }

    pub(in crate::session) fn intents(&self) -> &[NpcP2Intent] {
        &self.intents
    }

    pub(in crate::session) fn account_outputs(&self) -> &[NpcP2AccountOutput] {
        &self.accounts
    }

    #[cfg(test)]
    pub(in crate::session) fn strategy_state(
        &self,
        account: AccountId,
    ) -> Result<&StrategyState, NpcP2SourceError> {
        self.accounts
            .iter()
            .find(|output| output.account == account)
            .map(NpcP2AccountOutput::strategy_state)
            .ok_or(NpcP2SourceError::MissingOutputState(account))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub(in crate::session) enum NpcP2SourceError {
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
pub(in crate::session) fn run_npc_p2_source(
    snapshot: Arc<DecisionSnapshot>,
) -> Result<NpcP2SourceOutput, NpcP2SourceError> {
    let results = snapshot
        .due_npc_ids()
        .par_iter()
        .copied()
        .map(|account| {
            let input = snapshot
                .account(account)
                .map_err(NpcP2SourceError::Snapshot)?;
            let mut strategy = input
                .strategy_state()
                .clone()
                .into_strategy()
                .map_err(|source| NpcP2SourceError::StrategyHydration { account, source })?;
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
            );
            let strategy_state = strategy
                .state()
                .map_err(|source| NpcP2SourceError::StrategyHydration { account, source })?;
            Ok((account, strategy_state, decision, strategy))
        })
        .collect::<Vec<Result<_, _>>>()
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;

    let mut accounts = Vec::with_capacity(results.len());
    let mut intents = Vec::new();
    for (account, strategy_state, decision, strategy) in results {
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
                .map_err(|_| NpcP2SourceError::IntentOrdinalOverflow { account })?;
            intents.push(NpcP2Intent {
                key: P2CandidateKey::npc(account, npc_local_index),
                intent,
            });
        }
        accounts.push(NpcP2AccountOutput {
            account,
            strategy_state,
            reviewed_stocks,
            position_decision,
            updates_working_quotes,
            uses_parent_order_execution,
        });
    }
    let account_ids = accounts.iter().map(NpcP2AccountOutput::account).collect();
    Ok(NpcP2SourceOutput {
        account_ids,
        accounts,
        intents,
    })
}

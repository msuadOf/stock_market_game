use crate::behavior::BehaviorMarketObservation;
use crate::observation::AccountRiskObservation;
use crate::strategy::{MarketView, SelfView, StrategyState};
use crate::{AccountId, AccountKind, RetailExperienceState, TradingPhase};
use std::collections::BTreeMap;

/// Owned, read-only strategy input sealed before P2 starts.
///
/// The type deliberately contains no router, order book, event sink, or mutable
/// session reference. P2 source runners return deltas instead of mutating this
/// snapshot or authoritative state.
#[derive(Clone, Debug)]
pub struct DecisionSnapshot {
    tick: u64,
    npc_seed_base: u64,
    phase: TradingPhase,
    market_minute: u64,
    market: MarketView,
    behavior_market: Option<BehaviorMarketObservation>,
    due_npc_ids: Vec<AccountId>,
    accounts: BTreeMap<AccountId, DecisionAccountInput>,
}

#[derive(Clone, Debug)]
pub struct DecisionAccountInput {
    kind: AccountKind,
    self_view: SelfView,
    strategy_state: StrategyState,
    account_risk: Option<AccountRiskObservation>,
    retail_experience: Option<RetailExperienceState>,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum DecisionSnapshotError {
    #[error("due NPC accounts must be strictly increasing")]
    NonCanonicalDueAccounts,
    #[error("due NPC account {0:?} has no sealed decision input")]
    MissingAccount(AccountId),
    #[error("player account {0:?} cannot be an NPC decision source")]
    PlayerInNpcBatch(AccountId),
    #[error("retail NPC account {0:?} has no shared behavior-market observation")]
    MissingRetailBehaviorMarket(AccountId),
    #[error("retail NPC account {0:?} has no account-risk observation")]
    MissingRetailAccountRisk(AccountId),
    #[error("retail NPC account {0:?} has no retail-experience state")]
    MissingRetailExperience(AccountId),
    #[error(
        "decision clock mismatch: header tick/minute {tick}/{market_minute}, market view {market_tick}/{market_view_minute}"
    )]
    MarketClockMismatch {
        tick: u64,
        market_minute: u64,
        market_tick: u64,
        market_view_minute: u64,
    },
}

impl DecisionAccountInput {
    pub fn new(
        kind: AccountKind,
        self_view: SelfView,
        strategy_state: StrategyState,
        account_risk: Option<AccountRiskObservation>,
        retail_experience: Option<RetailExperienceState>,
    ) -> Self {
        Self {
            kind,
            self_view,
            strategy_state,
            account_risk,
            retail_experience,
        }
    }

    pub const fn kind(&self) -> AccountKind {
        self.kind
    }

    pub const fn self_view(&self) -> &SelfView {
        &self.self_view
    }

    pub const fn strategy_state(&self) -> &StrategyState {
        &self.strategy_state
    }

    pub const fn account_risk(&self) -> Option<&AccountRiskObservation> {
        self.account_risk.as_ref()
    }

    pub const fn retail_experience(&self) -> Option<&RetailExperienceState> {
        self.retail_experience.as_ref()
    }
}

impl DecisionSnapshot {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        tick: u64,
        npc_seed_base: u64,
        phase: TradingPhase,
        market_minute: u64,
        market: MarketView,
        behavior_market: Option<BehaviorMarketObservation>,
        due_npc_ids: Vec<AccountId>,
        accounts: BTreeMap<AccountId, DecisionAccountInput>,
    ) -> Result<Self, DecisionSnapshotError> {
        if market.tick != tick || market.market_minute != market_minute {
            return Err(DecisionSnapshotError::MarketClockMismatch {
                tick,
                market_minute,
                market_tick: market.tick,
                market_view_minute: market.market_minute,
            });
        }
        if due_npc_ids.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(DecisionSnapshotError::NonCanonicalDueAccounts);
        }
        if let Some(missing) = due_npc_ids
            .iter()
            .copied()
            .find(|account| !accounts.contains_key(account))
        {
            return Err(DecisionSnapshotError::MissingAccount(missing));
        }
        for account in &due_npc_ids {
            let input = &accounts[account];
            if input.kind == AccountKind::Player {
                return Err(DecisionSnapshotError::PlayerInNpcBatch(*account));
            }
            if input.kind == AccountKind::Retail {
                if behavior_market.is_none() {
                    return Err(DecisionSnapshotError::MissingRetailBehaviorMarket(*account));
                }
                if input.account_risk.is_none() {
                    return Err(DecisionSnapshotError::MissingRetailAccountRisk(*account));
                }
                if input.retail_experience.is_none() {
                    return Err(DecisionSnapshotError::MissingRetailExperience(*account));
                }
            }
        }
        Ok(Self {
            tick,
            npc_seed_base,
            phase,
            market_minute,
            market,
            behavior_market,
            due_npc_ids,
            accounts,
        })
    }

    pub const fn tick(&self) -> u64 {
        self.tick
    }

    /// Authoritative session seed used by the legacy-compatible per-account P2 RNG derivation.
    pub const fn npc_seed_base(&self) -> u64 {
        self.npc_seed_base
    }

    pub const fn phase(&self) -> TradingPhase {
        self.phase
    }

    pub const fn market_minute(&self) -> u64 {
        self.market_minute
    }

    pub const fn market(&self) -> &MarketView {
        &self.market
    }

    pub const fn behavior_market(&self) -> Option<&BehaviorMarketObservation> {
        self.behavior_market.as_ref()
    }

    pub fn due_npc_ids(&self) -> &[AccountId] {
        &self.due_npc_ids
    }

    pub fn account(
        &self,
        account: AccountId,
    ) -> Result<&DecisionAccountInput, DecisionSnapshotError> {
        self.accounts
            .get(&account)
            .ok_or(DecisionSnapshotError::MissingAccount(account))
    }
}

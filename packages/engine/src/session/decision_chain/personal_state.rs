//! Personal state owned by one account while its plan root observes the frozen market.

use crate::experience::{PersonalPriceMemory, PersonalWatchlist};
use crate::information::NpcInformationState;
use crate::session::attention::NpcAttentionState;
use crate::strategy::BeliefBook;
use crate::{AccountId, GameSession};

pub(in crate::session) struct PlanPersonalState {
    pub(super) attention: NpcAttentionState,
    pub(super) watchlist: PersonalWatchlist,
    pub(super) price_memory: PersonalPriceMemory,
    pub(super) information: NpcInformationState,
    pub(super) belief: BeliefBook,
}

impl PlanPersonalState {
    pub(in crate::session) fn take(session: &mut GameSession, account: AccountId) -> Self {
        Self {
            attention: session
                .npc_attention
                .remove(&account)
                .unwrap_or_else(|| panic!("plan account {account:?} has no attention state")),
            watchlist: session
                .watchlists
                .remove(&account)
                .unwrap_or_else(|| panic!("plan account {account:?} has no watchlist")),
            price_memory: session
                .price_memories
                .remove(&account)
                .unwrap_or_else(|| panic!("plan account {account:?} has no price memory")),
            information: session
                .information
                .remove(&account)
                .unwrap_or_else(|| panic!("plan account {account:?} has no information state")),
            belief: session
                .belief_books
                .remove(&account)
                .unwrap_or_else(|| panic!("plan account {account:?} has no belief book")),
        }
    }

    pub(in crate::session) fn install(self, session: &mut GameSession, account: AccountId) {
        assert!(
            session
                .npc_attention
                .insert(account, self.attention)
                .is_none(),
            "plan account {account:?} attention state was installed twice"
        );
        assert!(
            session.watchlists.insert(account, self.watchlist).is_none(),
            "plan account {account:?} watchlist was installed twice"
        );
        assert!(
            session
                .price_memories
                .insert(account, self.price_memory)
                .is_none(),
            "plan account {account:?} price memory was installed twice"
        );
        assert!(
            session
                .information
                .insert(account, self.information)
                .is_none(),
            "plan account {account:?} information was installed twice"
        );
        assert!(
            session.belief_books.insert(account, self.belief).is_none(),
            "plan account {account:?} belief book was installed twice"
        );
    }
}

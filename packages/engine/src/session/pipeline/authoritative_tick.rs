//! Single production dispatcher for one complete escrow-backed market tick.
//!
//! Phase-specific transactions own their complete P0-P9 candidate and expose
//! only an infallible authority swap after every validation has succeeded.

use super::{
    b1_continuous_transaction::prepare_b1_continuous_tick_with_guard,
    b2_auction_transaction::prepare_b2_auction_tick_with_guard,
    p9_candidate_commit::P8AuthorityGuard, pre_open_transaction::prepare_pre_open_tick_with_guard,
};
use crate::session::{hash::RollbackHashes, StepFatal};
use crate::{Event, GameSession, TradingPhase};

pub(in crate::session) fn execute_authoritative_tick(
    authority: &mut GameSession,
    rollback_before: RollbackHashes,
) -> Result<Vec<Event>, StepFatal> {
    let guard = P8AuthorityGuard::from_rollback_hashes(rollback_before);
    let events = match authority.phase() {
        TradingPhase::Continuous => prepare_b1_continuous_tick_with_guard(authority, guard)
            .map_err(|error| error.into_fatal())?
            .commit()
            .into_events(),
        TradingPhase::CallAuction | TradingPhase::ClosingAuction => {
            prepare_b2_auction_tick_with_guard(authority, guard)
                .map_err(|error| error.into_fatal())?
                .commit()
                .into_events()
        }
        TradingPhase::PreOpen => prepare_pre_open_tick_with_guard(authority, guard)
            .map_err(|error| error.into_fatal())?
            .commit()
            .into_events(),
    };
    Ok(events)
}

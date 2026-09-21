//! Single production dispatcher for one complete escrow-backed market tick.
//!
//! Phase-specific transactions own their complete P0-P9 candidate and expose
//! only an infallible authority swap after every validation has succeeded.

use super::{
    b1_continuous_transaction::prepare_b1_continuous_tick,
    b2_auction_transaction::prepare_b2_auction_tick, pre_open_transaction::prepare_pre_open_tick,
};
use crate::session::StepFatal;
use crate::{Event, GameSession, TradingPhase};

pub(in crate::session) fn execute_authoritative_tick(
    authority: &mut GameSession,
) -> Result<Vec<Event>, StepFatal> {
    let events = match authority.phase() {
        TradingPhase::Continuous => prepare_b1_continuous_tick(authority)
            .map_err(|error| error.into_fatal())?
            .commit()
            .into_events(),
        TradingPhase::CallAuction | TradingPhase::ClosingAuction => {
            prepare_b2_auction_tick(authority)
                .map_err(|error| error.into_fatal())?
                .commit()
                .into_events()
        }
        TradingPhase::PreOpen => prepare_pre_open_tick(authority)
            .map_err(|error| error.into_fatal())?
            .commit()
            .into_events(),
    };
    Ok(events)
}

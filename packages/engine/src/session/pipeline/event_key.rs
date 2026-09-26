use super::super::Event;
use crate::{AccountId, StockCode};
use std::cmp::Ordering;

/// P0 cancellations and P7 book events share the accepted Account/Sealed wire
/// domain. Reserve the top u32-sized range for account-local P0 identities so
/// later P7 activity cannot change the identity of an earlier expiry fact.
pub(super) const P0_EVENT_INDEX_BASE: u64 = crate::orderbook::js_safe_u64::MAX - u32::MAX as u64;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum EntityTag {
    Session,
    Stock(StockCode),
    Account(AccountId),
}

impl EntityTag {
    pub const fn rank(&self) -> u8 {
        match self {
            Self::Stock(_) => 0,
            Self::Account(_) => 1,
            Self::Session => 2,
        }
    }
}
impl Ord for EntityTag {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank()
            .cmp(&other.rank())
            .then_with(|| match (self, other) {
                (Self::Stock(left), Self::Stock(right)) => left.cmp(right),
                (Self::Account(left), Self::Account(right)) => left.cmp(right),
                (Self::Session | Self::Stock(_) | Self::Account(_), _) => Ordering::Equal,
            })
    }
}
impl PartialOrd for EntityTag {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum EventSourceIndex {
    Sealed,
    P0,
    PriceTick,
    DayEnd,
    Session,
}
impl EventSourceIndex {
    pub const fn rank(self) -> u8 {
        match self {
            Self::Sealed => 0,
            Self::P0 => 1,
            Self::PriceTick => 2,
            Self::DayEnd => 3,
            Self::Session => 4,
        }
    }
}
impl Ord for EventSourceIndex {
    fn cmp(&self, other: &Self) -> Ordering {
        self.rank().cmp(&other.rank())
    }
}
impl PartialOrd for EventSourceIndex {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Stable identity only, never business causality. Fields cannot be independently forged.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize, ts_rs::TS,
)]
pub struct EventStableKey {
    phase_rank: u8,
    entity: EntityTag,
    source: EventSourceIndex,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    local_event_index: u64,
}
impl EventStableKey {
    pub fn for_event(event: &Event, local_event_index: u64) -> Self {
        let (phase_rank, entity, source) = match event {
            Event::Trade { code, .. } => {
                (4, EntityTag::Stock(code.clone()), EventSourceIndex::Sealed)
            }
            Event::AuctionTick { code, .. }
            | Event::AuctionCompleted { code, .. }
            | Event::PriceTick { code, .. } => (
                4,
                EntityTag::Stock(code.clone()),
                EventSourceIndex::PriceTick,
            ),
            Event::DayBoundary { .. } => (5, EntityTag::Session, EventSourceIndex::DayEnd),
            Event::CivilDateAdvanced { .. } | Event::CompanyDisclosurePublished { .. } => {
                (6, EntityTag::Session, EventSourceIndex::Session)
            }
            Event::IntentRejected { account, .. }
            | Event::SettlementError { account, .. }
            | Event::OrderCanceled { account, .. }
            | Event::OrderAccepted { account, .. } => {
                (4, EntityTag::Account(*account), EventSourceIndex::Sealed)
            }
        };
        Self {
            phase_rank,
            entity,
            source,
            local_event_index,
        }
    }
    pub const fn entity(&self) -> &EntityTag {
        &self.entity
    }
    pub const fn phase_rank(&self) -> u8 {
        self.phase_rank
    }
    pub const fn source(&self) -> EventSourceIndex {
        self.source
    }
    pub const fn local_event_index(&self) -> u64 {
        self.local_event_index
    }
}

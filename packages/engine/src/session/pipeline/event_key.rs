use super::super::{Event, StepFatal};
use crate::{AccountId, StockCode};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};

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
            Event::CivilDateAdvanced { .. }
            | Event::CompanyDisclosurePublished { .. }
            | Event::ResourceLimit { .. } => (6, EntityTag::Session, EventSourceIndex::Session),
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

#[derive(Clone, Debug, PartialEq)]
pub struct KeyedEvent<'a> {
    pub key: EventStableKey,
    pub event: &'a Event,
}

/// Compatibility adapter for the deterministic legacy emitter, NOT arbitrary worker
/// outboxes. Future phase workers must attach explicit source-local keys at emission.
/// All phase-6 Session variants share ONE ordinal domain. Keep keys attached when
/// permuting facts; re-keying a permuted array changes identity. No seq allocation.
#[derive(Default)]
pub struct EventKeyStream {
    next: BTreeMap<(u8, EntityTag, EventSourceIndex), u64>,
    last_legacy_seq: Option<u64>,
}
impl EventKeyStream {
    pub fn attach_legacy_emission<'a>(
        &mut self,
        events: &'a [Event],
    ) -> Result<Vec<KeyedEvent<'a>>, StepFatal> {
        let mut previous = self.last_legacy_seq;
        for event in events {
            if previous.is_some_and(|seq| event.seq() <= seq) {
                return Err(StepFatal::InvariantViolation {
                    description: "legacy adapter requires original strictly increasing emission seq; attach identities before permutation".to_owned(),
                    location: "EventKeyStream::attach_legacy_emission".to_owned(),
                });
            }
            previous = Some(event.seq());
        }
        let mut result = Vec::with_capacity(events.len());
        for event in events {
            let mut key = EventStableKey::for_event(event, 0);
            let next = self
                .next
                .entry((key.phase_rank, key.entity.clone(), key.source))
                .or_default();
            key.local_event_index = *next;
            *next = next
                .checked_add(1)
                .ok_or_else(|| StepFatal::InvariantViolation {
                    description: "event stream ordinal overflow".to_owned(),
                    location: "EventKeyStream::attach_legacy_emission".to_owned(),
                })?;
            result.push(KeyedEvent { key, event });
        }
        self.last_legacy_seq = previous;
        Ok(result)
    }
}

pub fn validate_event_keys(events: &[KeyedEvent<'_>]) -> Result<(), StepFatal> {
    let mut seen = BTreeSet::new();
    for event in events {
        if !seen.insert(&event.key) {
            return Err(StepFatal::InvariantViolation {
                description: format!("duplicate event identity: {:?}", event.key),
                location: "pipeline::validate_event_keys".to_owned(),
            });
        }
    }
    Ok(())
}

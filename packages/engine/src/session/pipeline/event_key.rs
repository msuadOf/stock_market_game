use super::super::Event;
use crate::{AccountId, StockCode};
use std::cmp::Ordering;

#[cfg(test)]
mod source_contract_tests {
    use super::super::conservation::ConservationBasis;
    use super::super::{EnvelopeOrigin, ReceiptSource, ResVec};
    use super::EventSourceIndex;

    #[test]
    fn quote_expiry_source_uses_responsibility_tag_and_preserves_rank() {
        assert_eq!(EventSourceIndex::QuoteExpiry.rank(), 1);
        assert_eq!(
            serde_json::to_value(EventSourceIndex::QuoteExpiry).unwrap(),
            serde_json::json!("QuoteExpiry")
        );
        assert_eq!(
            serde_json::from_value::<EventSourceIndex>(serde_json::json!("QuoteExpiry")).unwrap(),
            EventSourceIndex::QuoteExpiry
        );
        assert!(serde_json::from_value::<EventSourceIndex>(serde_json::json!("P0")).is_err());
    }

    #[test]
    fn receipt_and_conservation_wire_names_describe_resource_origins() {
        assert_eq!(
            serde_json::to_value(ReceiptSource::QuoteExpiry(7)).unwrap(),
            serde_json::json!({"QuoteExpiry": 7})
        );
        assert_eq!(
            serde_json::to_value(EnvelopeOrigin::CreatedAtValidation).unwrap(),
            serde_json::json!("CreatedAtValidation")
        );
        let basis =
            serde_json::to_value(ConservationBasis::CreatedAtValidation(ResVec::ZERO)).unwrap();
        assert_eq!(
            basis.as_object().unwrap().keys().collect::<Vec<_>>(),
            vec!["CreatedAtValidation"]
        );
        let release = serde_json::to_value(super::super::ledger::ConservationState::EMPTY).unwrap();
        assert!(release.get("expiry_released").is_some());
        assert!(release.get("p0_released").is_none());
    }
}

/// ExpiryShadow 撤单与 Projection 订单簿事件共享既定 Account/Sealed wire 身份域。
/// 为账户局部报价过期身份预留最高的 u32 范围；
/// 后续 Projection 活动不能改变早先报价过期事实的身份。
pub(crate) const QUOTE_EXPIRY_EVENT_INDEX_BASE: u64 =
    crate::orderbook::js_safe_u64::MAX - u32::MAX as u64;

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
pub enum EntityTag {
    Session,
    Stock(StockCode),
    Account(
        #[serde(with = "crate::session::memberships::account_id_decimal")]
        #[ts(type = "string")]
        AccountId,
    ),
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
    QuoteExpiry,
    PriceTick,
    DayEnd,
    Session,
}
impl EventSourceIndex {
    pub const fn rank(self) -> u8 {
        match self {
            Self::Sealed => 0,
            Self::QuoteExpiry => 1,
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
            Event::Trade { code, .. } | Event::PublicTrade { code, .. } => {
                (4, EntityTag::Stock(code.clone()), EventSourceIndex::Sealed)
            }
            Event::PrivateEventOmitted { .. } => (4, EntityTag::Session, EventSourceIndex::Sealed),
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

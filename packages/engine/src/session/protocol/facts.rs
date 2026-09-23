use super::ProtocolError;
use crate::session::{
    pipeline::{EventKeyStream, EventStableKey},
    Event,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub struct EventFact {
    pub key: EventStableKey,
    pub event: Event,
    pub canonical_payload: String,
}

pub fn attach_facts(events: &[Event]) -> Result<Vec<EventFact>, ProtocolError> {
    EventKeyStream::default()
        .attach_legacy_emission(events)
        .map_err(|_| ProtocolError::FactIdentity)?
        .into_iter()
        .map(|fact| {
            Ok(EventFact {
                key: fact.key,
                event: fact.event.clone(),
                canonical_payload: canonical(fact.event)?,
            })
        })
        .collect()
}

pub(super) fn canonical(value: &impl serde::Serialize) -> Result<String, ProtocolError> {
    let value = serde_json::to_value(value).map_err(|_| ProtocolError::FactIdentity)?;
    serde_json::to_string(&value).map_err(|_| ProtocolError::FactIdentity)
}

pub(super) fn validate_facts(facts: &[EventFact], events: &[Event]) -> Result<(), ProtocolError> {
    if facts.len() != events.len() {
        return Err(ProtocolError::FactIdentity);
    }
    let mut keys = BTreeSet::new();
    let mut represented = BTreeMap::new();
    for fact in facts {
        if fact.key != EventStableKey::for_event(&fact.event, fact.key.local_event_index())
            || !keys.insert(&fact.key)
            || fact.canonical_payload != canonical(&fact.event)?
            || represented
                .insert(fact.event.seq(), fact.canonical_payload.clone())
                .is_some()
        {
            return Err(ProtocolError::FactIdentity);
        }
    }
    let mut supplied = BTreeMap::new();
    for event in events {
        if supplied.insert(event.seq(), canonical(event)?).is_some() {
            return Err(ProtocolError::FactIdentity);
        }
    }
    if supplied != represented {
        return Err(ProtocolError::FactIdentity);
    }
    Ok(())
}

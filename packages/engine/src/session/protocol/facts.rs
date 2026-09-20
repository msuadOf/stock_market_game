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
    attach_facts_after(&[], events)
}

pub(crate) fn attach_facts_after(
    preceding: &[EventFact],
    events: &[Event],
) -> Result<Vec<EventFact>, ProtocolError> {
    let mut stream = EventKeyStream::default();
    let preceding_events = preceding
        .iter()
        .map(|fact| fact.event.clone())
        .collect::<Vec<_>>();
    validate_facts(preceding, &preceding_events)?;
    let reconstructed = stream
        .attach_legacy_emission(&preceding_events)
        .map_err(|_| ProtocolError::FactIdentity)?;
    if reconstructed
        .iter()
        .zip(preceding)
        .any(|(keyed, fact)| keyed.key != fact.key)
    {
        return Err(ProtocolError::FactIdentity);
    }
    stream
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_after_rejects_a_preceding_fact_with_tampered_payload() {
        let event = Event::ResourceLimit {
            seq: 1,
            resource: crate::session::RuntimeResource::PendingPlanEvents,
            limit: 1,
        };
        let mut preceding = attach_facts(&[event]).unwrap();
        preceding[0].canonical_payload.push(' ');

        assert!(matches!(
            attach_facts_after(&preceding, &[]),
            Err(ProtocolError::FactIdentity)
        ));
    }
}

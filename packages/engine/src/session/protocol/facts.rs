use super::ProtocolError;
use crate::session::{
    pipeline::{EntityTag, EventSourceIndex, EventStableKey},
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

pub(in crate::session) fn attach_facts_with_keys(
    events: &[Event],
    keys: &[EventStableKey],
) -> Result<Vec<EventFact>, ProtocolError> {
    if events.len() != keys.len() {
        return Err(ProtocolError::FactIdentity);
    }
    let facts = events
        .iter()
        .zip(keys)
        .map(|(event, key)| {
            Ok(EventFact {
                key: key.clone(),
                event: event.clone(),
                canonical_payload: canonical(event)?,
            })
        })
        .collect::<Result<Vec<_>, ProtocolError>>()?;
    Ok(facts)
}

pub(crate) fn attach_facts_after(
    preceding: &[EventFact],
    events: &[Event],
) -> Result<Vec<EventFact>, ProtocolError> {
    validate_fact_collection(preceding)?;
    let mut next = BTreeMap::<(u8, EntityTag, EventSourceIndex), u64>::new();
    let mut last_seq = None;
    for fact in preceding {
        if last_seq.is_some_and(|seq| fact.event.seq() <= seq) {
            return Err(ProtocolError::FactIdentity);
        }
        last_seq = Some(fact.event.seq());
        let domain = (
            fact.key.phase_rank(),
            fact.key.entity().clone(),
            fact.key.source(),
        );
        let index = fact.key.local_event_index();
        next.entry(domain)
            .and_modify(|last| *last = (*last).max(index))
            .or_insert(index);
    }
    events
        .iter()
        .map(|event| {
            if last_seq.is_some_and(|seq| event.seq() <= seq) {
                return Err(ProtocolError::FactIdentity);
            }
            last_seq = Some(event.seq());
            let domain = EventStableKey::for_event(event, 0);
            let domain = (
                domain.phase_rank(),
                domain.entity().clone(),
                domain.source(),
            );
            let index = next
                .get(&domain)
                .map_or(Some(0), |last| last.checked_add(1))
                .ok_or(ProtocolError::FactIdentity)?;
            next.insert(domain, index);
            let key = EventStableKey::for_event(event, index);
            Ok(EventFact {
                key,
                event: event.clone(),
                canonical_payload: canonical(event)?,
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
    let represented = validate_fact_collection(facts)?;
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

fn validate_fact_collection(facts: &[EventFact]) -> Result<BTreeMap<u64, String>, ProtocolError> {
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
    Ok(represented)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attach_after_rejects_a_preceding_fact_with_tampered_payload() {
        let event = Event::CivilDateAdvanced {
            seq: 1,
            settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        let mut preceding = attach_facts(&[event]).unwrap();
        preceding[0].canonical_payload.push(' ');

        assert!(matches!(
            attach_facts_after(&preceding, &[]),
            Err(ProtocolError::FactIdentity)
        ));
    }

    #[test]
    fn civil_facts_continue_sparse_producer_identity_without_rekeying_it() {
        let first = Event::CivilDateAdvanced {
            seq: 1,
            settled_date: crate::CivilDate::from_iso("2030-01-06").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        let key = EventStableKey::for_event(&first, 7);
        let preceding = attach_facts_with_keys(&[first], &[key]).unwrap();
        let next = Event::CivilDateAdvanced {
            seq: 2,
            settled_date: crate::CivilDate::from_iso("2030-01-07").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-08").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        let continued = attach_facts_after(&preceding, &[next]).unwrap();
        assert_eq!(preceding[0].key.local_event_index(), 7);
        assert_eq!(continued[0].key.local_event_index(), 8);
    }
}

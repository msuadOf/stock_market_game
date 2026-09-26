//! P7 canonical event collector.
//!
//! Workers hand this stage owned event facts with their source-local identities already
//! attached.  P7 validates and orders those identities; it never infers them from a
//! legacy emission `seq` and never repairs producer ordinals.

use super::{Event, EventStableKey, StepFatal};

/// An event emitted by a worker together with the identity assigned at its source.
///
/// The fields are pipeline-private on purpose: production callers must not turn this
/// into a general event-rekeying API.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct OwnedEventFact {
    pub(super) key: EventStableKey,
    pub(super) event: Event,
}

/// P7's pure output. `next_seq` is the last assigned external event sequence cursor.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CollectedEvents {
    pub(super) events: Vec<Event>,
    pub(super) keys: Vec<EventStableKey>,
    pub(super) next_seq: u64,
}

/// Validates, canonically orders, and only then assigns external event sequences.
///
/// `next_seq` is the previously committed cursor, so the first emitted event receives
/// `next_seq + 1`.  Because the cursor is passed by value and output is returned only on
/// success, any validation or overflow failure leaks neither partial events nor a cursor
/// change to the caller.
pub(super) fn collect_events(
    mut facts: Vec<OwnedEventFact>,
    next_seq: u64,
) -> Result<CollectedEvents, StepFatal> {
    for fact in &facts {
        validate_mapping(fact)?;
    }
    facts.sort_by(|left, right| left.key.cmp(&right.key));
    validate_unique_keys(&facts)?;

    let mut cursor = next_seq;
    let mut events = Vec::with_capacity(facts.len());
    let mut keys = Vec::with_capacity(facts.len());
    for mut fact in facts {
        cursor = cursor.checked_add(1).ok_or_else(|| {
            invariant("external event sequence overflow while collecting canonical event facts")
        })?;
        set_seq(&mut fact.event, cursor);
        keys.push(fact.key);
        events.push(fact.event);
    }
    Ok(CollectedEvents {
        events,
        keys,
        next_seq: cursor,
    })
}

fn validate_mapping(fact: &OwnedEventFact) -> Result<(), StepFatal> {
    let expected = EventStableKey::for_event(&fact.event, fact.key.local_event_index());
    if expected != fact.key {
        return Err(invariant(
            "worker event key does not match the event variant, entity, phase, or source",
        ));
    }
    Ok(())
}

fn validate_unique_keys(facts: &[OwnedEventFact]) -> Result<(), StepFatal> {
    if facts.windows(2).any(|pair| pair[0].key == pair[1].key) {
        return Err(invariant("duplicate worker event stable key"));
    }
    Ok(())
}

/// Keep this exhaustive so adding an `Event` variant requires an explicit P7 sequence
/// assignment decision.  Identity validation remains separate and uses `for_event`.
fn set_seq(event: &mut Event, sequence: u64) {
    match event {
        Event::Trade { seq, .. }
        | Event::AuctionTick { seq, .. }
        | Event::AuctionCompleted { seq, .. }
        | Event::PriceTick { seq, .. }
        | Event::DayBoundary { seq, .. }
        | Event::CivilDateAdvanced { seq, .. }
        | Event::CompanyDisclosurePublished { seq, .. }
        | Event::IntentRejected { seq, .. }
        | Event::SettlementError { seq, .. }
        | Event::OrderCanceled { seq, .. }
        | Event::OrderAccepted { seq, .. } => *seq = sequence,
    }
}

fn invariant(description: &str) -> StepFatal {
    StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::p7_events::collect_events".to_owned(),
    }
}

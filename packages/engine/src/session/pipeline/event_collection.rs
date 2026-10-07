//! Projection canonical event collector.
//!
//! Workers hand this stage owned event facts with their source-local identities already
//! attached.  Projection validates and orders those identities; it never infers identities from
//! a producer's emission `seq` and never repairs producer ordinals.

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

/// Projection's pure output. `next_seq` is the last assigned external event sequence cursor.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct CollectedEvents {
    pub(super) events: Vec<Event>,
    pub(super) keys: Vec<EventStableKey>,
    pub(super) next_seq: u64,
}

/// Validates uniqueness and assigns external event sequences in arrival order.
///
/// `next_seq` is the previously committed cursor, so the first emitted event receives
/// `next_seq + 1`.  Because the cursor is passed by value and output is returned only on
/// success, any validation or overflow failure leaks neither partial events nor a cursor
/// change to the caller.
///
/// 跨实体事件不再按 StableKey 全局排序（用户已授权放宽：无关实体不要求固定次序）；
/// key 唯一性由 BTreeSet 独立校验，seq 按实际到达顺序连续分配。
pub(super) fn collect_events(
    facts: Vec<OwnedEventFact>,
    next_seq: u64,
) -> Result<CollectedEvents, StepFatal> {
    for fact in &facts {
        validate_mapping(fact)?;
    }
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
    let mut seen = std::collections::BTreeSet::new();
    for fact in facts {
        if !seen.insert(&fact.key) {
            return Err(invariant("duplicate worker event stable key"));
        }
    }
    Ok(())
}

/// Keep this exhaustive so adding an `Event` variant requires an explicit Projection sequence
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
        location: "pipeline::event_collection::collect_events".to_owned(),
    }
}

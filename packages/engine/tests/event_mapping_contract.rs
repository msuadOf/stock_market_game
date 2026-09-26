#[path = "support/event_mapping.rs"]
mod event_mapping;

use engine::session::pipeline::{EntityTag, EventStableKey};

#[test]
fn every_event_matches_adr_phase_entity_value_and_source() {
    let events = event_mapping::events();
    let mut variants = std::collections::BTreeSet::new();
    for event in &events {
        let (variant, phase, entity, source) = event_mapping::adr_mapping(event);
        let key = EventStableKey::for_event(event, 29);
        let observed_entity = match key.entity() {
            EntityTag::Stock(code) => format!("Stock:{}", code.0),
            EntityTag::Account(account) => format!("Account:{}", account.0),
            EntityTag::Session => "Session".into(),
        };
        assert_eq!(
            (key.phase_rank(), observed_entity, key.source().rank()),
            (phase, entity, source),
            "{variant}"
        );
        assert_eq!(key.local_event_index(), 29);
        assert!(variants.insert(variant));
    }
    assert_eq!(variants.len(), 11);
}

#[path = "../../tests/support/event_mapping.rs"]
mod event_mapping;

use engine::session::pipeline::{EntityTag, EventStableKey};

#[test]
fn every_harness_variant_agrees_with_adr_and_production_entity() {
    let events = event_mapping::events();
    for event in &events {
        let (expected_variant, _, expected_entity, _) = event_mapping::adr_mapping(event);
        let (variant, entity, _) = crate::frames::variant(event);
        let key = EventStableKey::for_event(event, 0);
        let production_entity = match key.entity() {
            EntityTag::Stock(code) => format!("Stock:{}", code.0),
            EntityTag::Account(account) => format!("Account:{}", account.0),
            EntityTag::Session => "Session".into(),
        };
        assert_eq!(variant, expected_variant);
        assert_eq!(entity, expected_entity, "{variant}");
        assert_eq!(entity, production_entity, "{variant}");
    }
}

#[test]
fn harness_and_production_share_phase_six_session_ordinals() {
    let events = event_mapping::events();
    let facts = serde_json::to_value(crate::frames::project(1, events.clone()).unwrap()).unwrap();
    let keyed = engine::session::pipeline::EventKeyStream::default()
        .attach_legacy_emission(&events)
        .unwrap();
    let mut ordinals = Vec::new();
    for (index, fact) in keyed.iter().enumerate() {
        if fact.key.phase_rank() == 6 {
            let ordinal = fact.key.local_event_index();
            assert_eq!(facts[index]["canonical_session_ordinal"], ordinal);
            ordinals.push(ordinal);
        } else {
            assert!(facts[index]["canonical_session_ordinal"].is_null());
        }
    }
    assert_eq!(ordinals, [0, 1, 2]);
}

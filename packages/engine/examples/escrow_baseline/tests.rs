use crate::{
    frames,
    scenarios::{collect, Scenario},
    CorpusError,
};

#[test]
fn malformed_scenario_is_rejected() {
    assert!(Scenario::parse("invalid").is_err());
}

#[test]
fn classes_have_real_frames_and_controlled_restore() -> Result<(), CorpusError> {
    for name in ["equivalence", "divergence-9", "representation", "stress"] {
        let records = collect(Scenario::parse(name)?, 1)?;
        assert!(
            records
                .iter()
                .filter(|record| record["kind"] == "TickFrame")
                .count()
                >= 20
        );
        if name == "representation" {
            assert!(records
                .iter()
                .any(|record| record["kind"] == "after_restore"));
        }
        if name == "divergence-9" {
            let boundary = records
                .iter()
                .find(|record| record["kind"] == "acceptance_boundary")
                .ok_or_else(|| {
                    CorpusError::Invariant("missing acceptance projection".to_owned())
                })?;
            assert_eq!(
                boundary["old_funded"]["snapshot"]["accounts"]["0"]["reserved_cash"],
                400
            );
            assert!(boundary["old_zero_cash"]["events"]
                .as_array()
                .ok_or_else(|| CorpusError::Invariant("missing events".to_owned()))?
                .iter()
                .any(|fact| fact["event"]["IntentRejected"]["reason"] == "InsufficientCash"));
        }
    }
    Ok(())
}

#[test]
fn resource_and_civil_events_share_session_ordinal() -> Result<(), CorpusError> {
    let settled_date = engine::CivilDate::from_iso("2030-01-01")
        .map_err(|error| CorpusError::Arguments(error.to_string()))?;
    let next_date = engine::CivilDate::from_iso("2030-01-02")
        .map_err(|error| CorpusError::Arguments(error.to_string()))?;
    let events = vec![
        engine::Event::ResourceLimit {
            seq: 1,
            resource: engine::session::RuntimeResource::PendingPlanEvents,
            limit: 10,
        },
        engine::Event::CivilDateAdvanced {
            seq: 2,
            settled_date,
            next_date,
            next_status: engine::calendar::DayStatus::Trading,
        },
        engine::Event::ResourceLimit {
            seq: 3,
            resource: engine::session::RuntimeResource::PendingPlanEvents,
            limit: 20,
        },
    ];
    let projected = serde_json::to_value(frames::project(1, events)?)?;
    for index in 0..3 {
        assert_eq!(projected[index]["canonical_session_ordinal"], index);
        assert_eq!(projected[index]["identity"][2], "Session");
    }
    Ok(())
}

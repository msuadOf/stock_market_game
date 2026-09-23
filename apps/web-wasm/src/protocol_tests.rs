#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use super::*;

#[test]
fn registry_step_returns_one_valid_frame_and_invalid_handle_is_explicit() {
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let session = ProtocolSession::new(setup, 41).unwrap();
    REGISTRY.with(|registry| registry.borrow_mut().insert(123, session));

    let update = step_update(123).unwrap();

    let EngineUpdate::TickBatch(batch) = &update else {
        panic!()
    };
    batch.validate().unwrap();
    assert_eq!(batch.frames.len(), 1);
    assert_eq!(batch.frames[0].tick, 1);
    println!(
        "wasm registry output: {}",
        serde_json::to_string(&update).unwrap()
    );
    drop_session(123);
    assert_eq!(step_update(123).unwrap_err(), "invalid session handle: 123");
}

#[test]
fn registry_retains_complete_closing_history() {
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .insert(124, ProtocolSession::new(setup, 41).unwrap())
    });
    for _ in 0..fixture::TICKS_PER_DAY {
        step_update(124).unwrap();
    }

    let civil = REGISTRY.with(|registry| {
        registry
            .borrow_mut()
            .get_mut(&124)
            .unwrap()
            .end_civil_day_update()
            .unwrap()
    });

    civil.validate().unwrap();
    assert_eq!(
        civil.refresh.intraday.len(),
        usize::try_from(fixture::TICKS_PER_DAY).unwrap()
    );
    println!(
        "wasm registry civil: {}",
        serde_json::to_string(&EngineUpdate::CivilUpdate(Box::new(civil))).unwrap()
    );
    drop_session(124);
}

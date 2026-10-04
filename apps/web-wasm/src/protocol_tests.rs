#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use super::*;

#[test]
fn exhausted_handles_never_wrap_to_zero_or_replace_live_sessions() {
    let next = AtomicU32::new(u32::MAX - 1);
    let mut registry = SessionRegistry::default();
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    for seed in [41, 42] {
        let session = ProtocolSession::new(setup.clone(), seed).unwrap();
        assert!(registry.register(session, &next).is_ok());
    }
    let exhausted = ProtocolSession::new(setup, 43).unwrap();
    assert!(
        matches!(registry.register(exhausted, &next), Err(SessionError::ResourceLimit(message)) if message.contains("句柄已耗尽"))
    );
    assert_eq!(registry.sessions.len(), 2);
    assert!(!registry.sessions.contains_key(&0));
    assert_eq!(next.load(Ordering::SeqCst), 0);
    assert_eq!(
        registry.sessions[&(u32::MAX - 1)]
            .game()
            .save()
            .unwrap()
            .seed,
        41
    );
    assert_eq!(registry.sessions[&u32::MAX].game().save().unwrap().seed, 42);
    registry.remove(u32::MAX);
    assert!(matches!(
        reserve_session_handle(&next),
        Err(SessionError::ResourceLimit(_))
    ));
}

#[test]
fn handle_collision_is_explicit_and_keeps_the_original_session() {
    let mut registry = SessionRegistry::default();
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    registry
        .sessions
        .insert(7, ProtocolSession::new(setup.clone(), 41).unwrap());
    let next = AtomicU32::new(7);
    let duplicate = ProtocolSession::new(setup, 42).unwrap();
    assert!(matches!(
        registry.register(duplicate, &next),
        Err(SessionError::Step(_))
    ));
    assert_eq!(registry.sessions.len(), 1);
    assert_eq!(registry.sessions[&7].game().save().unwrap().seed, 41);
}

#[test]
fn concurrent_reservations_issue_each_final_handle_once_and_then_reject() {
    let next = AtomicU32::new(u32::MAX - 15);
    let results = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..8)
            .map(|_| {
                scope.spawn(|| {
                    (0..4)
                        .map(|_| reserve_session_handle(&next))
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        workers
            .into_iter()
            .flat_map(|worker| worker.join().unwrap())
            .collect::<Vec<_>>()
    });
    let issued: std::collections::BTreeSet<_> = results
        .iter()
        .filter_map(|result| result.as_ref().ok().copied())
        .collect();
    assert_eq!(issued.len(), 16);
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 16);
    assert_eq!(
        results
            .iter()
            .filter(|result| matches!(result, Err(SessionError::ResourceLimit(_))))
            .count(),
        16
    );
    assert!(!issued.contains(&0));
    assert_eq!(next.load(Ordering::SeqCst), 0);
}

#[test]
fn producer_failure_contains_real_location_and_recovery_details() {
    let error = engine::session::StepFatal::InvariantViolation {
        location: "ReceiptAggregation::validate".into(),
        description: "receipt chain broke".into(),
    };
    let value = serde_json::to_value(HostFailure::step(error)).unwrap();
    assert_eq!(value["where"], "ReceiptAggregation::validate");
    assert_eq!(value["context"]["operation"], "step");
    assert_eq!(value["recoverable"], false);
    assert!(value["recoveryActions"]
        .as_array()
        .is_some_and(|actions| !actions.is_empty()));
    assert!(value["cause"].is_null());
}

#[test]
fn registry_step_returns_one_valid_frame_and_invalid_handle_is_explicit() {
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let handle = REGISTRY
        .with(|registry| registry.borrow_mut().create(setup, 41))
        .unwrap();

    let update = step_update(handle).unwrap();

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
    drop_session(handle);
    assert_eq!(
        step_update(handle).unwrap_err(),
        StepUpdateError::Operation(format!("invalid session handle: {handle}"))
    );
}

#[test]
fn step_fatal_maps_to_the_structured_host_failure_contract() {
    let fatal = engine::session::StepFatal::InvariantViolation {
        location: "web-wasm.step".into(),
        description: "receipt chain broke".into(),
    };
    let expected_message = "引擎不变量校验失败（原始详情已脱敏）";
    let failure = HostFailure::from(fatal);
    assert_eq!(failure.code, "STEP_FATAL");
    assert_eq!(failure.message, expected_message);
    let value = serde_json::to_value(&failure).unwrap();
    assert_eq!(value["code"], "STEP_FATAL");
    assert_eq!(value["message"], expected_message);
    assert_eq!(value["where"], "web-wasm.step");
    assert_eq!(value["cause"], serde_json::Value::Null);
    assert_eq!(value["context"], serde_json::json!({ "operation": "step" }));
    assert_eq!(value["recoverable"], false);
    assert_eq!(
        value["recoveryActions"],
        serde_json::json!([
            "停止当前会话；重新打开上一份有效日终存档或新局",
            "复制脱敏错误详情反馈"
        ])
    );
}

#[test]
fn session_step_fatal_maps_to_the_structured_host_failure_contract() {
    let fatal = engine::session::StepFatal::InvariantViolation {
        location: "web-wasm.civil_day_ready".into(),
        description: "civil clock invariant broke".into(),
    };
    let expected_message = "引擎不变量校验失败（原始详情已脱敏）";

    let StepUpdateError::Fatal(failure) =
        session_error_to_step_update_error(engine::SessionError::Step(fatal))
    else {
        panic!("expected fatal failure")
    };
    assert_eq!(failure.code, "STEP_FATAL");
    assert_eq!(failure.message, expected_message);
    assert_eq!(failure.r#where, "web-wasm.civil_day_ready");
    assert!(failure.cause.is_none());
}

#[test]
fn civil_settlement_error_maps_to_the_cross_host_failure_code() {
    let error = engine::SessionError::InvalidSave("civil boundary is inconsistent".into());
    let StepUpdateError::Fatal(failure) = civil_error_to_step_update_error(error) else {
        panic!("expected fatal failure")
    };
    assert_eq!(failure.code, "CIVIL_DAY_SETTLEMENT_FAILED");
    assert_eq!(
        failure.message,
        "日终存档或协议状态校验失败（原始详情已脱敏）"
    );
    assert_eq!(failure.r#where, "web-wasm.end_civil_day_update");
    assert!(failure.cause.is_none());
}

#[test]
fn registry_retains_complete_closing_history() {
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let handle = REGISTRY
        .with(|registry| registry.borrow_mut().create(setup, 41))
        .unwrap();
    for _ in 0..fixture::TICKS_PER_DAY {
        step_update(handle).unwrap();
    }
    let EngineUpdate::CivilUpdate(civil) = step_update(handle).unwrap() else {
        panic!("completed trading day must publish CivilUpdate before another tick")
    };

    civil.validate().unwrap();
    assert_eq!(
        civil.refresh.intraday.len(),
        usize::try_from(fixture::TICKS_PER_DAY).unwrap()
    );
    println!(
        "wasm registry civil: {}",
        serde_json::to_string(&EngineUpdate::CivilUpdate(civil)).unwrap()
    );
    let EngineUpdate::TickBatch(next) = step_update(handle).unwrap() else {
        panic!("CivilUpdate must be published exactly once before stepping resumes")
    };
    assert_eq!(next.frames[0].tick, fixture::TICKS_PER_DAY + 1);
    drop_session(handle);
}
#[test]
fn producer_failure_preserves_real_source_chain_without_private_state() {
    let error =
        engine::SessionError::Closing(engine::accounting::closing::ClosingError::Accounting(
            engine::accounting::AccountingError::AmountOverflow {
                op: "add",
                detail: "token=private-secret account cash=987654321".into(),
            },
        ));
    let value = serde_json::to_value(HostFailure::civil(error)).unwrap();
    assert_eq!(value["code"], "CIVIL_DAY_SETTLEMENT_FAILED");
    assert!(value["cause"].is_object());
    assert!(value["cause"]["cause"].is_object());
    assert!(value["cause"]["cause"]["cause"].is_null());
    let encoded = value.to_string();
    assert!(!encoded.contains("private-secret"));
    assert!(!encoded.contains("987654321"));
    assert!(!encoded.contains("recovery_actions"));
}
#[test]
fn producer_failure_redacts_real_bare_amount_overflow_operands() {
    let private_operand = 2_718_281_828_459_045_235_i128;
    let unknown = std::io::Error::other(private_operand.to_string());
    assert_eq!(
        failure_description(&unknown),
        ("ERROR_DETAILS_REDACTED", "原始错误类型未识别，详情未公开")
    );
    assert!(failure_cause(&unknown).is_none());
    let overflow = engine::accounting::AccountingAmount::MAX
        .add(engine::accounting::AccountingAmount::from_cents(
            private_operand,
        ))
        .unwrap_err();
    let engine::accounting::AccountingError::AmountOverflow { ref detail, .. } = overflow else {
        panic!("fixture must produce a real amount overflow")
    };
    assert_eq!(detail, &format!("{} + {private_operand}", i128::MAX));
    let error = engine::SessionError::Closing(
        engine::accounting::closing::ClosingError::Accounting(overflow),
    );
    let value = serde_json::to_value(HostFailure::civil(error)).unwrap();
    println!("A08_BARE_AMOUNT_FAILURE={value}");
    let encoded = value.to_string();
    assert!(
        !encoded.contains(&private_operand.to_string()),
        "bare private operand leaked"
    );
    assert!(
        !encoded.contains(&i128::MAX.to_string()),
        "bare private operand leaked"
    );
    assert_eq!(value["cause"]["code"], "CLOSING_ACCOUNTING_FAILED");
    assert_eq!(
        value["cause"]["cause"]["code"],
        "ACCOUNTING_AMOUNT_OVERFLOW"
    );
    assert_eq!(
        value["cause"]["cause"]["message"],
        "公司会计金额运算溢出（操作数已脱敏）"
    );
    assert!(value["cause"]["cause"]["cause"].is_null());
}

#[test]
fn registry_owns_independent_handles_and_failed_construction_does_not_register() {
    let mut registry = SessionRegistry::default();
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-02").unwrap());
    let first = registry.create(setup.clone(), 41).unwrap();
    let second = registry.create(setup.clone(), 42).unwrap();
    assert_ne!(first, second);
    registry.step_update(first).unwrap();
    assert_eq!(
        registry
            .with_session(first, |session| session.tick())
            .unwrap(),
        1
    );
    assert_eq!(
        registry
            .with_session(second, |session| session.tick())
            .unwrap(),
        0
    );
    let count = registry.sessions.len();
    let mut invalid = setup;
    invalid.stocks.clear();
    assert!(registry.create(invalid, 43).is_err());
    assert_eq!(registry.sessions.len(), count);
    registry.remove(u32::MAX);
    assert_eq!(registry.sessions.len(), count);
    registry.remove(first);
    assert!(registry.with_session(first, |_| ()).is_err());
    assert!(registry.step_update(first).is_err());
    assert_eq!(
        registry
            .with_session(second, |session| session.tick())
            .unwrap(),
        0
    );
}

#[test]
fn registry_restore_failure_preserves_existing_session_and_civil_barrier() {
    let setup = fixture::civil_setup(engine::CivilDate::from_iso("2030-01-05").unwrap());
    let mut settled = ProtocolSession::new(setup.clone(), 41).unwrap();
    settled.end_civil_day_update().unwrap();
    let slot = settled.save().unwrap();
    let mut registry = SessionRegistry::default();
    let original = registry.create(setup, 42).unwrap();
    let restored = registry.restore(&slot).unwrap();
    let count = registry.sessions.len();
    let mut invalid = slot.clone();
    invalid.pending_player.push((
        AccountId(0),
        Intent::Cancel {
            code: engine::StockCode("600101".into()),
            id: engine::OrderId(1),
        },
    ));
    assert!(registry.restore(&invalid).is_err());
    assert_eq!(registry.sessions.len(), count);
    assert_eq!(
        registry
            .with_session(original, |session| session.civil_date())
            .unwrap()
            .to_iso(),
        "2030-01-05"
    );
    let EngineUpdate::CivilUpdate(civil) = registry.step_update(restored).unwrap() else {
        panic!("恢复的周日应先发布 CivilUpdate")
    };
    assert_eq!(civil.tick, slot.snapshot.tick);
    assert_eq!(
        registry
            .with_session(restored, |session| session.civil_date())
            .unwrap()
            .to_iso(),
        "2030-01-07"
    );
}

//! 真实 native registry 的首红；不能把本测试当实际 JS export 或浏览器。
use super::*;
#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;
include!("../../../packages/engine/test-support/simple_company.rs");
include!("../../../packages/engine/test-support/host_company_writes.rs");

#[test]
fn proposal_registry_reaches_actual_owner_zero_no_holding_rejection() {
    let mut registry = SessionRegistry::default();
    let handle = registry
        .create(
            company_write_setup(fixture::civil_setup(
                engine::CivilDate::from_iso("2030-06-03").unwrap(),
            )),
            7181,
        )
        .unwrap();
    registry
        .with_session(handle, |session| {
            let proposal = serde_json::from_value(no_holding_cash_proposal()).unwrap();
            let error = propose_company_action(session, proposal).unwrap_err();
            let failure = serde_json::to_value(error.public_failure()).unwrap();
            assert_eq!(
                failure["kind"], "PlayerProposal",
                "WRITE_HOST_PROPOSAL_WASM_STUB：{failure}"
            );
            assert_eq!(failure["data"]["account"], "0");
            assert_eq!(failure["data"]["company"], 0);
            assert_eq!(failure["data"]["kind"], "CashDividend");
            assert_eq!(failure["data"]["day"], "2030-06-03");
            assert_eq!(failure["data"]["classification"], "BusinessCondition");
            assert!(failure["data"]["reason"]
                .as_str()
                .unwrap()
                .contains("总持仓为零"));
        })
        .unwrap();
    registry.remove(handle).unwrap();
}

#[test]
fn correction_registry_enqueue_preserves_payload_and_does_not_publish() {
    let mut registry = SessionRegistry::default();
    let setup = company_write_setup(fixture::civil_setup(
        engine::CivilDate::from_iso("2030-06-03").unwrap(),
    ));
    let exported_setup = serde_json::to_value(&setup).unwrap();
    let handle = registry.create(setup, 7181).unwrap();
    registry.with_session(handle, |session| {
        let query = engine::company::PublicReportQuery { company_id:0, cursor:None, page_size:Some(100) };
        let before = serde_json::to_value(session.query_public_reports(&query).unwrap()).unwrap();
        let request: engine::session::CompanyReportCorrection = serde_json::from_value(host_correction_request(&before)).unwrap();
        println!("COMPANY_WRITE_EXPORT_INPUTS {}", serde_json::json!({
            "setup": exported_setup, "proposal": no_holding_cash_proposal(), "request": request,
        }));
        let ack = enqueue_correction(session, request.clone()).unwrap_or_else(|error| {
            panic!("WRITE_HOST_CORRECTION_ENQUEUE_WASM_STUB：{error}")
        });
        assert_eq!(ack.operation_id, request.operation_id);
        let status = session.game().report_corrections().unwrap();
        assert_eq!(status.pending, vec![request]);
        assert!(status.completed.is_empty());
        assert_eq!(before, serde_json::to_value(session.query_public_reports(&query).unwrap()).unwrap());
    }).unwrap();
    registry.remove(handle).unwrap();
}

#[test]
fn correction_registry_cancel_reaches_not_pending_and_keeps_queue_empty() {
    let mut registry = SessionRegistry::default();
    let handle = registry
        .create(
            company_write_setup(fixture::civil_setup(
                engine::CivilDate::from_iso("2030-06-03").unwrap(),
            )),
            7181,
        )
        .unwrap();
    registry
        .with_session(handle, |session| {
            let error = cancel_correction(session, "absent-correction").unwrap_err();
            let failure = serde_json::to_value(error.public_failure()).unwrap();
            assert_eq!(
                failure["kind"], "ReportCorrection",
                "WRITE_HOST_CORRECTION_CANCEL_WASM_STUB：{failure}"
            );
            assert_eq!(failure["data"]["kind"], "NotPending");
            assert_eq!(failure["data"]["data"]["operation_id"], "absent-correction");
            assert!(session
                .game()
                .report_corrections()
                .unwrap()
                .pending
                .is_empty());
        })
        .unwrap();
    registry.remove(handle).unwrap();
}

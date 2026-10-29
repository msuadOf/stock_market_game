//! 通过实际 Tauri invoke 注册与真实 Session 校验写命令。
use super::command_builder;
use super::lib_tests::{diagnostic_setup, invoke_json};
use serde_json::json;
use tauri::test::{mock_builder, mock_context, noop_assets};
use tauri::WebviewWindowBuilder;
include!("../../../../packages/engine/test-support/simple_company.rs");
include!("../../../../packages/engine/test-support/host_company_writes.rs");
include!(
    "../../../../packages/engine/test-support/host_company_writes/no_holding_cash_proposal.rs"
);

#[tokio::test]
async fn proposal_ipc_reaches_actual_no_holding_domain_rejection() {
    let app = command_builder(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let session_id = invoke_json(
        &webview,
        "create_session",
        json!({"setup":company_write_setup(diagnostic_setup()),"seed":"7181"}),
    )
    .unwrap();
    let failure = invoke_json(
        &webview,
        "propose_player_company_action",
        json!({"sessionId":session_id,"generation":"1","proposal":no_holding_cash_proposal()}),
    )
    .expect_err("零持仓的真实领域命令应拒绝");
    assert_eq!(
        failure["kind"], "PlayerProposal",
        "WRITE_HOST_PROPOSAL_IPC_REGISTER：{failure}"
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
    invoke_json(&webview, "stop_session", json!({"sessionId":session_id})).unwrap();
}

#[tokio::test]
async fn correction_enqueue_ipc_acknowledges_queue_without_publishing() {
    let app = command_builder(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let session_id = invoke_json(
        &webview,
        "create_session",
        json!({"setup":company_write_setup(diagnostic_setup()),"seed":"7181"}),
    )
    .unwrap();
    let query = json!({"company_id":0,"cursor":null,"page_size":100});
    let before = invoke_json(
        &webview,
        "public_reports",
        json!({"sessionId":session_id,"generation":"1","query":query}),
    )
    .unwrap();
    let request = host_correction_request(&before["value"]);
    let receipt = invoke_json(
        &webview,
        "enqueue_report_correction",
        json!({"sessionId":session_id,"generation":"1","request":request}),
    )
    .unwrap_or_else(|error| panic!("WRITE_HOST_CORRECTION_ENQUEUE_IPC_REGISTER：{error}"));
    assert_eq!(receipt["generation"], "1");
    assert_eq!(receipt["operation_id"], request["operation_id"]);
    let after = invoke_json(
        &webview,
        "public_reports",
        json!({"sessionId":session_id,"generation":"1","query":query}),
    )
    .unwrap();
    assert_eq!(before, after, "入队不能提前发布报告或冒充更正完成");
    let cancel =
        json!({"sessionId":session_id,"generation":"1","operationId":request["operation_id"]});
    let cancelled = invoke_json(&webview, "cancel_report_correction", cancel.clone()).unwrap();
    assert_eq!(cancelled["operation_id"], request["operation_id"]);
    let repeated = invoke_json(&webview, "cancel_report_correction", cancel).unwrap_err();
    assert_eq!(repeated["kind"], "ReportCorrection");
    assert_eq!(
        repeated["data"]["kind"], "NotPending",
        "第二次取消不能伪造成功：{repeated}"
    );
    invoke_json(&webview, "stop_session", json!({"sessionId":session_id})).unwrap();
}

#[tokio::test]
async fn correction_cancel_ipc_reaches_actual_not_pending_rejection() {
    let app = command_builder(mock_builder())
        .build(mock_context(noop_assets()))
        .unwrap();
    let webview = WebviewWindowBuilder::new(&app, "main", Default::default())
        .build()
        .unwrap();
    let session_id = invoke_json(
        &webview,
        "create_session",
        json!({"setup":company_write_setup(diagnostic_setup()),"seed":"7181"}),
    )
    .unwrap();
    let failure = invoke_json(
        &webview,
        "cancel_report_correction",
        json!({"sessionId":session_id,"generation":"1","operationId":"absent-correction"}),
    )
    .expect_err("不存在的更正应由真实队列拒绝");
    assert_eq!(
        failure["kind"], "ReportCorrection",
        "WRITE_HOST_CORRECTION_CANCEL_IPC_REGISTER：{failure}"
    );
    assert_eq!(failure["data"]["kind"], "NotPending");
    assert_eq!(failure["data"]["data"]["operation_id"], "absent-correction");
    invoke_json(&webview, "stop_session", json!({"sessionId":session_id})).unwrap();
}

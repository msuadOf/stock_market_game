//! 真实 router 缺注册的行为首红；不以 parser 或 mock 成功替代写命令。
use super::auth_tests::{request, setup};
use axum::http::StatusCode;
use serde_json::{json, Value};
include!("../../../../packages/engine/test-support/simple_company.rs");
include!("../../../../packages/engine/test-support/host_company_writes.rs");

async fn company_market() -> (axum::Router, Value, String) {
    let app = crate::app_router();
    let setup = company_write_setup(serde_json::from_value(setup()).unwrap());
    let (status, created) = request(
        app.clone(),
        "POST",
        "/api/new",
        Some(json!({"setup":setup,"seed":"7181"})),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "真实新局夹具失败：{created}");
    let credential = created["test_credential"].as_str().unwrap().to_owned();
    (app, created, credential)
}

#[tokio::test]
async fn proposal_http_reaches_actual_no_holding_domain_rejection() {
    let (app, created, credential) = company_market().await;
    let (status, failure) = request(
        app,
        "POST",
        "/api/propose-player-company-action",
        Some(
            json!({"session_id":created["session_id"],"generation":created["generation"],
            "proposal":no_holding_cash_proposal()}),
        ),
        Some(&credential),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "WRITE_HOST_PROPOSAL_HTTP_REGISTER：{failure}"
    );
    assert_eq!(failure["code"], "PLAYER_COMPANY_ACTION_REJECTED");
    assert_eq!(failure["failure"]["kind"], "PlayerProposal");
    assert_eq!(failure["failure"]["data"]["account"], "0");
    assert_eq!(failure["failure"]["data"]["company"], 0);
    assert_eq!(failure["failure"]["data"]["kind"], "CashDividend");
    assert_eq!(failure["failure"]["data"]["day"], "2030-06-03");
    assert_eq!(
        failure["failure"]["data"]["classification"],
        "BusinessCondition"
    );
    assert!(failure["failure"]["data"]["reason"]
        .as_str()
        .unwrap()
        .contains("总持仓为零"));
}

#[tokio::test]
async fn correction_enqueue_http_acknowledges_queue_without_publishing() {
    let (app, created, credential) = company_market().await;
    let reports_url = format!(
        "/api/companies/0/reports?session_id={}&limit=100",
        created["session_id"].as_str().unwrap()
    );
    let (status, before) = request(app.clone(), "GET", &reports_url, None, Some(&credential)).await;
    assert_eq!(status, StatusCode::OK, "真实报告夹具失败：{before}");
    let request_body = host_correction_request(&before);
    let (status, receipt) = request(
        app.clone(),
        "POST",
        "/api/enqueue-report-correction",
        Some(
            json!({"session_id":created["session_id"],"generation":created["generation"],
            "request":request_body}),
        ),
        Some(&credential),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "WRITE_HOST_CORRECTION_ENQUEUE_HTTP_REGISTER：{receipt}"
    );
    assert_eq!(receipt["generation"], created["generation"]);
    assert_eq!(receipt["operation_id"], request_body["operation_id"]);
    let (status, after) = request(app.clone(), "GET", &reports_url, None, Some(&credential)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(before, after, "入队不能提前发布报告或冒充更正完成");
    let cancel = json!({"session_id":created["session_id"],"generation":created["generation"],"operation_id":request_body["operation_id"]});
    let (status, cancelled) = request(
        app.clone(),
        "POST",
        "/api/cancel-report-correction",
        Some(cancel.clone()),
        Some(&credential),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "实际 pending 命令必须可取消：{cancelled}"
    );
    assert_eq!(cancelled["operation_id"], request_body["operation_id"]);
    let (status, repeated) = request(
        app,
        "POST",
        "/api/cancel-report-correction",
        Some(cancel),
        Some(&credential),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        repeated["failure"]["data"]["kind"], "NotPending",
        "第二次取消不能伪造成功：{repeated}"
    );
}

#[tokio::test]
async fn correction_cancel_http_reaches_actual_not_pending_rejection() {
    let (app, created, credential) = company_market().await;
    let (status, failure) = request(
        app,
        "POST",
        "/api/cancel-report-correction",
        Some(
            json!({"session_id":created["session_id"],"generation":created["generation"],
            "operation_id":"absent-correction"}),
        ),
        Some(&credential),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "WRITE_HOST_CORRECTION_CANCEL_HTTP_REGISTER：{failure}"
    );
    assert_eq!(failure["code"], "REPORT_CORRECTION_REJECTED");
    assert_eq!(failure["failure"]["kind"], "ReportCorrection");
    assert_eq!(failure["failure"]["data"]["kind"], "NotPending");
    assert_eq!(
        failure["failure"]["data"]["data"]["operation_id"],
        "absent-correction"
    );
}

#[tokio::test]
async fn correction_enqueue_http_accepts_controller_without_financial_membership() {
    let manager = crate::actor::SessionManager::default();
    let identities = crate::identity::IdentityService::new(manager.database());
    let creator = identities.guest().unwrap();
    let controller = identities.guest().unwrap();
    let creator_subject = engine::session::OpaqueSubjectId::new(creator.subject.subject_id).unwrap();
    let controller_subject =
        engine::session::OpaqueSubjectId::new(controller.subject.subject_id).unwrap();
    manager
        .database()
        .grant_market_control("shared-market", controller_subject.as_str())
        .unwrap();
    let (session_id, created) = manager
        .new_shared_session(
            company_write_setup(serde_json::from_value(setup()).unwrap()),
            7181,
            creator_subject,
        )
        .unwrap();
    assert!(created, "首个共享市场应由本测试创建");
    let handles = manager.lookup(&session_id).unwrap();
    assert!(handles
        .require_control(controller_subject.clone())
        .await
        .is_ok());
    assert!(handles
        .resolve_member(controller_subject.clone())
        .await
        .is_err());
    let generation = handles
        .market_context_for(controller_subject)
        .await
        .unwrap()
        .generation;
    let reports = handles
        .public_report_page(engine::company::PublicReportQuery {
            company_id: 0,
            cursor: None,
            page_size: Some(100),
        })
        .await
        .unwrap();
    let reports = serde_json::to_value(reports).unwrap();
    let correction = host_correction_request(&reports);
    let operation_id = correction["operation_id"].as_str().unwrap().to_owned();
    let app = crate::app_router_with_manager(manager.clone());
    let (status, receipt) = request(
        app,
        "POST",
        "/api/enqueue-report-correction",
        Some(json!({"session_id":session_id,"generation":generation,"request":correction})),
        Some(&controller.token),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "COMPANY_WRITE_CORRECTION_CONTROLLER_ENQUEUE：{receipt}"
    );
    assert_eq!(receipt["generation"], generation);
    assert_eq!(receipt["operation_id"], operation_id);
    handles.shutdown().await.unwrap();
}

#[tokio::test]
async fn correction_cancel_http_accepts_controller_without_financial_membership() {
    let manager = crate::actor::SessionManager::default();
    let identities = crate::identity::IdentityService::new(manager.database());
    let creator = identities.guest().unwrap();
    let controller = identities.guest().unwrap();
    let creator_subject = engine::session::OpaqueSubjectId::new(creator.subject.subject_id).unwrap();
    let controller_subject =
        engine::session::OpaqueSubjectId::new(controller.subject.subject_id).unwrap();
    manager
        .database()
        .grant_market_control("shared-market", controller_subject.as_str())
        .unwrap();
    let (session_id, created) = manager
        .new_shared_session(
            company_write_setup(serde_json::from_value(setup()).unwrap()),
            7181,
            creator_subject,
        )
        .unwrap();
    assert!(created, "首个共享市场应由本测试创建");
    let handles = manager.lookup(&session_id).unwrap();
    assert!(handles
        .require_control(controller_subject.clone())
        .await
        .is_ok());
    assert!(handles
        .resolve_member(controller_subject.clone())
        .await
        .is_err());
    let generation = handles
        .market_context_for(controller_subject.clone())
        .await
        .unwrap()
        .generation
        .parse::<u64>()
        .unwrap();
    let reports = handles
        .public_report_page(engine::company::PublicReportQuery {
            company_id: 0,
            cursor: None,
            page_size: Some(100),
        })
        .await
        .unwrap();
    let correction = host_correction_request(&serde_json::to_value(reports).unwrap());
    let operation_id = correction["operation_id"].as_str().unwrap().to_owned();
    let pending = handles
        .member_request(
            controller_subject,
            crate::actor::MemberRequest::EnqueueReportCorrection {
                generation,
                request: serde_json::from_value(correction).unwrap(),
            },
        )
        .await
        .unwrap();
    match pending {
        crate::actor::MemberResponse::ReportCorrection(_, Ok(acknowledgement)) => {
            assert_eq!(acknowledgement.operation_id, operation_id);
        }
        crate::actor::MemberResponse::ReportCorrection(_, Err(error)) => {
            panic!("真实 pending 更正建立失败：{error}");
        }
        _ => panic!("建立 pending 更正返回了错误响应类型"),
    }
    let app = crate::app_router_with_manager(manager.clone());
    let cancel = json!({"session_id":session_id,"generation":generation.to_string(),"operation_id":operation_id});
    let (status, receipt) = request(
        app.clone(),
        "POST",
        "/api/cancel-report-correction",
        Some(cancel.clone()),
        Some(&controller.token),
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "COMPANY_WRITE_CORRECTION_CONTROLLER_CANCEL：{receipt}"
    );
    assert_eq!(receipt["operation_id"], operation_id);
    let (status, repeated) = request(
        app,
        "POST",
        "/api/cancel-report-correction",
        Some(cancel),
        Some(&controller.token),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(repeated["failure"]["data"]["kind"], "NotPending");
    handles.shutdown().await.unwrap();
}

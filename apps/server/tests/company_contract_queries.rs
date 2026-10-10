//! 公司共同契约查询面（F 批四视图）的 Server 宿主 REST 暴露（任务 178）。
//!
//! 同一份公司域矩阵 fixture 经 `/api/load` 进入 actor 会话后，四个查询路由
//! 必须返回与 engine 直连查询深等的结果（owner 隔离按请求成员账户）；
//! 拒绝路径显式分类：过期 generation 409、未知公司/非简税模式 400、
//! 无凭据 401、非成员 403。路由缺失时本文件先红（404/405）。

#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use axum::body::to_bytes;
use axum::http::{Request, StatusCode};
use engine::company::CompanyId;
use engine::session::protocol::ProtocolSession;
use serde_json::{json, Value};
use server::{app_router_with_manager, SessionManager};
use tower::ServiceExt;

fn fixture_value() -> Value {
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../apps/web/src/save/fixtures/company-domain-host-matrix-save.json"),
    )
    .expect("矩阵 fixture 必须存在（先跑 company_save_contract 生成）");
    serde_json::from_str(&text).expect("矩阵 fixture 必须是合法 JSON")
}

/// 与矩阵 fixture 时钟兼容（12 tick/日）的最小 setup；读档后整会话被替换。
fn matrix_clock_setup_json() -> Value {
    let mut setup =
        fixture::civil_setup(engine::calendar::CivilDate::from_iso("2030-01-01").unwrap());
    setup.ticks_per_day = 12;
    let setup = serde_json::to_value(&setup).expect("setup 必须可序列化");
    setup
}

async fn guest_router() -> (axum::Router, String) {
    let manager = SessionManager::default();
    let identity = server::identity::IdentityService::new(manager.database())
        .guest()
        .expect("guest identity should be created");
    (app_router_with_manager(manager), identity.token)
}

async fn call(
    app: &axum::Router,
    method: &str,
    uri: &str,
    token: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut builder = Request::builder()
        .method(method)
        .uri(uri)
        .header("authorization", format!("Bearer {token}"));
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    let request = builder
        .body(axum::body::Body::from(
            body.map(|value| value.to_string()).unwrap_or_default(),
        ))
        .unwrap();
    let response = app
        .clone()
        .oneshot(request)
        .await
        .expect("请求必须返回响应");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 64 << 20)
        .await
        .expect("读取 body 失败");
    let value = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    (status, value)
}

/// 创建会话并载入矩阵 fixture；返回 (session_id, 载入后 generation, 成员账户)。
async fn session_with_matrix_fixture() -> (axum::Router, String, String, String, Value) {
    let (app, token) = guest_router().await;
    let (status, body) = call(
        &app,
        "POST",
        "/api/new",
        &token,
        Some(json!({ "setup": matrix_clock_setup_json(), "seed": "42" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "会话创建必须成功：{body}");
    let session_id = body["session_id"].as_str().expect("session_id").to_owned();
    let generation = body["generation"].as_str().expect("generation").to_owned();

    let (status, body) = call(
        &app,
        "POST",
        "/api/load",
        &token,
        Some(json!({
            "session_id": session_id,
            "generation": generation,
            "slot": fixture_value(),
        })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "载入矩阵 fixture 必须成功：{body}");

    // restore 原子替换市场成员关系：按真实客户端流程重新加入市场取得本人账户。
    let (status, context) = call(
        &app,
        "GET",
        &format!("/api/market/context?session_id={session_id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "读上下文必须成功：{context}");
    let next_generation = context["generation"]
        .as_str()
        .expect("换代后 generation")
        .to_owned();
    let (status, body) = call(
        &app,
        "POST",
        "/api/markets/join",
        &token,
        Some(json!({ "session_id": session_id, "generation": next_generation, "confirmed_rejoin": true })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "重入市场必须成功：{body}");

    let (status, context) = call(
        &app,
        "GET",
        &format!("/api/market/context?session_id={session_id}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "读上下文必须成功：{context}");
    let next_generation = context["generation"]
        .as_str()
        .expect("join 后 generation")
        .to_owned();
    let member = context["member"].clone();
    assert!(member.is_object(), "join 后必须有成员关系：{context}");
    (app, token, session_id, next_generation, member)
}

#[tokio::test]
async fn company_capabilities_route_matches_engine_direct_query_on_loaded_fixture() {
    let (app, token, session_id, generation, _member) = session_with_matrix_fixture().await;

    let (status, body) = call(
        &app,
        "GET",
        &format!(
            "/api/companies/C-600101/capabilities?session_id={session_id}&generation={generation}"
        ),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "能力面查询必须成功：{body}");
    let keys: Vec<&str> = body
        .as_object()
        .expect("响应必须是对象")
        .keys()
        .map(String::as_str)
        .collect();
    assert_eq!(
        keys,
        vec!["capabilities", "generation"],
        "响应字段必须恰为 generation+capabilities"
    );

    // engine 直连深等基准：fixture 无配股方案 → owner_rights 对任意账户恒为空
    // （先对两个 fixture 账户证明该不变量），故 REST（重入成员账户）与 engine
    // 直连（fixture 账户 0）的全量 wire 深等成立。
    let slot: engine::SaveSlot = serde_json::from_value(fixture_value()).expect("fixture 反序列化");
    let engine_session = ProtocolSession::restore(&slot).expect("engine 直连恢复");
    for probe in [0_u64, 1] {
        let probe_caps = engine_session
            .game()
            .company_capabilities(&CompanyId("C-600101".into()), engine::AccountId(probe))
            .expect("fixture 账户能力面查询");
        assert!(
            probe_caps.owner_rights.is_empty(),
            "fixture 无配股方案，任意账户 owner_rights 必须为空"
        );
    }
    let direct = engine_session
        .game()
        .company_capabilities(&CompanyId("C-600101".into()), engine::AccountId(0))
        .expect("engine 直连能力面查询");
    let direct_wire = serde_json::to_value(&direct).expect("engine 直连序列化");
    assert_eq!(
        body["capabilities"], direct_wire,
        "REST 能力面必须与 engine 直连查询深等（同一契约）"
    );
    // 公司域内容真实在场：进行中分红方案来自 fixture 的公告中方案。
    let plans = &body["capabilities"]["active_plans"];
    assert!(
        plans.as_array().is_some_and(|plans| plans
            .iter()
            .any(|plan| plan["kind"] == json!("CashDividend"))),
        "能力面必须包含公告中的现金分红方案：{plans}"
    );
}

#[tokio::test]
async fn company_period_explanation_route_returns_settled_period() {
    let (app, token, session_id, generation, _member) = session_with_matrix_fixture().await;
    let (status, body) = call(
        &app,
        "GET",
        &format!(
            "/api/companies/C-600101/period-explanation?session_id={session_id}&generation={generation}&period_end=2029-12-31"
        ),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "期间解释查询必须成功：{body}");
    assert_eq!(
        body["explanation"]["cycle"],
        json!("Monthly"),
        "fixture 为月度结算周期"
    );

    let slot: engine::SaveSlot = serde_json::from_value(fixture_value()).expect("fixture 反序列化");
    let direct = ProtocolSession::restore(&slot)
        .expect("engine 直连恢复")
        .game()
        .company_period_explanation(
            &CompanyId("C-600101".into()),
            engine::calendar::CivilDate::from_iso("2029-12-31").unwrap(),
        )
        .expect("engine 直连期间解释");
    assert_eq!(
        body["explanation"],
        serde_json::to_value(&direct).unwrap(),
        "REST 期间解释必须与 engine 直连查询深等"
    );
}

#[tokio::test]
async fn owner_rights_and_flat_receipt_routes_return_owner_isolated_views() {
    let (app, token, session_id, generation, _member) = session_with_matrix_fixture().await;
    let (status, body) = call(
        &app,
        "GET",
        &format!("/api/owner/rights-offerings?session_id={session_id}&generation={generation}"),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "本人配股权益查询必须成功：{body}");
    assert_eq!(
        body["offerings"],
        json!([]),
        "fixture 无配股方案时为空数组（不缺失）"
    );

    let (status, body) = call(
        &app,
        "GET",
        &format!(
            "/api/owner/flat-withholding-receipts?session_id={session_id}&generation={generation}"
        ),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "简税代扣回执查询必须成功：{body}");
    assert_eq!(
        body["receipts"],
        json!([]),
        "owner 隔离：成员账户（玩家）无名册持仓，回执必须为空数组"
    );
}

#[tokio::test]
async fn company_query_routes_reject_stale_generation_unknown_company_and_non_flat_mode() {
    let (app, token, session_id, _generation, _member) = session_with_matrix_fixture().await;
    let (status, body) = call(
        &app,
        "GET",
        &format!("/api/companies/C-600101/capabilities?session_id={session_id}&generation=99999"),
        &token,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::CONFLICT,
        "过期 generation 必须 409：{body}"
    );
    assert_eq!(body["code"], json!("STALE_SESSION_GENERATION"));

    let (status, body) = call(
        &app,
        "GET",
        &format!(
            "/api/companies/不存在公司/capabilities?session_id={session_id}&generation={}",
            refreshed_generation(&app, &token, &session_id).await
        ),
        &token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "未知公司必须 400：{body}");
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("未知公司")),
        "错误信息必须指明未知公司：{body}"
    );

    // 非 Flat 模式：简税回执概念不适用，必须显式拒绝而非空台账。
    let (app2, token2) = guest_router().await;
    let mut setup =
        fixture::civil_setup(engine::calendar::CivilDate::from_iso("2030-01-01").unwrap());
    setup.ticks_per_day = 12;
    setup.dividend_tax_mode =
        engine::company::cash_dividend_tax::CashDividendTaxMode::AShareIndividual;
    // 模式配对校验：非简税模式不得携带简税比例。
    setup.flat_withholding_bp = None;
    let (status, body) = call(
        &app2,
        "POST",
        "/api/new",
        &token2,
        Some(json!({ "setup": serde_json::to_value(&setup).unwrap(), "seed": "42" })),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "非简税会话创建必须成功：{body}");
    let session2 = body["session_id"].as_str().expect("session_id").to_owned();
    let generation2 = body["generation"].as_str().expect("generation").to_owned();
    let (status, body) = call(
        &app2,
        "GET",
        &format!(
            "/api/owner/flat-withholding-receipts?session_id={session2}&generation={generation2}"
        ),
        &token2,
        None,
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "非简税模式必须 400：{body}"
    );
    assert!(
        body["message"]
            .as_str()
            .is_some_and(|message| message.contains("FlatWithholding")),
        "错误信息必须指明模式不适用：{body}"
    );
}

#[tokio::test]
async fn company_query_routes_require_credentials_and_membership() {
    let (app, _token, session_id, generation, _member) = session_with_matrix_fixture().await;
    let request = Request::builder()
        .method("GET")
        .uri(format!(
            "/api/companies/C-600101/capabilities?session_id={session_id}&generation={generation}"
        ))
        .body(axum::body::Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::UNAUTHORIZED,
        "缺凭据必须 401"
    );

    // 有身份但非本市场成员：不得读取他人市场的公司查询面。
    let guest = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/auth/guest")
                .header("content-type", "application/json")
                .body(axum::body::Body::from("{}"))
                .unwrap(),
        )
        .await
        .expect("guest request must return a response");
    assert_eq!(guest.status(), StatusCode::CREATED);
    let outsider_token =
        serde_json::from_slice::<Value>(&to_bytes(guest.into_body(), 1 << 20).await.unwrap())
            .unwrap()["token"]
            .as_str()
            .expect("guest token")
            .to_owned();
    let (status, body) = call(
        &app,
        "GET",
        &format!(
            "/api/companies/C-600101/capabilities?session_id={session_id}&generation={generation}"
        ),
        &outsider_token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN, "非成员必须 403：{body}");
}

/// 未知公司负例需要当前有效 generation（上一断言的 409 不消耗换代）。
async fn refreshed_generation(app: &axum::Router, token: &str, session_id: &str) -> String {
    let (status, context) = call(
        app,
        "GET",
        &format!("/api/market/context?session_id={session_id}"),
        token,
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK, "读上下文必须成功：{context}");
    context["generation"]
        .as_str()
        .expect("generation")
        .to_owned()
}

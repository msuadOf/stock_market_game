//! 公司共同契约查询面（F 批四视图）的 Desktop 宿主桥接测试（任务 178）。
//!
//! 同一份公司域矩阵 fixture 经 engine 直连恢复后，Desktop actor 的四个查询
//! 命令必须返回与 engine 直连查询深等的 JSON wire（owner 固定本机玩家
//! `AccountId(0)`，与 web-wasm 导出同口径）；存档经 desktop 共用 native-store
//! SQLite 往返后与 fixture 深等。命令缺失时本文件先红（编译期）。

use super::*;

#[tokio::test]
async fn desktop_company_query_commands_return_engine_wire() {
    let slot: engine::SaveSlot = serde_json::from_value(matrix_fixture()).unwrap();
    let game = ProtocolSession::restore(&slot).unwrap();
    let mut harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "company-queries",
        "company-queries-timeline",
    );
    let actor = &mut harness.actor;
    let company = engine::company::CompanyId("C-600101".into());

    // 能力面：命令回包 wire == engine 直连查询 wire（同一会话、同一 owner）。
    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::CompanyCapabilities {
            generation: 1,
            company: company.clone(),
            reply,
        })
        .await;
    let response = received.await.unwrap().expect("能力面命令必须成功");
    assert_eq!(response.generation, "1");
    let direct = serde_json::to_value(
        actor
            .game
            .game()
            .company_capabilities(&company, AccountId(0))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        response.value, direct,
        "Desktop 能力面命令必须与 engine 直连深等"
    );
    // 公司域内容在场：进行中分红方案来自 fixture 公告中的方案。
    assert!(direct["active_plans"]
        .as_array()
        .expect("active_plans 必须是数组")
        .iter()
        .any(|plan| plan["kind"] == serde_json::json!("CashDividend")));

    // 期间解释：已结算期间可读，wire 深等。
    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::CompanyPeriodExplanation {
            generation: 1,
            company: company.clone(),
            period_end: engine::CivilDate::from_iso("2029-12-31").unwrap(),
            reply,
        })
        .await;
    let response = received.await.unwrap().expect("期间解释命令必须成功");
    let direct = serde_json::to_value(
        actor
            .game
            .game()
            .company_period_explanation(
                &company,
                engine::CivilDate::from_iso("2029-12-31").unwrap(),
            )
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        response.value, direct,
        "Desktop 期间解释命令必须与 engine 直连深等"
    );
    assert_eq!(direct["cycle"], serde_json::json!("Monthly"));

    // 本人配股权益：fixture 无配股方案 → 空数组（不缺失），wire 深等。
    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::OwnerRightsOfferings {
            generation: 1,
            reply,
        })
        .await;
    let response = received.await.unwrap().expect("本人配股权益命令必须成功");
    let direct = serde_json::to_value(
        actor
            .game
            .game()
            .owner_rights_offerings(AccountId(0))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        response.value, direct,
        "Desktop 本人配股权益命令必须与 engine 直连深等"
    );
    assert_eq!(
        direct,
        serde_json::json!([]),
        "fixture 无配股方案时为空数组"
    );

    // 简税代扣回执：owner 隔离（玩家无名册持仓 → 空），wire 深等。
    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::OwnerFlatWithholdingReceipts {
            generation: 1,
            reply,
        })
        .await;
    let response = received.await.unwrap().expect("简税回执命令必须成功");
    let direct = serde_json::to_value(
        actor
            .game
            .game()
            .owner_flat_withholding_receipts(AccountId(0))
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        response.value, direct,
        "Desktop 简税回执命令必须与 engine 直连深等"
    );

    // 过期 generation：显式拒绝，不静默返回旧数据。
    let (reply, received) = oneshot::channel();
    actor
        .handle_command(SessionCommand::CompanyCapabilities {
            generation: 99,
            company,
            reply,
        })
        .await;
    let error = received
        .await
        .unwrap()
        .expect_err("过期 generation 必须显式拒绝");
    assert!(
        error.to_string().contains("stale session generation"),
        "拒绝信息必须指明过期 generation：{error}"
    );
}

#[tokio::test]
async fn desktop_matrix_fixture_roundtrips_through_native_archive() {
    let fixture = matrix_fixture();
    let slot: engine::SaveSlot = serde_json::from_value(fixture.clone()).unwrap();
    let game = ProtocolSession::restore(&slot).unwrap();
    let harness = ActorHarness::new_protocol_actor(
        game,
        false,
        PausePreferences::default(),
        "company-matrix",
        "company-matrix-timeline",
    );
    // actor 当前权威存档与 fixture 深等（Desktop 路径 restore→save）。
    assert_eq!(
        serde_json::to_value(harness.actor.game.save().unwrap()).unwrap(),
        fixture,
        "Desktop actor restore→save 必须与 fixture 深等"
    );
    // 经 desktop 共用 SQLite 介质往返仍深等。
    let key = engine::session::protocol::SaveCandidateKey {
        seq: slot.snapshot.seq,
        settled_date: slot
            .civil_clock
            .settled_through
            .expect("fixture 必须携带 settled_through"),
    };
    let candidate = native_store::DayEndCandidate::capture(&harness.actor.game, &key).unwrap();
    let database = native_store::NativeDatabase::open_in_memory().unwrap();
    native_store::ArchiveWriter::activate(database.clone(), "matrix")
        .unwrap()
        .save_day_end(&candidate)
        .unwrap();
    assert_eq!(
        serde_json::to_value(database.load("matrix").unwrap().unwrap()).unwrap(),
        fixture,
        "Desktop native-store SQLite 往返必须与 fixture 深等"
    );
}

fn matrix_fixture() -> serde_json::Value {
    // 与 server 侧矩阵测试共用同一 fixture 文件（apps/web 路径为跨宿主单一来源）。
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../apps/web/src/save/fixtures/company-domain-host-matrix-save.json"),
    )
    .unwrap_or_else(|error| panic!("读取公司域矩阵 fixture 失败：{error}"));
    serde_json::from_str(&text).expect("矩阵 fixture 必须是合法 JSON")
}

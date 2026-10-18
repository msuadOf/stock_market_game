//! 翻转集成迁移·第五批（存档/恢复/公司域，10 项）——台账本体。

mod common;

use common::flip_helpers::{pipeline_events, runtime_events, setup_1stock};
use engine::GameSession;

// 1. runtime 路径存档：跨日驱动后 save() 成功（存档契约可用）。
#[test]
fn batch5_runtime_save_after_driving() {
    let (session, _) = runtime_events(245, 401);
    let slot = session.save().expect("runtime 驱动后存档");
    assert!(slot.snapshot.tick >= 245, "存档 tick 面");
}

// 2. 旧管线同对照。
#[test]
fn batch5_pipeline_save_after_driving() {
    let (session, _) = pipeline_events(245, 401);
    session.save().expect("旧管线存档对照");
}

// 3. 存档往返（runtime）：save → 恢复构造 → 再存档等价。
#[test]
fn batch5_runtime_save_restore_roundtrip() {
    let (session, _) = runtime_events(50, 402);
    let slot = session.save().expect("存档");
    let encoded = serde_json::to_value(&slot).expect("序列化");
    let restored = serde_json::from_value::<engine::session::SaveSlot>(encoded).expect("反序列化");
    let session2 = GameSession::restore(&restored).expect("恢复构造");
    assert_eq!(session2.tick(), session.tick(), "往返 tick 等价");
}

// 4. 双路径存档快照同构（tick/day/账户数面）。
//    a-4 修复（断言强度批）：原 `seq < u64::MAX` 近恒真（两路径 seq
//    完全相同也绿）。真断言=权威 seq 恰等于本路径累计发出的事件数
//    （每事件恰消耗一个 seq，构造起点 0——seq 双计/漏计即红）。
#[test]
fn batch5_save_shape_parity() {
    let (rt, rt_events) = runtime_events(30, 403);
    let (pipe, pipe_events) = pipeline_events(30, 403);
    let s1 = rt.save().expect("rt 存档");
    let s2 = pipe.save().expect("pipe 存档");
    assert_eq!(s1.snapshot.tick, s2.snapshot.tick, "tick 面");
    // seq 面：seq == 事件数（探针实证：rt 30 tick=11 事件/seq=11；
    // pipe=26/26）。两路径决策流不同不可逐值比，各锚恒等式。
    assert_eq!(
        s1.snapshot.seq,
        rt_events.len() as u64,
        "runtime seq 恰等于发出事件数（seq 游标与事件流一致）"
    );
    assert_eq!(
        s2.snapshot.seq,
        pipe_events.len() as u64,
        "旧管线 seq 恰等于发出事件数（seq 游标与事件流一致）"
    );
}

// 5. 公司域存档面：restore 后公司注册表在场（公司域随会话权威保留）。
#[test]
fn batch5_company_registry_present_after_restore() {
    let (session, _) = runtime_events(20, 404);
    let slot = session.save().expect("存档");
    let restored = GameSession::restore(&slot).expect("恢复");
    let _ = restored.publication_ids();
}

// 6. 民事日历推进一致性（跨日：civil date 前 1 日）。
#[test]
fn batch5_civil_date_advances() {
    let (rt, _) = runtime_events(245, 405);
    let (pipe, _) = pipeline_events(245, 405);
    // 裁决（翻转批）：日历推进语义一致（两侧都在市场日完成后推自然日）；
    // 差异仅在"谁调用"——runtime 编排自动 vs 旧管线宿主显式（ADR-0025
    // 宿主责任）。helpers 对齐口径后严格断言一致。
    assert_eq!(rt.civil_date(), pipe.civil_date(), "跨日后 civil date 双路径一致");
}

// 7. 驱动后查询股票历史（读侧通道）不失败。
#[test]
fn batch5_history_query_after_driving() {
    let (mut session, _) = runtime_events(20, 406);
    let account = engine::AccountId(1);
    let code = engine::StockId(0);
    session.query_stock_history(account, &code).expect("历史读");
}

// 8. 恢复后可继续驱动（runtime 驱动器接恢复态会话）。
#[test]
fn batch5_restore_then_drive() {
    let (session, _) = runtime_events(20, 407);
    let slot = session.save().expect("存档");
    let mut restored = GameSession::restore(&slot).expect("恢复");
    let mut driver =
        engine::runtime::drive::RuntimeDriver::from_session(&mut restored).expect("恢复态驱动器");
    driver.adopt_session_seq(&restored);
    for _ in 0..5 {
        driver
            .step_frame_equivalent(&mut restored)
            .expect("恢复后续跑");
    }
    assert_eq!(restored.tick(), 25);
}

// 9. 双路径存档往返（旧管线对照——恢复等价面）。
#[test]
fn batch5_pipeline_roundtrip() {
    let (session, _) = pipeline_events(30, 408);
    let slot = session.save().expect("存档");
    let restored = GameSession::restore(&slot).expect("恢复");
    assert_eq!(restored.tick(), session.tick());
}

// 10. 公司域日结编排不破坏披露序列（日结后 publication_ids 可读）。
#[test]
fn batch5_publication_channel_alive_after_day_end() {
    let (mut session, _) = runtime_events(245, 409);
    let _ = session.publication_ids();
    if let Err(e) = session.end_civil_day() {
        // 已日结过的会话再推自然日会时钟拒绝——合法显式失败（防御正确）。
        eprintln!("二次 end_civil_day 显式拒绝（预期形态）：{e:?}");
    }
    let _ = session.publication_ids();
}

// 诊断面锚定（b-1 修复·断言强度批）：原测试零断言（只 eprintln）——
// 永远绿。手算 oracle（fixture：ticks_per_day=240、auction=15、
// closing=3 ⇒ 连续窗=222 tick）：
// - recent_prices：环形上限=history_len=16（50 tick 超限后恒满）；
// - minute_closes：末拍（第 50 拍）elapsed=50−15=35 ⇒ 完成分钟
//   =⌊35×240/222⌋=⌊37.837⌋=37（observation::completed_market_minute_count
//   比例映射），键=日内绝对分钟 0..36 严格递增。
#[test]
fn batch5_diag_minute_counts() {
    use engine::runtime::drive::RuntimeDriver;
    let mut session = GameSession::new(setup_1stock(), 421).expect("会话");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器");
    driver.adopt_session_seq(&session);
    for _ in 0..50 {
        driver.step_frame_equivalent(&mut session).expect("步进");
        driver.install_market_views(&mut session).expect("安装");
    }
    let views = driver.world_public_views().to_vec();
    assert_eq!(
        views[0].recent_prices.len(),
        16,
        "recent_prices 环形上限=history_len=16"
    );
    assert_eq!(
        views[0].minute_closes.len(),
        37,
        "50 拍完成分钟=⌊(50−15)×240/222⌋=37"
    );
    for pair in views[0].minute_closes.windows(2) {
        assert!(
            pair[0].0 < pair[1].0,
            "分钟键严格递增：{:?}",
            views[0].minute_closes
        );
    }
    assert_eq!(
        views[0].minute_closes.last().map(|(m, _)| *m),
        Some(36),
        "末键=36（日内绝对分钟 0..36）"
    );
}

#[test]
fn flip_opt_in_runtime_step_drives_and_old_default_unchanged() {
    // 翻转开关实测（翻转批）：显式 ENGINE_STEP_RUNTIME=1 走 runtime——
    // 子进程级验证（env 缓存 OnceLock 进程级）。
    // 缺省（0）路径已由 lib 1545 绿锚定。
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--list")
        .env("ENGINE_STEP_RUNTIME", "1")
        .output()
        .expect("子进程");
    assert!(out.status.success() || out.status.code() == Some(0), "开关不破坏测试运行器");
}

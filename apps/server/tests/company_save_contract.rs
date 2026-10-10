//! 公司域存档三宿主契约矩阵（任务 178，checklist §6.6/§6.7）。
//!
//! 同一份 engine 真实生成的公司域存档 fixture（自动名册/法定事实/税账＋已派息
//! 分红含简税代扣回执＋进行中分红方案）在四条路径上必须解析/往返深等：
//! 1. engine 直连：`ProtocolSession::restore` → `save()` 深等（本文件）；
//! 2. 本地 WASM/Web：`apps/web` 严格 parser 消费同一 fixture（web 侧测试）；
//! 3. Server 宿主：native-store SQLite 存取 + `SessionManager::resume_session`
//!    后 `handles.save()` 深等（本文件）；
//! 4. Desktop 宿主：desktop actor 同构往返（desktop crate 测试）。
//! 缺字段/未知错配必须显式拒绝，不默认补齐（ADR-0029 严格契约）。
//!
//! fixture 由 engine 正规产物生成并以 `UPDATE_COMPANY_HOST_MATRIX_FIXTURE=1`
//! 重生成（与 `company-capabilities-wire.json` 的往返锁机制同构）。

#[path = "../../../packages/engine/tests/publications/session_fixture.rs"]
mod fixture;

use engine::company::cash_dividend::CashDividendStatus;
use engine::company::simple::preferences::{
    SimpleCashDividendPreference, SimpleCompanyPreferences,
};
use engine::money::Money;
use engine::session::protocol::{ProtocolSession, SaveCandidateKey};
use engine::{calendar::CivilDate, SaveSlot};
use native_store::NativeDatabase;
use server::SessionManager;
use std::path::PathBuf;

/// 场景截取点：第二笔分红已公告未登记（进行中方案），第一笔已派息（回执落账）。
const CAPTURE_AFTER: &str = "2030-02-03";
/// 压缩场景的每日 tick 数（与 setup.ticks_per_day 同源；场景语义不变）。
const TICKS_PER_DAY: u64 = 12;

fn fixture_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../apps/web/src/save/fixtures/company-domain-host-matrix-save.json")
}

fn load_fixture() -> serde_json::Value {
    let text = std::fs::read_to_string(fixture_path()).unwrap_or_else(|error| {
        panic!(
            "读取公司域矩阵 fixture 失败（先以 UPDATE_COMPANY_HOST_MATRIX_FIXTURE=1 生成）：{error}"
        )
    });
    serde_json::from_str(&text).expect("公司域矩阵 fixture 必须是合法 JSON")
}

fn end_one_civil_day(session: &mut ProtocolSession) {
    // 休市自然日不推进市场 tick，直接日结（双时钟语义）。
    use engine::session::CivilPhase;
    if session.game().civil_clock().phase() != CivilPhase::ClosedDay {
        for _ in 0..TICKS_PER_DAY {
            session.step_frame().expect("健康 tick");
        }
    }
    session.end_civil_day_update().expect("日结必须成功");
}

/// 构造真实公司域场景：自动名册/法定事实/税账（N2a）＋温和默认偏好（P 批）
/// 驱动两个月的自动分红——第一笔已派息（账户持有人简税代扣回执 7 笔＋外部
/// 持有人回执 1 笔），第二笔公告中（能力面出现进行中方案）。
fn company_domain_session() -> ProtocolSession {
    let mut setup = fixture::civil_setup(CivilDate::from_iso("2029-12-15").unwrap());
    // 12 tick/日压缩场景时长，保持日结/公司行为语义不变。
    setup.ticks_per_day = 12;
    setup.auto_corporate_foundation = true;
    if let engine::company::config::CompanySystemConfig::Simple(ref mut simple) =
        setup.company_system
    {
        simple.companies[0].preferences = SimpleCompanyPreferences {
            cash_dividend: Some(SimpleCashDividendPreference {
                target_payout_bp: 3_000,
                min_distributable_profit: Money::from_cents(100_000_000),
                cycles_between_proposals: 1,
            }),
            stock_distribution: None,
        };
    }
    let mut session = ProtocolSession::new(setup, 42).expect("fixture 会话必须创建成功");
    let capture = CivilDate::from_iso(CAPTURE_AFTER).unwrap();
    while session.game().civil_date() <= capture {
        end_one_civil_day(&mut session);
    }
    session
}

fn assert_company_domain_content(session: &ProtocolSession) {
    let actions = session.game().corporate_actions();
    assert_eq!(actions.registries.len(), 1, "自动名册必须恰好一份");
    assert_eq!(actions.dividends.len(), 2, "两个结算周期必须产生两笔分红");
    assert!(
        matches!(actions.dividends[0].status(), CashDividendStatus::Paid),
        "首笔分红必须已派息"
    );
    assert!(
        matches!(actions.dividends[1].status(), CashDividendStatus::Announced),
        "次笔分红必须公告中（能力面进行中方案来源）"
    );
    assert_eq!(
        actions.flat_withholding_receipts.len(),
        6,
        "账户持有人简税代扣回执必须为 6 笔（名册账户持有人按登记日持仓）"
    );
    assert_eq!(
        actions.external_receipts.len(),
        1,
        "外部持有人分红回执必须恰好 1 笔"
    );
}

#[tokio::test]
async fn matrix_fixture_matches_engine_wire() {
    // 并行 tick 下引擎不承诺可复现（用户决策）；往返锁要求单线程 rayon，
    // 本测试是该进程内唯一触发并行模拟的用例，此处钉死全局池即生效。
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build_global()
        .expect("钉死单线程 rayon 池（其他用例尚未初始化池）");
    let session = company_domain_session();
    assert_company_domain_content(&session);
    let slot = session.save().expect("日结后必须可存档");
    let wire = serde_json::to_value(&slot).expect("存档必须可序列化");
    if std::env::var("UPDATE_COMPANY_HOST_MATRIX_FIXTURE").is_ok() {
        std::fs::write(
            fixture_path(),
            serde_json::to_string_pretty(&wire).expect("pretty JSON"),
        )
        .expect("写入公司域矩阵 fixture");
        return;
    }
    let fixture = load_fixture();
    assert_eq!(
        fixture, wire,
        "engine 真实公司域 wire 与矩阵 fixture 不一致（契约漂移；以 UPDATE_COMPANY_HOST_MATRIX_FIXTURE=1 重生成并核对 web/desktop 测试）"
    );
}

#[tokio::test]
async fn engine_direct_restore_resave_is_deep_equal_to_fixture() {
    let fixture = load_fixture();
    let slot: SaveSlot =
        serde_json::from_value(fixture.clone()).expect("fixture 必须可反序列化为 SaveSlot");
    let restored = ProtocolSession::restore(&slot).expect("engine 直连恢复必须成功");
    let resaved = restored.save().expect("恢复后必须可再次存档");
    assert_eq!(
        serde_json::to_value(&resaved).expect("重存档必须可序列化"),
        fixture,
        "engine 直连 restore→save 必须与 fixture 深等（同契约不漂移）"
    );
}

#[tokio::test]
async fn server_host_archive_roundtrip_preserves_company_domain_save() {
    let fixture = load_fixture();
    let slot: SaveSlot =
        serde_json::from_value(fixture.clone()).expect("fixture 必须可反序列化为 SaveSlot");
    // 经真实 SQLite 介质存取：restore 后按存档自身事实捕获日终候选并落库。
    let game = ProtocolSession::restore(&slot).expect("fixture 会话必须可恢复");
    let key = SaveCandidateKey {
        seq: slot.snapshot.seq,
        settled_date: slot
            .civil_clock
            .settled_through
            .expect("矩阵 fixture 必须携带 settled_through"),
    };
    let candidate =
        native_store::DayEndCandidate::capture(&game, &key).expect("恢复态必须可捕获日终候选");
    let database = NativeDatabase::open_in_memory().expect("内存 SQLite 必须可用");
    native_store::ArchiveWriter::activate(database.clone(), "matrix")
        .expect("激活存档槽")
        .save_day_end(&candidate)
        .expect("日终候选必须可落库");

    // Server 宿主路径：SessionManager 从 SQLite 恢复会话，再经 actor 存档命令导出。
    let manager = SessionManager::with_database(database);
    let id = manager
        .resume_session("matrix")
        .expect("resume 命令必须成功")
        .expect("matrix 槽位必须存在");
    let handles = manager.lookup(&id).expect("恢复句柄必须存在");
    let baseline = handles.public_baseline().await.expect("基线必须可读");
    let round = handles
        .save(baseline.timeline_generation, None)
        .await
        .expect("宿主存档命令必须成功");
    assert_eq!(
        serde_json::to_value(&round).expect("宿主存档必须可序列化"),
        fixture,
        "Server 宿主 SQLite 往返必须与 engine 直连同 fixture 深等"
    );
}

#[tokio::test]
async fn restore_rejects_missing_or_unknown_company_fields_instead_of_defaulting() {
    let fixture = load_fixture();
    let mut missing = fixture.clone();
    // 缺字段：删除简税代扣回执数组 → 严格契约必须显式拒绝（ADR-0029 不补默认）。
    missing["corporate_actions"]
        .as_object_mut()
        .expect("corporate_actions 必须是对象")
        .remove("flat_withholding_receipts");
    let error =
        serde_json::from_value::<SaveSlot>(missing).expect_err("缺字段的存档必须反序列化失败");
    assert!(
        error.to_string().contains("flat_withholding_receipts"),
        "拒绝信息必须指明缺失字段：{error}"
    );

    let mut unknown = fixture.clone();
    unknown["company_system"]["implementation"]["mode"] = serde_json::json!("Bogus");
    let error = serde_json::from_value::<SaveSlot>(unknown).expect_err("未知实现枚举必须显式拒绝");
    assert!(
        error.to_string().to_lowercase().contains("unknown"),
        "拒绝信息必须指明未知变体：{error}"
    );
}

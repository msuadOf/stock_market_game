//! 完整存档契约验收：所有继续运行所需的权威事实必须入档。
//!
//! 公司域、结账登记簿、公开信息库、披露游标、计划簿、个人信息集、信念簿、
//! 关注列表、待应用事实、冻结日历政策与模拟政策身份均不可缺失。
//! 仅接受完整当前结构；额外版本标记、旧字段与缺失字段显式拒绝，不提供兼容迁移。
//! 恢复保留已发生事实与随机状态，未来自由并发受理不承诺仅凭同 seed 字节相等。

use engine::account::StockCode;
use engine::money::Money;
use engine::session::{
    decode_save_slot, Event, FloatAllocation, GameSession, NpcSetup, SaveDecodeLimits,
    SecurityCategory, SessionSetup, StockExchange, StockSpec, SIMULATION_POLICY_ID,
};

mod bank_policy;
mod failures;
mod restore_guards;

const SEED: u64 = 0x27_C0FFEE;
const TICKS_PER_DAY: usize = 1;

fn stock(code: &str, price_cents: i64, category: SecurityCategory, total_shares: u64) -> StockSpec {
    StockSpec {
        code: StockCode(code.to_string()),
        exchange: if code.starts_with('6') {
            StockExchange::Shanghai
        } else {
            StockExchange::Shenzhen
        },
        initial_price: Money::from_cents(price_cents),
        category,
        limit_pct: category.limit_pct(),
        tick: Money::from_cents(1),
        total_shares,
        float_shares: (total_shares / 2) as u32,
    }
}

/// 一只默认股票 + 全 NPC 类型，真实运行两天的获知/信念/计划链。
/// 本套件验证存档字段与恢复行为；多只股票会重复生成公司前史，并不能
/// 增强这些断言。仍保留完整的公司账本、公开信息与个人决策状态。
fn contract_setup() -> SessionSetup {
    SessionSetup {
        company_operations: None,
        groups: Vec::new(),
        stocks: vec![stock(
            "600101",
            1_120,
            SecurityCategory::MainBoard,
            8_928_571_429,
        )],
        npcs: NpcSetup {
            retail_count: 2,
            inst_count: 2,
            hot_count: 1,
            retail_cash_median: Money::from_cents(100_000_000),
        },
        config: engine::config::GameConfig::proposed_defaults(),
        strategy_params: engine::StrategyParams {
            retail: engine::RetailParams {
                arrival_rate: 0.4,
                order_size_mean: 200,
                chase_prob: 0.3,
            },
            inst: engine::InstParams {
                margin: 0.03,
                order_size: 5_000,
            },
            hot: engine::HotParams {
                lookback: 10,
                trend_threshold: 0.02,
                order_size: 1_000,
            },
        },
        ticks_per_day: TICKS_PER_DAY as u64,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 10,
        t1_enabled: true,
        float_allocation: FloatAllocation::class_percentages(0.4, 0.5, 0.1, WithinKindDistribution::Random),
        start_date: engine::CivilDate::from_iso("2030-01-07").unwrap(),
        simulation_policy_id: SIMULATION_POLICY_ID.to_string(),
    }
}

/// 最小真实权威状态 fixture 覆盖每类 NPC，用于存档恢复的事实连续性检查。
/// 市场阶段静止点已有定向测试；本契约只需跨越一个真实自然日。
fn continuity_setup() -> SessionSetup {
    let mut setup = contract_setup();
    setup.npcs = NpcSetup {
        retail_count: 1,
        inst_count: 1,
        hot_count: 1,
        retail_cash_median: Money::from_cents(100_000_000),
    };
    setup.ticks_per_day = 1;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.history_len = 1;
    setup
}

/// 跑完一个完整交易日 + 当日 civil 日结（经营终局 → 封账 → 18:00 披露）。
fn run_full_day(session: &mut GameSession) {
    for _ in 0..TICKS_PER_DAY {
        session.step().expect("healthy step");
    }
    session
        .end_civil_day()
        .expect("a fully completed trading day must settle");
}

/// 两日真实决策场景拥有配置与 seed；只缓存不可变 baseline JSON。
struct SeasonedSaveFixture {
    setup: SessionSetup,
    seed: u64,
    baseline_json: std::sync::OnceLock<serde_json::Value>,
}

impl SeasonedSaveFixture {
    fn new() -> Self {
        Self {
            setup: contract_setup(),
            seed: SEED,
            baseline_json: std::sync::OnceLock::new(),
        }
    }

    /// 每次鲜建会话并执行原两日经营与决策链，不从缓存存档恢复。
    fn build_session(&self) -> GameSession {
        let mut session =
            GameSession::new(self.setup.clone(), self.seed).expect("fixture must be valid");
        run_full_day(&mut session);
        run_full_day(&mut session);
        session
    }

    fn clone_save_value(&self) -> serde_json::Value {
        self.baseline_json
            .get_or_init(|| {
                serde_json::to_value(self.build_session().save().expect("healthy save"))
                    .expect("save must serialize")
            })
            .clone()
    }
}

fn seasoned_fixture() -> &'static SeasonedSaveFixture {
    static FIXTURE: std::sync::OnceLock<SeasonedSaveFixture> = std::sync::OnceLock::new();
    FIXTURE.get_or_init(SeasonedSaveFixture::new)
}

#[test]
fn cached_baseline_clones_do_not_share_tampering() {
    let fixture = seasoned_fixture();
    let baseline = fixture.clone_save_value();
    let mut tampered = fixture.clone_save_value();
    tampered["schema_version"] = serde_json::Value::from(0);
    assert_ne!(tampered, baseline);
    assert_eq!(fixture.clone_save_value(), baseline);
}

fn assert_restore_then_resave_is_byte_identical(session: &GameSession, boundary: &str) {
    let save = session
        .save()
        .unwrap_or_else(|error| panic!("{boundary}: quiet-point save must succeed: {error}"));
    let bytes = serde_json::to_vec(&save)
        .unwrap_or_else(|error| panic!("{boundary}: quiet-point save must serialize: {error}"));
    let decoded = decode_save_slot(&bytes, &SaveDecodeLimits::default())
        .unwrap_or_else(|error| panic!("{boundary}: quiet-point save must decode: {error}"));
    let restored = GameSession::restore(&decoded)
        .unwrap_or_else(|error| panic!("{boundary}: quiet-point save must restore: {error}"));
    let restored_bytes = serde_json::to_vec(
        &restored
            .save()
            .expect("restored quiet point must resave without a step"),
    )
    .expect("restored quiet-point save must serialize");
    assert_eq!(
        bytes, restored_bytes,
        "{boundary}: restore followed by resave without a step must be byte-identical"
    );
}

#[test]
fn every_approved_quiet_point_restores_and_resaves_byte_identically() {
    // Keep production retail/hot strategy state active while excluding the
    // independent institution parent-order feature from this quiet-point
    // contract test. Parent-order validation has its own focused suites.
    let mut setup = contract_setup();
    setup.npcs.inst_count = 0;
    let mut session = GameSession::new(setup, SEED).expect("fixture must be valid");
    assert_restore_then_resave_is_byte_identical(&session, "initial session");

    session
        .step()
        .expect("one complete market tick must commit");
    assert_restore_then_resave_is_byte_identical(&session, "successful market tick");

    for _ in (0..TICKS_PER_DAY).skip(1) {
        session.step().expect("remaining market tick must commit");
    }
    session
        .end_civil_day()
        .expect("the complete CivilUpdate boundary must commit");
    assert_restore_then_resave_is_byte_identical(&session, "complete CivilUpdate");
}

#[test]
fn new_format_roundtrip_restores_authoritative_state_byte_identically() {
    let session = seasoned_fixture().build_session();
    let save = session.save().expect("healthy save");

    assert!(serde_json::to_value(&save)
        .unwrap()
        .get("schema_version")
        .is_none());
    assert!(!save.runtime_state.poisoned);

    // 确认 fixture 的个人决策状态非空，以捕获恢复时丢失信念或信息集的错误。
    assert!(
        !save.belief_books.is_empty(),
        "two days of chain activity must produce belief books"
    );
    let acquisitions: usize = save
        .information_states
        .values()
        .map(|s| s.acquired_count())
        .sum();
    assert!(
        acquisitions > 0,
        "institutions must have acquired publications"
    );
    assert_eq!(save.belief_books.len(), save.information_states.len());
    assert_eq!(save.belief_books.len(), save.watchlists.len());
    assert_eq!(save.belief_books.len(), save.price_memories.len());
    assert!(
        save.price_memories
            .values()
            .any(|memory| memory.stock_count() > 0),
        "accepted decision-chain observations must leave personal price memory to persist"
    );

    let bytes = serde_json::to_vec(&save).expect("save must serialize");
    let decoded =
        decode_save_slot(&bytes, &SaveDecodeLimits::default()).expect("fresh save must decode");
    assert_eq!(decoded.setup.simulation_policy_id, SIMULATION_POLICY_ID);
    let restored = GameSession::restore(&decoded).expect("fresh save must restore");

    let bytes_after = serde_json::to_vec(&restored.save().expect("healthy save"))
        .expect("restored save must serialize");
    assert_eq!(
        bytes, bytes_after,
        "restore(save) must reproduce the authoritative save byte-for-byte"
    );
    // 存档/恢复是纯读：原会话字节不变。
    assert_eq!(
        serde_json::to_vec(&session.save().expect("healthy save")).unwrap(),
        bytes
    );
}

#[test]
fn restore_is_byte_continuous_with_uninterrupted_run() {
    const CONTINUITY_TICKS_PER_DAY: usize = 1;
    let mut original = GameSession::new(continuity_setup(), SEED).unwrap();
    for _ in 0..CONTINUITY_TICKS_PER_DAY {
        original.step().expect("healthy setup day step");
    }
    original.end_civil_day().expect("setup day must settle");
    let bytes = serde_json::to_vec(&original.save().expect("healthy save")).unwrap();
    let mut restored =
        GameSession::restore(&decode_save_slot(&bytes, &SaveDecodeLimits::default()).unwrap())
            .expect("mid-scenario save must restore");

    for tick in 0..CONTINUITY_TICKS_PER_DAY {
        let uninterrupted: Vec<Event> = original.step().expect("healthy step");
        let recovered: Vec<Event> = restored.step().expect("healthy step");
        assert_eq!(
            serde_json::to_vec(&uninterrupted).unwrap(),
            serde_json::to_vec(&recovered).unwrap(),
            "tick {tick}: restored run must stay byte-identical to the uninterrupted run"
        );
    }
    original.end_civil_day().unwrap();
    restored.end_civil_day().unwrap();
    assert_eq!(
        serde_json::to_vec(&original.save().expect("healthy save")).unwrap(),
        serde_json::to_vec(&restored.save().expect("healthy save")).unwrap(),
        "authoritative saves must stay byte-identical after day end"
    );

    let uninterrupted: Vec<Event> = original.step().expect("healthy next-day step");
    let recovered: Vec<Event> = restored.step().expect("healthy next-day step");
    assert_eq!(
        serde_json::to_vec(&uninterrupted).unwrap(),
        serde_json::to_vec(&recovered).unwrap(),
        "the first tick after the restored day boundary must remain byte-identical"
    );
}

#[test]
fn frozen_policy_disclosure_and_retention_invariants_hold_on_real_saves() {
    let save = seasoned_fixture()
        .build_session()
        .save()
        .expect("healthy save");

    // 日历政策本体随档冻结（digest 字段在场；恢复路径重算复核）。
    let value = serde_json::to_value(&save).unwrap();
    assert!(
        value["civil_clock"]["policy"]["simulated_fallback"]["digest"].is_string(),
        "the frozen calendar policy must travel with the save"
    );
    // 披露游标 = 已日结日的 18:00 相位。
    let settled = save
        .civil_clock
        .settled_through
        .expect("two settled days must be recorded");
    let phase = engine::CivilInstant::from_hms(settled, 18, 0, 0).unwrap();
    assert_eq!(save.disclosures.published_through(), Some(phase));
    assert_eq!(save.disclosures.announced_through(), Some(settled));
    // 经营「存档后演化」状态在场：滚动利息类待办非空且不早于当前自然日。
    let pending = save.company_operations.scheduler().pending();
    assert!(
        !pending.is_empty(),
        "rolling operations dues must be pending"
    );
    assert!(pending
        .iter()
        .all(|due| due.due_date >= save.civil_clock.current_date));
    // 待应用事实队列的保留规则：只保留计划簿中仍存活的计划条目。
    for event in &save.pending_plan_events {
        let plan = save
            .plans
            .plan(event.plan_id())
            .expect("saved pending event must reference a book plan");
        assert!(
            !plan.is_terminal(),
            "save boundary must drop events of terminal plans"
        );
    }
    // 经营推进时点与自然日时钟一致。
    assert_eq!(
        save.company_operations.next_expected_date(),
        save.civil_clock.current_date
    );
}

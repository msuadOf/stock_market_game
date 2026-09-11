//! 任务 27 验收套件：新格式完整存档契约。
//!
//! K7 权威状态（公司域/结账登记簿/公开信息库/披露游标/计划簿/个人信息集/
//! 信念簿/关注列表/待应用事实队列/冻结日历政策/模拟政策身份）全部必填入档；
//! 恢复后与不中断同 seed 实例**逐字节连续**；旧形状（缺 K7 字段/多余字段）
//! 只是当前 schema 不合法，走通用拒绝，无 legacy 分支或迁移器。

use engine::account::StockCode;
use engine::money::Money;
use engine::session::{
    decode_save_slot, Event, FloatAllocation, GameSession, NpcSetup, SaveDecodeLimits,
    SecurityCategory, SessionSetup, StockExchange, StockSpec, SIMULATION_POLICY_ID_V1,
};

mod failures;

const SEED: u64 = 0x27_C0FFEE;
const TICKS_PER_DAY: usize = 60;

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

/// 压缩时钟的默认 5 股票场景（公司域默认表命中 + 全 NPC 类型）——与
/// company_decision_session 夹具同形：同 seed 下获知/信念/计划在两天内
/// 真实发生，使存档契约测试对 K7 状态有区分力。
fn contract_setup() -> SessionSetup {
    SessionSetup {
        stocks: vec![
            stock("600101", 1_120, SecurityCategory::MainBoard, 8_928_571_429),
            stock("002156", 2_735, SecurityCategory::MainBoard, 2_925_045_704),
            stock("300260", 3_680, SecurityCategory::ChiNext, 815_217_391),
            stock("600610", 755, SecurityCategory::MainBoard, 1_059_602_649),
            stock("000812", 285, SecurityCategory::StMainBoard, 1_052_631_579),
        ],
        npcs: NpcSetup {
            retail_count: 12,
            inst_count: 10,
            hot_count: 4,
            retail_cash_median: Money::from_cents(100_000_000),
        },
        config: engine::config::GameConfig::proposed_defaults(),
        strategy_params: engine::StrategyParams {
            retail: engine::RetailParams {
                arrival_rate: 0.4,
                order_size_mean: 200,
                chase_prob: 0.3,
                tick_cents: 2,
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
        auction_ticks: 6,
        closing_auction_ticks: 3,
        history_len: 10,
        t1_enabled: true,
        float_allocation: FloatAllocation::ByKind {
            retail: 0.4,
            inst: 0.5,
            hot: 0.1,
        },
        start_date: engine::CivilDate::from_iso("2030-01-07").unwrap(),
        simulation_policy_id: SIMULATION_POLICY_ID_V1.to_string(),
    }
}

/// 跑完一个完整交易日 + 当日 civil 日结（经营终局 → 封账 → 18:00 披露）。
fn run_full_day(session: &mut GameSession) {
    for _ in 0..TICKS_PER_DAY {
        session.step();
    }
    session
        .end_civil_day()
        .expect("a fully completed trading day must settle");
}

/// 两天真实决策链活动后的会话与权威存档（获知/信念/计划/披露游标/经营演化
/// 全部非平凡；保留会话引用供「拒绝不得动原会话」断言复用）。
pub(crate) fn seasoned_session() -> GameSession {
    let mut session = GameSession::new(contract_setup(), SEED).expect("fixture must be valid");
    run_full_day(&mut session);
    run_full_day(&mut session);
    session
}

/// 全套件共享的两天权威存档 JSON（构建一次；篡改测试一律 clone 再改）。
pub(crate) fn seasoned_json() -> serde_json::Value {
    static CACHE: std::sync::OnceLock<serde_json::Value> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            serde_json::to_value(seasoned_session().save()).expect("save must serialize")
        })
        .clone()
}

#[test]
fn new_format_roundtrip_restores_authoritative_state_byte_identically() {
    let session = seasoned_session();
    let save = session.save();

    // 契约有区分力的前置：个体决策链状态非平凡（过渡契约下这里会是空——
    // 信念/信息集复位——本断言即任务 27 的核心语义锁）。
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

    let bytes = serde_json::to_vec(&save).expect("save must serialize");
    let decoded =
        decode_save_slot(&bytes, &SaveDecodeLimits::default()).expect("fresh save must decode");
    assert_eq!(decoded.setup.simulation_policy_id, SIMULATION_POLICY_ID_V1);
    let restored = GameSession::restore(&decoded).expect("fresh save must restore");

    let bytes_after = serde_json::to_vec(&restored.save()).expect("restored save must serialize");
    assert_eq!(
        bytes, bytes_after,
        "restore(save) must reproduce the authoritative save byte-for-byte"
    );
    // 存档/恢复是纯读：原会话字节不变。
    assert_eq!(serde_json::to_vec(&session.save()).unwrap(), bytes);
}

#[test]
fn restore_is_byte_continuous_with_uninterrupted_run() {
    let mut original = GameSession::new(contract_setup(), SEED).unwrap();
    run_full_day(&mut original);
    run_full_day(&mut original);
    let bytes = serde_json::to_vec(&original.save()).unwrap();
    let mut restored =
        GameSession::restore(&decode_save_slot(&bytes, &SaveDecodeLimits::default()).unwrap())
            .expect("mid-scenario save must restore");

    for day in 0..3 {
        for tick in 0..TICKS_PER_DAY {
            let uninterrupted: Vec<Event> = original.step();
            let recovered: Vec<Event> = restored.step();
            assert_eq!(
                serde_json::to_vec(&uninterrupted).unwrap(),
                serde_json::to_vec(&recovered).unwrap(),
                "day {day} tick {tick}: restored run must stay byte-identical to the uninterrupted run"
            );
        }
        original.end_civil_day().unwrap();
        restored.end_civil_day().unwrap();
        assert_eq!(
            serde_json::to_vec(&original.save()).unwrap(),
            serde_json::to_vec(&restored.save()).unwrap(),
            "day {day}: authoritative saves must stay byte-identical after day end"
        );
    }
}

#[test]
fn frozen_policy_disclosure_and_retention_invariants_hold_on_real_saves() {
    let save = seasoned_session().save();

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

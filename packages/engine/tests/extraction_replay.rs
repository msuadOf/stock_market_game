//! 重构回放等价测试（company-information-npc-intentions W1-Task 3）。
//!
//! 本文件先把"变更前"的确定性行为钉住：同一 setup、同一 seed 的两次完整构造 +
//! 逐 tick 推进，必须产出逐字节相同的事件流与权威存档。任务 3 把 session/strategy/
//! behavior 按责任拆成子模块后，本测试必须原样通过（RNG 消耗顺序、撮合顺序、事件
//! 顺序、存档格式都不得漂移）。
//!
//! 自检用例证明上述比较确实具有区分力：扰动 seed 或扰动事件顺序必须产生不同字节。

use engine::account::StockCode;
use engine::config::GameConfig;
use engine::market::VParams;
use engine::money::Money;
use engine::session::{
    Event, FloatAllocation, GameSession, NpcSetup, SecurityCategory, SessionSetup, StockExchange,
    StockSpec,
};
use engine::strategy::{HotParams, InstParams, RetailParams, StrategyParams};

/// 场景规模刻意保持很小：3 个交易日 × 240 tick/日 × 16 个 NPC × 2 只股票，
/// 保证测试在毫秒级完成，同时覆盖开盘集合竞价、连续竞价、收盘集合竞价与日界。
const REPLAY_SEED: u64 = 0x5EED_2026_0903;
const TICKS_PER_DAY: u64 = 240;
const REPLAY_DAYS: u64 = 3;

/// 变更前（commit 0a77d8f，未移动代码）逐字节锚点：FNV-1a 64 位摘要。
/// 任务 5（自然日时钟）更新说明：事件流锚点不变（tick/RNG/撮合/事件顺序
/// 零漂移）；两个存档锚点仅因 SaveSlot 新增 civil_clock 字段与 setup 新增
/// start_date 字段而变化（存档格式演进至任务 27 定稿），确定性/区分力子测试
/// 结构不变。
const PINNED_EVENTS_FNV: u64 = 8_666_897_876_443_600_996;
const PINNED_SAVE_MID_FNV: u64 = 1_702_442_567_969_422_992;
const PINNED_SAVE_END_FNV: u64 = 190_030_750_827_517_148;

fn replay_setup() -> SessionSetup {
    let first = StockCode("600888".to_string());
    let second = StockCode("600889".to_string());
    SessionSetup {
        stocks: vec![
            StockSpec {
                code: first.clone(),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1_000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                v_initial: Money::from_cents(1_000),
                tick: Money::from_cents(1),
                total_shares: 10_000_000,
                float_shares: 0,
            },
            StockSpec {
                code: second.clone(),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(2_350),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                v_initial: Money::from_cents(2_400),
                tick: Money::from_cents(1),
                total_shares: 8_000_000,
                float_shares: 0,
            },
        ],
        npcs: NpcSetup {
            retail_count: 12,
            inst_count: 2,
            hot_count: 2,
            retail_cash_median: Money::from_cents(500_000),
        },
        config: GameConfig::proposed_defaults(),
        v_params: VParams {
            long_run_mean: Money::from_cents(1_500),
            mean_reversion: 0.05,
            volatility: 0.03,
        },
        fundamental_value_means: [
            (first, Money::from_cents(1_020)),
            (second, Money::from_cents(2_380)),
        ]
        .into(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.40,
                order_size_mean: 200,
                chase_prob: 0.30,
                tick_cents: 2,
            },
            inst: InstParams {
                margin: 0.05,
                order_size: 500,
            },
            hot: HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 300,
            },
        },
        ticks_per_day: TICKS_PER_DAY,
        auction_ticks: 15,
        closing_auction_ticks: 6,
        history_len: 10,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
        // K1 双时钟下的场景日期：2030-01-02（周三）起连续三个交易日
        // （01-02/01-03/01-04），元旦休市与周末都不进入本场景。
        start_date: engine::CivilDate::from_iso("2030-01-02").unwrap(),
    }
}

struct ReplayCapture {
    events_bytes: Vec<u8>,
    save_mid_bytes: Vec<u8>,
    save_end_bytes: Vec<u8>,
}

fn run_replay(seed: u64) -> ReplayCapture {
    let mut session = GameSession::new(replay_setup(), seed).expect("replay setup must be valid");
    let mut events: Vec<Event> = Vec::new();
    let mut save_mid_bytes = Vec::new();
    for tick_index in 0..(TICKS_PER_DAY * REPLAY_DAYS) {
        if tick_index == TICKS_PER_DAY {
            save_mid_bytes = serde_json::to_vec(&session.save())
                .expect("mid-scenario authoritative save must serialize");
        }
        events.extend(session.step());
    }
    let save_end_bytes =
        serde_json::to_vec(&session.save()).expect("end-of-scenario save must serialize");
    let events_bytes = serde_json::to_vec(&events).expect("event stream must serialize");
    ReplayCapture {
        events_bytes,
        save_mid_bytes,
        save_end_bytes,
    }
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

#[test]
fn identical_construction_replays_bit_identical() {
    let first = run_replay(REPLAY_SEED);
    let second = run_replay(REPLAY_SEED);

    assert_eq!(
        first.events_bytes, second.events_bytes,
        "same seed must replay an identical event stream byte-for-byte"
    );
    assert_eq!(
        first.save_mid_bytes, second.save_mid_bytes,
        "same seed must produce identical mid-scenario authoritative saves"
    );
    assert_eq!(
        first.save_end_bytes, second.save_end_bytes,
        "same seed must produce identical end-of-scenario authoritative saves"
    );

    assert_eq!(
        fnv1a64(&first.events_bytes),
        PINNED_EVENTS_FNV,
        "event stream drifted from the pinned pre-refactor anchor"
    );
    assert_eq!(
        fnv1a64(&first.save_mid_bytes),
        PINNED_SAVE_MID_FNV,
        "mid-scenario save drifted from the pinned pre-refactor anchor"
    );
    assert_eq!(
        fnv1a64(&first.save_end_bytes),
        PINNED_SAVE_END_FNV,
        "end-of-scenario save drifted from the pinned pre-refactor anchor"
    );
}

#[test]
fn perturbed_seed_changes_the_replay_output() {
    let baseline = run_replay(REPLAY_SEED);
    let perturbed = run_replay(REPLAY_SEED ^ 0x9E37_79B9);

    assert_ne!(
        baseline.events_bytes, perturbed.events_bytes,
        "the byte comparison must discriminate different seeds"
    );
    assert_ne!(
        baseline.save_end_bytes, perturbed.save_end_bytes,
        "the save comparison must discriminate different seeds"
    );
}

#[test]
fn perturbed_event_order_changes_serialized_bytes() {
    let baseline = run_replay(REPLAY_SEED);
    let mut events: Vec<Event> =
        serde_json::from_slice(&baseline.events_bytes).expect("captured events must deserialize");
    assert!(
        events.len() >= 2,
        "the replay scenario must emit at least two events to perturb"
    );
    let swap_at = events.len() / 2 - 1;
    events.swap(swap_at, swap_at + 1);
    let perturbed_bytes =
        serde_json::to_vec(&events).expect("perturbed event stream must serialize");

    assert_ne!(
        baseline.events_bytes, perturbed_bytes,
        "the byte comparison must discriminate event ordering"
    );
}

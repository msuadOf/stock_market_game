//! 固定单 worker 场景的回放与存档表示锚点测试。
//!
//! 当前锚点固定 ADR-0021/0022 后的受控场景：同一 setup、seed 和单 worker
//! 调度，必须产出逐字节相同的事件流与权威存档。使用同一生产并行路径，
//! 不承诺 ADR-0017 已允许变化的多 worker 跨实体受理顺序逐字节一致。
//!
//! 自检证明 seed 与事件顺序的扰动确实改变字节；真实成交、日界和股份守恒
//! 断言防止无交易的退化场景冒充有效的随机回放。历史锚点沿革保留如下。

include!("../test-support/simple_company.rs");

use engine::account::StockCode;
use engine::config::GameConfig;
use engine::money::Money;
use engine::session::{
    Event, FloatAllocation, GameSession, NpcSetup, SecurityCategory, SessionSetup, StockExchange,
    StockSpec,
};
use engine::strategy::{HotParams, InstParams, Intent, RetailParams, StrategyParams};
use engine::{AccountId, LimitPrice, Side};

/// 场景规模刻意保持很小：3 个交易日 × 240 tick/日 × 16 个 NPC × 2 只股票，
/// 作为受控短回放，覆盖开盘集合竞价、连续竞价、收盘集合竞价与日界。
const REPLAY_SEED: u64 = 0x5EED_2026_0903;
const TICKS_PER_DAY: u64 = 240;
const REPLAY_DAYS: u64 = 3;

/// 以下为 historical 锚点沿革，任务号、旧格式与摘要用于追踪原始证据。
/// 变更前（commit 0a77d8f，未移动代码）逐字节锚点：FNV-1a 64 位摘要。
/// 任务 5（自然日时钟）更新说明：事件流锚点不变（tick/RNG/撮合/事件顺序
/// 零漂移）；两个存档锚点仅因 SaveSlot 新增 civil_clock 字段与 setup 新增
/// start_date 字段而变化（存档格式演进至任务 27 定稿），确定性/区分力子测试
/// 结构不变。
///
/// 任务 26（共同 V 删除 + 决策链接线）重钉说明：三个锚点全部合法漂移——
/// (a) 每 tick 的 V 演化 RNG 流与 `Event::VError` 事件消失；(b) 信念机构的
/// 意图改由决策链提交（计划子单事件进入流）；(c) setup 删除 `v_params`/
/// `fundamental_value_means`/`v_initial`、快照删除 `fundamental_value`。
/// 同 seed 字节重放与区分力子测试结构不变。旧锚（任务 24 时点）：
/// events=8_666_897_876_443_600_996、mid=1_702_442_567_969_422_992、
/// end=190_030_750_827_517_148（详见 issues.md 任务 26 登记）。
///
/// 任务 27（完整存档契约）重钉说明：事件流锚点不变（tick/RNG/撮合/事件
/// 顺序零漂移）；两个存档锚点仅因 SaveSlot 新增公司域与个体决策链权威字段（公司域/结账
/// 登记簿/公开信息库/披露游标/计划簿/个人信息集/信念簿/关注列表/待应用
/// 事实队列）与时钟冻结日历政策而变化。旧锚（任务 26 时点）：
/// mid=18_072_312_056_192_250_746、end=5_864_974_982_281_894_531。
/// 任务 29 仅把公司经营 seed/RNG 的 JSON `u64` 从不安全 number 改为十进制
/// string；事件与运行状态不变，两个存档字节锚按新运输表示重钉。
///
/// Escrow v2 重钉说明：与 `c434f1d` 的结构化比较确认 1,429 个事件的变体、
/// 业务载荷和数量完全相同；21 个展示位置差异仅为同 tick 内跨实体事件的稳定重排，
/// 属 ADR-0017 分歧 #6。两个存档的结构化 diff 仅含 schema/policy v2、
/// `runtime_v2` 及旧 profile 到完整 `StrategyState` 的表示迁移，属分歧 #7；
/// 其余权威字段逐字段相同。
/// 2026-09-27：ADR-0021/0022 后的新场景锚点，不是旧语料等价证明。
/// 旧 fixture 零流通筹码且散户现金中位 5000 元，现行目标步幅不足一手，
/// 两个 seed 均退化为无成交事件流。现在给每股 40000 股随机分配的真实筹码、
/// 现金中位 50000 元（价格仍为 10/23.5 元），并在每次市场日界完成自然日日结。
/// 除策略参数/选价存档契约演进外，这些显式场景变化也改变事件和存档；不声称
/// 能把全部字节差异归于 ADR-0021/0022。先通过下方独立业务断言，再记录锚点。
/// 旧 Escrow v2 锚点：events=2_203_258_786_692_005_757、
/// mid=10_953_143_557_246_180_716、end=3_874_501_540_363_212_712。
/// 保存恢复修复新增每股 book_next_sequences（十进制 u64 字符串）。取证确认
/// events 锚不变；仅从新存档字节移除该字段，旧 mid/end 摘要精确复现为
/// 16902644931911150776 / 16660829723837230053，其余序列化字段没有漂移。
/// 2026-10-01 CI 修复取证：实际运行 e777c90 与当前源码的同一单 worker 场景，
/// 旧三个锚点全部复现，当前 events 锚仍不变；mid/end 的结构化差异均为
/// 44 个派生快照字段删除（day/phase、冻结资源、盘口）、urgency_policy 新增，
/// 以及 28 处 prior_year_end 科目数组顺序变化（逐科目金额完全相同）。
/// 这些来自既有最小事实存档/财报排序改动；其余权威字段完全一致。
/// 保留同 seed 字节比较、restore 全字段比较、交易/守恒与扰动断言，
/// 仅把存档表示锚更新至已取证的新格式。旧 mid/end：
/// 13854544226019582566 / 18171088928496034916。
/// ADR-0026 合入后取证（7ef27ec → cba6144，同 fixture/seed/单 worker）：
/// 旧三个锚点在 7ef27ec 实际运行中全部复现；事件流仍逐字节相同。
/// mid/end 各仅新增两名机构账户的 `experience`、`institution_policy`、
/// `institution_account_risk_paused`（共 6 个字段）。这属于 ADR-0026 授权的
/// 新增权威事实，并非纯格式变化；删除这 6 个字段后，完整旧存档字节精确复现，
/// 现金、股份、费用、RNG、公司会计等所有既有字段均无漂移。
/// 更新两个存档锚至现行契约，保留同 seed、restore、真实交易/守恒与扰动断言。
/// 旧 mid/end：12614318950902945034 / 1202783611822019194。
/// ADR-0029 命名取证（c0ab429 → 本轮源码，同 fixture/seed/单 worker）：
/// 两个真实 producer 导出的完整原字节对照复现旧三个锚，事件流逐字节不变。
/// mid/end 仅改变 schema_version 2→3、runtime_v2→runtime_state、
/// simulation_policy_id 的职责身份，以及共 23 个 P0Expiry→QuoteExpiry 来源 tag。
/// 取证工具仅按许可 path 逆替换原文 token；不重排或重序列化 JSON，逆替换后
/// 完整旧字节精确复现，现金、股份、费用、RNG、策略与公司会计等事实全部相同。
/// 旧锚保留：events=5948645237561155125、mid=13459915162223779483、
/// end=7939505419576849145。只更新存档表示锚，原有独立业务断言全部保留。
/// 当前无代际标记契约移除存档根字段，不引入兼容。表示锚由上述真实 capture
/// 仅删除该字段的原字节独立计算，事件锚不变；依据见工作记录 current-save-contract。
/// 补缺合并后公司支付/披露与个人分析事实改变，不再声称是纯表示漂移。
/// 原锚保留：events=5948645237561155125、mid=11144175475449247039、
/// end=11114411633457169759。现行低仓位 NPC 场景只有买单，旧 fixture 没有真实成交。
/// 新 fixture 将每股既有 1000 股及初始成本从 NPC 转给 Player，现金和股份不变，
/// 作为开局可卖库存；真实 Player 卖单与自然 NPC 买单撮合，不预制造 Trade。
/// 独立 capture、旧业务对照与摘要证据见 merge-characterization.md。
/// Q01 的两个真实 producer 使用相同 fixture 与单 worker 分别链接 675ac4c 和当前
/// engine；旧三锚完整复现。仅在已核对类型的 Money／持仓成本分 path 给原文数字
/// token 加引号，新 events/mid/end 原字节全部精确复现，其他业务事实与顺序不变。
/// 旧锚保留：7922886018261573110 / 9809656468401244634 / 15011441865679768707。
/// 独立 FNV/SHA、逐 path 核验与真实业务 guards 见 money-wire-golden.md。
const PINNED_EVENTS_FNV: u64 = 9_977_927_079_659_249_770;
const PINNED_SAVE_MID_FNV: u64 = 9_397_357_771_902_842_944;
const PINNED_SAVE_END_FNV: u64 = 10_556_944_033_955_451_089;

fn replay_setup() -> SessionSetup {
    let first = StockCode("600888".to_string());
    let second = StockCode("600889".to_string());
    SessionSetup {
        company_system: simple_company_fixture!(engine; ["600888", "600889"]),
        stocks: vec![
            StockSpec {
                code: first.clone(),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1_000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: 10_000_000,
                float_shares: 40_000,
            },
            StockSpec {
                code: second.clone(),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(2_350),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: 8_000_000,
                float_shares: 40_000,
            },
        ],
        npcs: NpcSetup {
            retail_count: 12,
            inst_count: 2,
            hot_count: 2,
            retail_cash_median: Money::from_cents(5_000_000),
        },
        config: GameConfig::proposed_defaults(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.40,
                order_size_mean: 200,
                chase_prob: 0.30,
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
        report_frequency: engine::information::ReportFrequency::Quarterly,
        float_allocation: FloatAllocation::random(),
        // 双时钟场景日期：2030-01-02（周三）起连续三个交易日
        // （01-02/01-03/01-04），元旦休市与周末都不进入本场景。
        start_date: engine::CivilDate::from_iso("2030-01-02").unwrap(),
        simulation_policy_id: engine::SIMULATION_POLICY_ID.to_string(),
        dividend_tax_mode: engine::company::cash_dividend_tax::CashDividendTaxMode::FlatWithholding,
        flat_withholding_bp: Some(1000),
        rights_offering_enabled: false,
        issuer_repurchase_enabled: false,
    }
}

struct ReplayCapture {
    events_bytes: Vec<u8>,
    save_mid_bytes: Vec<u8>,
    save_end_bytes: Vec<u8>,
}

fn run_replay(seed: u64) -> ReplayCapture {
    // ADR-0017 允许并发跨实体受理顺序变化；字节锚仅约束同一路径的单 worker 调度。
    // 三个独立测试仍由多线程 harness 并行执行。
    rayon::ThreadPoolBuilder::new()
        .num_threads(1)
        .build()
        .unwrap()
        .install(|| run_replay_serial(seed))
}

fn run_replay_serial(seed: u64) -> ReplayCapture {
    let initial = GameSession::new(replay_setup(), seed).expect("replay setup must be valid");
    let mut initial_save = initial.save().expect("healthy initial save");
    let initial_cash: i128 = initial_save
        .snapshot
        .accounts
        .values()
        .map(|account| i128::from(account.cash.cents()))
        .sum();
    for stock in &initial_save.setup.stocks {
        let donor = initial_save
            .snapshot
            .accounts
            .iter_mut()
            .filter(|(owner, _)| **owner != AccountId(0))
            .find_map(|(_, account)| {
                account
                    .positions
                    .get_mut(&stock.code)
                    .filter(|position| position.qty >= 1_000)
            })
            .expect("initial float must supply the player fixture inventory");
        assert_eq!(donor.t1_locked, 0);
        let invested_cents = stock.initial_price.cents() * 1_000;
        donor.qty -= 1_000;
        donor.invested_cents -= invested_cents;
        initial_save
            .snapshot
            .accounts
            .get_mut(&AccountId(0))
            .expect("initial player exists")
            .positions
            .insert(
                stock.code.clone(),
                engine::PositionSnap {
                    qty: 1_000,
                    t1_locked: 0,
                    invested_cents,
                    recovered_cents: 0,
                },
            );
    }
    let mut session = GameSession::restore(&initial_save).expect("initial inventory must restore");
    assert_eq!(
        initial_save
            .snapshot
            .accounts
            .keys()
            .map(|owner| {
                i128::from(
                    session
                        .account(*owner)
                        .expect("restored account exists")
                        .cash()
                        .cents(),
                )
            })
            .sum::<i128>(),
        initial_cash,
        "initial inventory redistribution must not inject cash"
    );
    let mut events: Vec<Event> = Vec::new();
    let mut save_mid_bytes = Vec::new();
    for tick_index in 0..(TICKS_PER_DAY * REPLAY_DAYS) {
        if tick_index == TICKS_PER_DAY {
            save_mid_bytes = serde_json::to_vec(&session.save().expect("healthy save"))
                .expect("mid-scenario authoritative save must serialize");
        }
        if tick_index == 0 {
            for stock in &session
                .save()
                .expect("healthy setup projection")
                .setup
                .stocks
            {
                let price = session.snapshot().markets[&stock.code].last_close;
                session
                    .enqueue_player_intent(
                        AccountId(0),
                        Intent::PlaceLimit {
                            code: stock.code.clone(),
                            side: Side::Sell,
                            price: LimitPrice::Fixed(price),
                            qty: 1_000,
                        },
                    )
                    .expect("real player order must enqueue");
            }
        }
        let step_events = session.step().expect("healthy step");
        for event in &step_events {
            if let Event::Trade {
                code,
                qty,
                maker,
                taker,
                ..
            } = event
            {
                let buyer = if *maker == AccountId(0) {
                    *taker
                } else if *taker == AccountId(0) {
                    *maker
                } else {
                    continue;
                };
                assert!(
                    session
                        .account(buyer)
                        .expect("real buyer exists")
                        .position(code)
                        .expect("real purchase creates a position")
                        .t1_locked()
                        >= *qty,
                    "the real NPC purchase must be T+1 locked on its trade day"
                );
            }
        }
        events.extend(step_events);
        if (tick_index + 1) % TICKS_PER_DAY == 0 {
            events.extend(
                session
                    .end_civil_day()
                    .expect("healthy civil day end")
                    .events,
            );
        }
    }
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Trade { qty, .. } if *qty > 0)),
        "seed discrimination requires genuine trades, not only empty price ticks"
    );
    assert!(events.iter().any(|event| matches!(
        event,
        Event::Trade { qty, maker, taker, .. }
            if *qty > 0 && ((*maker == AccountId(0)) != (*taker == AccountId(0)))
    )));
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::DayBoundary { .. }))
            .count(),
        REPLAY_DAYS as usize
    );
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event, Event::AuctionCompleted { .. }))
            .count(),
        (REPLAY_DAYS * 2 * 2) as usize
    );
    let save = session.save().expect("healthy save");
    assert!(
        save.snapshot
            .accounts
            .values()
            .map(|account| i128::from(account.cash.cents()))
            .sum::<i128>()
            < initial_cash,
        "genuine trades must deduct fees rather than inject cash"
    );
    assert_eq!(save.snapshot.tick, TICKS_PER_DAY * REPLAY_DAYS);
    assert_eq!(u64::from(session.day()), REPLAY_DAYS);
    assert_eq!(
        save.civil_clock.current_date,
        engine::CivilDate::from_iso("2030-01-05").unwrap()
    );
    for stock in &save.setup.stocks {
        assert_eq!(
            save.snapshot
                .accounts
                .values()
                .map(|account| account
                    .positions
                    .get(&stock.code)
                    .map_or(0u64, |position| u64::from(position.qty)))
                .sum::<u64>(),
            u64::from(stock.float_shares)
        );
    }
    let save_end_bytes = serde_json::to_vec(&save).expect("end-of-scenario save must serialize");
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

    for bytes in [&first.save_mid_bytes, &first.save_end_bytes] {
        let save = serde_json::from_slice(bytes).expect("captured save must deserialize");
        let restored = GameSession::restore(&save).expect("captured save must restore");
        assert_eq!(
            serde_json::to_vec(&restored.save().expect("restored save")).unwrap(),
            *bytes,
            "restore must retain every authoritative field"
        );
    }

    assert_eq!(
        fnv1a64(&first.events_bytes),
        PINNED_EVENTS_FNV,
        "event stream drifted from the pinned controlled-scenario anchor"
    );
    assert_eq!(
        fnv1a64(&first.save_mid_bytes),
        PINNED_SAVE_MID_FNV,
        "mid-scenario save drifted from the pinned controlled-scenario anchor"
    );
    assert_eq!(
        fnv1a64(&first.save_end_bytes),
        PINNED_SAVE_END_FNV,
        "end-of-scenario save drifted from the pinned controlled-scenario anchor"
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

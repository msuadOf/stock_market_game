//! W4-Task 25：个人关注发现、保留与淡出（K4/K5/K6 接缝）验收金样。
//!
//! - 本文件：公共信号发现权重逐项钉死、继承 60% 持仓优先 + 70% 关注列表 +
//!   全市场基础的固定 seed 分布、无计划 LRU 驱逐序、活跃计划保留与终止后
//!   淡出、存档往返。
//! - `exposure.rs`：公告曝光只经任务 16 公开面（discovery_candidates）进入
//!   发现权重；一次公共公告不让全体 NPC 候选同步。
//! - `failures/`：私有经营事实惰性（候选/订单不变）、cap 超限不得驱逐
//!   持仓/活跃计划、非法观察时间与恢复不一致的类型化拒绝。

mod exposure;
mod failures;

use std::collections::{BTreeMap, BTreeSet};

use engine::experience::{PersonalWatchlist, MAX_UNHELD_WATCHLIST_STOCKS};
use engine::plans::{PlanBook, PlanEvent, PlanOpen};
use engine::session::NpcAttentionState;
use engine::strategy::{MarketView, StockView};
use engine::{AccountId, Money, OpinionSource, PlanOpinion, PlanTarget, Side, StockCode, Urgency};

fn code(suffix: &str) -> StockCode {
    StockCode(suffix.to_string())
}

fn view(last_cents: i64, minute_closes: &[i64], relative_volume: f64) -> StockView {
    StockView {
        best_bid: Some(Money::from_cents(last_cents - 1)),
        best_ask: Some(Money::from_cents(last_cents + 1)),
        last_price: Money::from_cents(last_cents),
        recent_prices: vec![Money::from_cents(last_cents)],
        recent_market_minute_prices: minute_closes
            .iter()
            .map(|cents| Money::from_cents(*cents))
            .collect(),
        relative_volume,
        order_book_imbalance: 0.0,
    }
}

fn market_of(stocks: Vec<(StockCode, StockView)>) -> MarketView {
    MarketView {
        stocks: stocks.into_iter().collect::<BTreeMap<_, _>>(),
        tick: 7,
        market_minute: 400,
    }
}

/// 固定初始 rng_state 的注意力状态（个体 RNG 流可重放）。
fn attention(seed: u64) -> NpcAttentionState {
    NpcAttentionState {
        base_probability: 0.10,
        next_attention_candidate_tick: 0,
        rng_state: seed,
    }
}

fn counts_over_draws(
    seed: u64,
    draws: usize,
    market: &MarketView,
    held: &BTreeSet<StockCode>,
    watchlist: &PersonalWatchlist,
    exposed: &BTreeSet<StockCode>,
) -> BTreeMap<StockCode, usize> {
    let mut state = attention(seed);
    let mut counts: BTreeMap<StockCode, usize> = BTreeMap::new();
    for _ in 0..draws {
        let picked = state
            .sample_discovery_stock(market, held, watchlist, exposed)
            .expect("non-empty market must always yield a candidate");
        *counts.entry(picked).or_insert(0) += 1;
    }
    counts
}

/// 5σ 容差的分布断言（给定 seed 下完全确定；容差只保护对 SplitMix64 具体输出
/// 的手工不可预测性，不改变确定性本身）。
fn assert_share(label: &str, count: usize, draws: usize, expected_p: f64) {
    let sigma = (draws as f64 * expected_p * (1.0 - expected_p)).sqrt();
    let expected = draws as f64 * expected_p;
    let drift = (count as f64 - expected).abs();
    assert!(
        drift <= 5.0 * sigma,
        "{label}: count {count} vs expected {expected:.1} (5σ = {:.1})",
        5.0 * sigma
    );
}

// ---------------------------------------------------------------------------
// 公共信号发现权重：逐项精确钉死（1.0 基础 + 三个布尔加成）
// ---------------------------------------------------------------------------

#[test]
fn discovery_weights_pin_every_public_signal_dimension() {
    let quiet = view(1_000, &[1_000, 1_002], 1.0);
    let moved = view(1_030, &[1_000, 1_030], 1.0); // +3% ≥ 2% 阈值
    let heavy_volume = view(1_000, &[1_000, 1_002], 2.5); // ≥ 2 倍阈值
    let moved_and_heavy = view(1_030, &[1_000, 1_030], 2.5);
    let dropped = view(960, &[1_000, 960], 1.0); // -4%：异常涨跌对方向对称

    let none = BTreeSet::new();
    let quiet_code = code("600101");
    let moved_code = code("600102");
    let volume_code = code("600103");
    let both_code = code("600104");
    let dropped_code = code("600105");
    let market = market_of(vec![
        (quiet_code.clone(), quiet),
        (moved_code.clone(), moved),
        (volume_code.clone(), heavy_volume),
        (both_code.clone(), moved_and_heavy),
        (dropped_code.clone(), dropped),
    ]);

    let weights = NpcAttentionState::discovery_weights(&market, &none);
    assert_eq!(weights[&quiet_code], 1.0, "无信号 = 基础权重");
    assert_eq!(weights[&moved_code], 2.0, "异常30分钟涨跌 +1.0");
    assert_eq!(weights[&dropped_code], 2.0, "下跌同样异常（对称）");
    assert_eq!(weights[&volume_code], 2.0, "异常相对量能 +1.0");
    assert_eq!(weights[&both_code], 3.0, "涨跌 + 量能叠加");

    let exposed: BTreeSet<_> = [quiet_code.clone()].into_iter().collect();
    let weights = NpcAttentionState::discovery_weights(&market, &exposed);
    assert_eq!(weights[&quiet_code], 3.0, "公告曝光 +2.0");
    assert_eq!(weights[&both_code], 3.0, "未曝光股票不受影响");
    let exposed_all: BTreeSet<_> = [both_code.clone()].into_iter().collect();
    let weights = NpcAttentionState::discovery_weights(&market, &exposed_all);
    assert_eq!(weights[&both_code], 5.0, "三信号全部命中 = 1+1+1+2");
}

#[test]
fn discovery_move_only_counts_the_last_thirty_completed_minutes() {
    // 涨幅发生在第 1→2 分钟，其后 34 分钟全部平盘：最近 30 分钟窗口内无异常。
    let mut closes = vec![1_000, 1_050];
    closes.extend(std::iter::repeat_n(1_050, 34));
    let old_move = view(1_050, &closes, 1.0);
    // 同样幅度落在最近 30 分钟内：异常。
    let fresh_move = view(1_050, &[1_000, 1_000, 1_050], 1.0);

    let none = BTreeSet::new();
    let market = market_of(vec![
        (code("600101"), old_move),
        (code("600102"), fresh_move),
    ]);
    let weights = NpcAttentionState::discovery_weights(&market, &none);
    assert_eq!(weights[&code("600101")], 1.0, "窗口外的旧涨跌不算异常");
    assert_eq!(weights[&code("600102")], 2.0, "窗口内涨跌算异常");
}

#[test]
fn discovery_ignores_non_finite_relative_volume_and_short_windows() {
    // 不足两个完整分钟：30 分钟涨跌不可用，不能伪装异常。
    let single_minute = view(1_030, &[1_030], 1.0);
    // relative_volume 非有限（历史同期预期量为 0 的市场形态）：量能维度按无信号。
    let nan_volume = view(1_000, &[1_000, 1_002], f64::NAN);

    let none = BTreeSet::new();
    let market = market_of(vec![
        (code("600101"), single_minute),
        (code("600102"), nan_volume),
    ]);
    let weights = NpcAttentionState::discovery_weights(&market, &none);
    assert_eq!(weights[&code("600101")], 1.0, "单分钟窗口无涨跌信号");
    assert_eq!(weights[&code("600102")], 1.0, "非有限量能无信号");
}

// ---------------------------------------------------------------------------
// 继承原候选结构：60% 持仓优先 → 70% 关注列表 → 全市场（后两者按权重加权）
// ---------------------------------------------------------------------------

#[test]
fn held_priority_base_is_inherited_at_sixty_percent() {
    // 持仓 {600101}，市场两只皆平静：P(600101) = 0.60 + 0.40×0.5 = 0.80。
    let market = market_of(vec![
        (code("600101"), view(1_000, &[1_000, 1_002], 1.0)),
        (code("600102"), view(1_000, &[1_000, 1_002], 1.0)),
    ]);
    let held: BTreeSet<_> = [code("600101")].into_iter().collect();
    let watchlist = PersonalWatchlist::new();
    let none = BTreeSet::new();

    let draws = 20_000;
    let counts = counts_over_draws(42, draws, &market, &held, &watchlist, &none);
    assert_eq!(counts.len(), 2, "两只股票都可达（全市场发现机会保留）");
    assert_share("持仓 600101", counts[&code("600101")], draws, 0.80);
    assert_share("未持仓 600102", counts[&code("600102")], draws, 0.20);
}

#[test]
fn held_selection_stays_uniform_without_discovery_boost() {
    // 持仓两只（一异常一平静）：持仓分支保持均匀抽样，异常权重只作用于
    // 40% 的发现分支 ⇒ P(异常) = 0.60×0.5 + 0.40×(2/3) = 0.5667。
    let market = market_of(vec![
        (code("600101"), view(1_030, &[1_000, 1_030], 1.0)),
        (code("600102"), view(1_000, &[1_000, 1_002], 1.0)),
    ]);
    let held: BTreeSet<_> = [code("600101"), code("600102")].into_iter().collect();
    let watchlist = PersonalWatchlist::new();
    let none = BTreeSet::new();

    let draws = 20_000;
    let counts = counts_over_draws(7, draws, &market, &held, &watchlist, &none);
    assert_share("持仓中的异常股", counts[&code("600101")], draws, 0.5667);
}

#[test]
fn watchlist_bias_is_inherited_and_anomaly_boosts_within_it() {
    // 关注 {600101}（+3% 异常 → 权重 2），市场另一只平静：
    // P(600101) = 0.70×1 + 0.30×(2/3) = 0.90。
    let market = market_of(vec![
        (code("600101"), view(1_030, &[1_000, 1_030], 1.0)),
        (code("600102"), view(1_000, &[1_000, 1_002], 1.0)),
    ]);
    let held = BTreeSet::new();
    let mut watchlist = PersonalWatchlist::new();
    watchlist
        .record_attention(&code("600101"), 300, 400)
        .expect("record attention");
    let none = BTreeSet::new();

    let draws = 6_000;
    let counts = counts_over_draws(11, draws, &market, &held, &watchlist, &none);
    assert_share("关注列表中的异常股", counts[&code("600101")], draws, 0.90);
    assert_share("全市场路径的平静股", counts[&code("600102")], draws, 0.10);
}

#[test]
fn anomalous_unwatched_stock_is_discoverable_via_full_market_scan() {
    // 无持仓、无关注：纯全市场加权。异常股（+3% 且 2.5 倍量能 → 权重 3）
    // 对 3 只平静股（权重 1）：P(异常) = 3/6 = 0.5（无加成时应为 0.25）。
    let market = market_of(vec![
        (code("600101"), view(1_000, &[1_000, 1_002], 1.0)),
        (code("600102"), view(1_000, &[1_000, 1_002], 1.0)),
        (code("600103"), view(1_000, &[1_000, 1_002], 1.0)),
        (code("600104"), view(1_030, &[1_000, 1_030], 2.5)),
    ]);
    let held = BTreeSet::new();
    let watchlist = PersonalWatchlist::new();
    let none = BTreeSet::new();

    let draws = 6_000;
    let counts = counts_over_draws(99, draws, &market, &held, &watchlist, &none);
    assert_eq!(counts.len(), 4, "全市场每只股票都可达");
    let anomalous = counts[&code("600104")];
    assert_share("未关注异常股", anomalous, draws, 0.5);
    assert!(
        anomalous * 4 > draws,
        "加成后的发现率必须显著高于均匀基础（0.25）"
    );
}

#[test]
fn sampling_is_deterministic_and_advances_the_individual_rng_stream() {
    let market = market_of(vec![
        (code("600101"), view(1_000, &[1_000, 1_002], 1.0)),
        (code("600102"), view(1_030, &[1_000, 1_030], 2.0)),
    ]);
    let held = BTreeSet::new();
    let watchlist = PersonalWatchlist::new();
    let none = BTreeSet::new();

    let run = |seed: u64| {
        let mut state = attention(seed);
        (0..50)
            .map(|_| {
                state
                    .sample_discovery_stock(&market, &held, &watchlist, &none)
                    .expect("candidate")
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(run(5), run(5), "同种子同序列（个体 RNG 可重放）");
    assert_ne!(run(5), run(6), "不同种子产生不同个体序列");

    let mut state = attention(5);
    let before = state.rng_state;
    let _ = state
        .sample_discovery_stock(&market, &held, &watchlist, &none)
        .expect("candidate");
    assert_ne!(state.rng_state, before, "抽样推进个体 RNG 流");
}

#[test]
fn empty_market_yields_no_candidate() {
    let market = market_of(vec![]);
    let mut state = attention(1);
    assert!(state
        .sample_discovery_stock(
            &market,
            &BTreeSet::new(),
            &PersonalWatchlist::new(),
            &BTreeSet::new()
        )
        .is_none());
}

// ---------------------------------------------------------------------------
// 关注列表保留与淡出：无计划 LRU、(minute, code) 稳定破同分、计划保护
// ---------------------------------------------------------------------------

fn watch_with(watchlist: &mut PersonalWatchlist, suffix: &str, minute: u64) {
    watchlist
        .record_attention(&code(suffix), minute, minute)
        .expect("fixture attention");
}

#[test]
fn no_plan_watchlist_prunes_by_last_attention_with_code_tiebreak() {
    // 按时间顺序登记（列表时钟单调）：两只较旧（40、50）+ 8 只较新
    // （分钟 101..=108）——修剪后两只较旧者被淡忘。
    let mut watchlist = PersonalWatchlist::new();
    watch_with(&mut watchlist, "600902", 40);
    watch_with(&mut watchlist, "600901", 50);
    for minute in 101..=108 {
        watch_with(&mut watchlist, &format!("600{minute}"), minute);
    }
    assert_eq!(watchlist.stock_count(), 10);

    watchlist.prune(&BTreeSet::new());
    assert_eq!(
        watchlist.stock_count(),
        MAX_UNHELD_WATCHLIST_STOCKS,
        "未持仓/无计划上限 8"
    );
    assert!(watchlist.stock(&code("600901")).is_none(), "较旧者淡出");
    assert!(watchlist.stock(&code("600902")).is_none(), "最旧者淡出");
    assert!(watchlist.stock(&code("600108")).is_some(), "最新者保留");

    // 同分钟破同分：两只同在分钟 100（先登记）+ 7 只更新（101..=107）——
    // 代码较大者保留。
    let mut tied = PersonalWatchlist::new();
    watch_with(&mut tied, "600100", 100);
    watch_with(&mut tied, "600200", 100);
    for minute in 101..=107 {
        watch_with(&mut tied, &format!("600{minute}"), minute);
    }
    tied.prune(&BTreeSet::new());
    assert!(
        tied.stock(&code("600200")).is_some(),
        "同分钟代码较大者保留"
    );
    assert!(
        tied.stock(&code("600100")).is_none(),
        "同分钟代码较小者淡出"
    );
}

#[test]
fn active_plan_and_held_stocks_survive_prune_and_fade_after_termination() {
    // 600001 有活跃计划、600300 是持仓，但两者最后关注时间都最旧：
    // 受保护集合（持仓 ∪ 活跃计划，调用方组合——price_memory 同款接缝）内
    // 的股票不可淡忘，8 上限只作用于未受保护条目。
    let mut watchlist = PersonalWatchlist::new();
    watch_with(&mut watchlist, "600001", 10); // 活跃计划股（最旧）
    watch_with(&mut watchlist, "600300", 20); // 持仓股（次旧）
    for minute in 101..=109 {
        watch_with(&mut watchlist, &format!("600{minute}"), minute);
    }
    assert_eq!(watchlist.stock_count(), 11);

    let mut plans = PlanBook::default();
    let npc = AccountId(7);
    let plan_id = plans
        .create(PlanOpen {
            account: npc,
            code: code("600001"),
            direction: Side::Buy,
            target: PlanTarget::ShareCount(500),
            opinion: PlanOpinion {
                signal_score_bp: 2500,
                source: OpinionSource::Fundamental,
            },
            confidence_bp: 6000,
            urgency: Urgency::Normal,
            horizon_trading_days: 20,
            created_trading_day: 1,
        })
        .expect("open plan");
    assert!(plans.active_plan(npc, &code("600001")).is_some());

    let mut protected: BTreeSet<_> = [code("600300")].into_iter().collect(); // 持仓
    protected.insert(code("600001")); // 活跃计划

    watchlist.prune(&protected);
    assert!(
        watchlist.stock(&code("600001")).is_some(),
        "活跃计划股票不可淡忘（尽管关注时间最旧）"
    );
    assert!(
        watchlist.stock(&code("600300")).is_some(),
        "持仓股票不可淡忘"
    );
    // 未受保护 9 只 → 保留最新 8 只：600101 淡出；总数 = 2 保护 + 8 = 10。
    assert!(watchlist.stock(&code("600101")).is_none());
    assert_eq!(watchlist.stock_count(), 10);

    // 计划终止：保护解除，600001 变为可淡忘（此时它是最旧的未受保护条目）。
    plans
        .apply(
            plan_id,
            PlanEvent::Terminated {
                reason: engine::TerminationReason::Cancelled,
                trading_day: 2,
            },
        )
        .expect("terminate plan");
    assert!(plans.active_plan(npc, &code("600001")).is_none());
    let held_only: BTreeSet<_> = [code("600300")].into_iter().collect();
    watchlist.prune(&held_only);
    assert!(
        watchlist.stock(&code("600001")).is_none(),
        "计划终止后可淡出"
    );
    assert!(watchlist.stock(&code("600300")).is_some(), "持仓仍受保护");
    // 未受保护 = 600001 + 600102..=600109 共 9 只 → 保留 8 只最新。
    assert_eq!(watchlist.stock_count(), 9);
}

#[test]
fn watchlist_survives_serde_round_trip_and_keeps_pruned_clock() {
    let mut watchlist = PersonalWatchlist::new();
    for (suffix, minute) in [("600101", 120), ("600102", 130), ("600103", 140)] {
        watch_with(&mut watchlist, suffix, minute);
    }
    // 修剪移除条目不回拨列表时钟（latest 停留在 140）。
    watchlist.prune(&BTreeSet::new());

    let json = serde_json::to_string(&watchlist).expect("serialize watchlist");
    let restored: PersonalWatchlist = serde_json::from_str(&json).expect("restore watchlist");
    assert_eq!(restored, watchlist, "存档往返一致");
    assert_eq!(restored.stock_count(), 3);
    assert_eq!(
        restored
            .stock(&code("600103"))
            .expect("entry")
            .last_observed_market_minute,
        140
    );
}

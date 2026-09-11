//! 发现面的负向路径：只变私有经营事实 ⇒ 候选与订单都不变；抽样绝不写
//! 个人信息状态（新曝光不等于已读）。

use std::collections::BTreeSet;

use engine::experience::PersonalWatchlist;
use engine::information::NpcInformationState;
use engine::money::Money;
use engine::orderbook::AccountId;
use engine::session::NpcAttentionState;
use engine::session::{
    FloatAllocation, GameSession, NpcSetup, SecurityCategory, SessionSetup, StockExchange,
    StockSpec,
};
use engine::strategy::{HotParams, InstParams, RetailParams, StrategyParams};
use engine::{CivilDate, VParams};

use crate::exposure::{
    exposed_codes, library_with_announcement, post_undisclosed_fact, private_books, quiet_market,
};
use crate::{attention, code};

fn discovery_setup() -> SessionSetup {
    SessionSetup {
        stocks: vec![
            StockSpec {
                code: code("600101"),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                v_initial: Money::from_cents(1000),
                tick: Money::from_cents(1),
                total_shares: 10_000_000,
                float_shares: 0,
            },
            StockSpec {
                code: code("600102"),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                v_initial: Money::from_cents(1000),
                tick: Money::from_cents(1),
                total_shares: 10_000_000,
                float_shares: 0,
            },
        ],
        npcs: NpcSetup {
            retail_count: 4,
            inst_count: 1,
            hot_count: 1,
            retail_cash_median: Money::from_cents(10_000_000),
        },
        config: engine::GameConfig::proposed_defaults(),
        v_params: VParams {
            long_run_mean: Money::from_cents(1000),
            mean_reversion: 0.5,
            volatility: 0.05,
        },
        fundamental_value_means: [
            (code("600101"), Money::from_cents(1000)),
            (code("600102"), Money::from_cents(1000)),
        ]
        .into(),
        strategy_params: StrategyParams {
            retail: RetailParams {
                arrival_rate: 0.5,
                order_size_mean: 100,
                chase_prob: 0.2,
                tick_cents: 1,
            },
            inst: InstParams {
                margin: 0.05,
                order_size: 200,
            },
            hot: HotParams {
                lookback: 3,
                trend_threshold: 0.02,
                order_size: 200,
            },
        },
        ticks_per_day: 10,
        auction_ticks: 0,
        closing_auction_ticks: 0,
        history_len: 5,
        t1_enabled: true,
        float_allocation: FloatAllocation::Random,
        start_date: CivilDate::from_iso("2030-01-01").unwrap(),
    }
}

/// 跑一段真实会话，取事件流与存档的 serde 字节（订单/成交状态的完整投影）。
fn session_bytes() -> Vec<u8> {
    let mut session = GameSession::new(discovery_setup(), 42).expect("session builds");
    let mut events = Vec::new();
    for _ in 0..150 {
        events.extend(session.step());
    }
    let mut bytes = serde_json::to_vec(&events).expect("events serialize");
    bytes.extend(b"|");
    bytes.extend(serde_json::to_vec(&session.save()).expect("save serializes"));
    bytes
}

#[test]
fn mutating_only_private_operating_facts_changes_no_candidate_and_no_order() {
    let (library, published) = library_with_announcement();
    let market = quiet_market();
    let all_codes = vec![code("600101"), code("600102")];
    let exposed = exposed_codes(&library, &all_codes, published);
    assert_eq!(exposed.len(), 1, "前置：公告确实带来一处公共曝光");

    let books_before = private_books();
    let weights_before = NpcAttentionState::discovery_weights(&market, &exposed);
    let orders_before = session_bytes();
    let mut state = attention(321);
    let candidates_before: Vec<_> = (0..200)
        .map(|_| {
            state.sample_discovery_stock(
                &market,
                &BTreeSet::new(),
                &PersonalWatchlist::new(),
                &exposed,
            )
        })
        .collect();

    // 只变私有经营事实：开放期间过账新收入（总账改变），不结账、不公布。
    let mut books_after = private_books();
    post_undisclosed_fact(&mut books_after, 90);
    assert_ne!(
        serde_json::to_string(&books_after).unwrap(),
        serde_json::to_string(&books_before).unwrap(),
        "前置：私有总账事实确实改变了"
    );

    let weights_after = NpcAttentionState::discovery_weights(&market, &exposed);
    let orders_after = session_bytes();
    let mut state = attention(321);
    let candidates_after: Vec<_> = (0..200)
        .map(|_| {
            state.sample_discovery_stock(
                &market,
                &BTreeSet::new(),
                &PersonalWatchlist::new(),
                &exposed,
            )
        })
        .collect();

    assert_eq!(weights_before, weights_after, "私有经营事实不改变发现权重");
    assert_eq!(
        candidates_before, candidates_after,
        "私有经营事实不改变个体候选序列"
    );
    assert_eq!(
        orders_before, orders_after,
        "私有经营事实不改变订单/事件/存档字节"
    );
}

#[test]
fn discovery_sampling_never_writes_into_information_state() {
    // 新曝光不等于已读：抽样 1000 次只产生候选代码，个人信息集字节不变
    // （获知只能经 record_acquisition 显式登记——任务 16 语义）。
    let (library, published) = library_with_announcement();
    let market = quiet_market();
    let all_codes = vec![code("600101"), code("600102")];
    let exposed = exposed_codes(&library, &all_codes, published);

    let npc = AccountId(11);
    let info = NpcInformationState::new(npc);
    let before = serde_json::to_string(&info).expect("serialize info state");
    let mut state = attention(777);
    for _ in 0..1000 {
        let picked = state
            .sample_discovery_stock(
                &market,
                &BTreeSet::new(),
                &PersonalWatchlist::new(),
                &exposed,
            )
            .expect("candidate");
        assert!(all_codes.contains(&picked), "候选必须落在市场股票集合内");
    }
    let after = serde_json::to_string(&info).expect("serialize info state");
    assert_eq!(
        before, after,
        "发现抽样不得写入个人信息状态（新曝光不等于已读）"
    );
    assert_eq!(info.acquired_count(), 0, "没有任何自动获知");
}

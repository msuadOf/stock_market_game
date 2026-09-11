//! 关注列表的负向路径：时间回拨/未来观察的类型化拒绝、恢复边界不一致拒绝、
//! cap 超限时受保护股票（持仓 ∪ 活跃计划）绝不被驱逐。

use std::collections::BTreeSet;

use engine::experience::{PersonalWatchlist, WatchlistError};

use super::super::code;

#[test]
fn record_attention_rejects_future_observations() {
    let mut watchlist = PersonalWatchlist::new();
    let error = watchlist
        .record_attention(&code("600101"), 100, 99)
        .expect_err("观察分钟超过当前权威分钟必须拒绝");
    assert!(matches!(
        error,
        WatchlistError::ObservationInFuture {
            attempted: 100,
            now: 99
        }
    ));
    assert!(
        watchlist.stock(&code("600101")).is_none(),
        "被拒绝的观察不得留下任何状态"
    );
}

#[test]
fn record_attention_rejects_time_going_backwards() {
    let mut watchlist = PersonalWatchlist::new();
    watchlist
        .record_attention(&code("600101"), 100, 100)
        .expect("first attention");
    watchlist
        .record_attention(&code("600102"), 120, 120)
        .expect("later attention on another stock");

    // 列表时钟已到 120：任何股票的观察都不允许回拨。
    let error = watchlist
        .record_attention(&code("600101"), 119, 120)
        .expect_err("早于列表最近关注分钟必须拒绝");
    assert!(matches!(
        error,
        WatchlistError::TimeWentBackwards {
            attempted: 119,
            last: 120
        }
    ));
    // 被拒绝的回拨不得改写已有条目。
    assert_eq!(
        watchlist
            .stock(&code("600101"))
            .expect("entry survives rejection")
            .last_observed_market_minute,
        100
    );
}

#[test]
fn cap_overflow_never_evicts_held_or_active_plan_stocks() {
    // 10 只受保护股票 + 6 只未受保护：受保护集合不受 8 上限约束，全部保留；
    // 未受保护 6 ≤ 8 也全保留——任何条目都不因 cap 超限被驱逐出保护集合。
    let mut watchlist = PersonalWatchlist::new();
    for index in 0..10u64 {
        watchlist
            .record_attention(&code(&format!("60010{index}")), 10 + index, 10 + index)
            .expect("protected attention");
    }
    for index in 0..6u64 {
        watchlist
            .record_attention(&code(&format!("60020{index}")), 100 + index, 100 + index)
            .expect("unprotected attention");
    }
    assert_eq!(watchlist.stock_count(), 16);

    let mut protected: BTreeSet<_> = (0..10u64)
        .map(|index| code(&format!("60010{index}")))
        .collect();
    protected.insert(code("600201")); // 既有持仓也有活跃计划的股票
    watchlist.prune(&protected);
    for index in 0..10u64 {
        assert!(
            watchlist.stock(&code(&format!("60010{index}"))).is_some(),
            "受保护股票 {index} 不得被驱逐"
        );
    }
    assert!(
        watchlist.stock(&code("600201")).is_some(),
        "持仓/活跃计划股票不得被驱逐"
    );
    assert_eq!(
        watchlist.stock_count(),
        16,
        "受保护条目在 8 上限之外，未受保护未超限：无驱逐"
    );

    // 未受保护超过 8 只时，驱逐只落在未受保护条目上。
    let mut crowded = PersonalWatchlist::new();
    for index in 0..12u64 {
        crowded
            .record_attention(&code(&format!("60030{index:02}")), 200 + index, 200 + index)
            .expect("attention");
    }
    let protected: BTreeSet<_> = [code("6003000"), code("6003011")].into_iter().collect();
    crowded.prune(&protected);
    assert!(crowded.stock(&code("6003000")).is_some(), "最旧但受保护");
    assert!(crowded.stock(&code("6003011")).is_some(), "最新且受保护");
    assert_eq!(crowded.stock_count(), 10, "2 受保护 + 8 条最新未受保护");
}

#[test]
fn restore_rejects_entries_beyond_the_list_attention_clock() {
    // 篡改存档：条目分钟 50 > 列表时钟 40 ⇒ 恢复边界显式拒绝，不静默截断。
    let json = r#"{
        "stocks": {
            "600101": { "last_observed_market_minute": "50" }
        },
        "latest_attention_minute": "40"
    }"#;
    let error =
        serde_json::from_str::<PersonalWatchlist>(json).expect_err("条目分钟越过列表时钟必须拒绝");
    assert!(
        error.to_string().contains("inconsistent watchlist state"),
        "错误必须显式指出恢复不一致：{error}"
    );

    // 合法形状（条目 ≤ 列表时钟）照常恢复。
    let legal = r#"{
        "stocks": {
            "600101": { "last_observed_market_minute": "40" }
        },
        "latest_attention_minute": "50"
    }"#;
    let restored: PersonalWatchlist = serde_json::from_str(legal).expect("合法存档必须恢复");
    assert_eq!(restored.stock_count(), 1);
    assert_eq!(
        restored
            .stock(&code("600101"))
            .expect("entry")
            .last_observed_market_minute,
        40
    );
}

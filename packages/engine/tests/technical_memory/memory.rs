//! 个人价格记忆金样：本人所见锚点、公开历史读取事件与驱逐排序。

use std::collections::BTreeSet;

use engine::experience::PersonalPriceMemory;

use super::{code, price};

#[test]
fn personal_observations_record_first_last_and_high_low_since_first_observation() {
    let mut memory = PersonalPriceMemory::default();
    let stock = code("600101");
    memory.observe_price(&stock, price(1000), 100).unwrap();
    memory.observe_price(&stock, price(1200), 200).unwrap();
    memory.observe_price(&stock, price(800), 300).unwrap();

    let entry = memory.stock(&stock).unwrap();
    assert_eq!(entry.first_observed_minute, 100);
    assert_eq!(entry.first_observed_price, price(1000));
    assert_eq!(entry.last_observed_minute, 300);
    assert_eq!(entry.last_observed_price, price(800));
    // 已观察高低只来自本人见过的价格，时间窗 = [首次, 最近] 观察分钟。
    assert_eq!(entry.observed_high, price(1200));
    assert_eq!(entry.observed_low, price(800));
    assert_eq!(entry.last_touched_minute, 300);
    assert_eq!(entry.last_public_history_read_minute, None);
    assert_eq!(entry.public_history_read_count, 0);
}

#[test]
fn public_history_read_is_recorded_without_overwriting_personal_anchors() {
    // 本人先见 1000/1200/800；公开历史里存在更极端的 2000/500（首次观察之前）。
    // 专业技术分析主动读取公开历史：读取行为被记录（来源=公开历史读取、时间戳），
    // 但不把历史极值冒充成本人亲历——锚点与已观察高低保持本人所见。
    let mut memory = PersonalPriceMemory::default();
    let stock = code("600101");
    memory.observe_price(&stock, price(1000), 100).unwrap();
    memory.observe_price(&stock, price(1200), 200).unwrap();
    memory.observe_price(&stock, price(800), 300).unwrap();

    memory.record_public_history_read(&stock, 400).unwrap();

    let entry = memory.stock(&stock).unwrap();
    assert_eq!(entry.last_public_history_read_minute, Some(400));
    assert_eq!(entry.public_history_read_count, 1);
    assert_eq!(entry.observed_high, price(1200));
    assert_eq!(entry.observed_low, price(800));
    assert_eq!(entry.last_observed_minute, 300);
    assert_eq!(entry.last_observed_price, price(800));
    assert_eq!(entry.first_observed_minute, 100);
    // 读取也构成一次真实接触，参与最近接触时间排序。
    assert_eq!(entry.last_touched_minute, 400);

    memory.record_public_history_read(&stock, 500).unwrap();
    assert_eq!(memory.stock(&stock).unwrap().public_history_read_count, 2);
}

#[test]
fn prune_keeps_protected_and_eight_most_recent_unheld_stocks() {
    // 持仓股最早接触（minute 0）仍被保护；10 个未持仓股按最近接触保留 8 个，
    // 最早两个被驱逐。上限复用 MAX_UNHELD_WATCHLIST_STOCKS = 持仓 + 8 未持仓。
    let mut memory = PersonalPriceMemory::default();
    let held = code("600001");
    memory.observe_price(&held, price(1000), 0).unwrap();
    for index in 1..=10_u64 {
        memory
            .observe_price(&code(&format!("6001{index:02}")), price(1000), index)
            .unwrap();
    }

    let protected = BTreeSet::from([held.clone()]);
    memory.prune(&protected);

    assert!(memory.stock(&held).is_some());
    assert!(memory.stock(&code("600103")).is_some());
    assert!(memory.stock(&code("600110")).is_some());
    assert!(memory.stock(&code("600101")).is_none());
    assert!(memory.stock(&code("600102")).is_none());
    assert_eq!(memory.stock_count(), 9);
}

#[test]
fn prune_breaks_last_touched_ties_by_stock_code() {
    // 9 个未持仓股：两个共享最早接触分钟 5，其余 7 个在 6..=12。
    // 同分钟按 StockCode 稳定破同分（与 RetailExperienceState::prune_watchlist
    // 一致：降序 (minute, code)，代码较大者保留），"000001" 被驱逐。
    let mut memory = PersonalPriceMemory::default();
    memory
        .observe_price(&code("000001"), price(1000), 5)
        .unwrap();
    memory
        .observe_price(&code("000002"), price(1000), 5)
        .unwrap();
    for minute in 6..=12_u64 {
        memory
            .observe_price(&code(&format!("6001{minute:02}")), price(1000), minute)
            .unwrap();
    }

    memory.prune(&BTreeSet::new());

    assert!(memory.stock(&code("000001")).is_none());
    assert!(memory.stock(&code("000002")).is_some());
    assert_eq!(memory.stock_count(), 8);
}

#[test]
fn public_read_refreshes_recency_for_eviction() {
    // 9 个未持仓股：X 最早观察（minute 1），Y1..Y8 在 2..=9。X 在 minute 100
    // 主动读取公开历史 → last_touched=100，超过所有 Y；驱逐最旧的 Y1 而非 X。
    let mut memory = PersonalPriceMemory::default();
    let earliest = code("600100");
    memory.observe_price(&earliest, price(1000), 1).unwrap();
    for minute in 2..=9_u64 {
        memory
            .observe_price(&code(&format!("6001{minute:02}")), price(1000), minute)
            .unwrap();
    }
    memory.record_public_history_read(&earliest, 100).unwrap();

    memory.prune(&BTreeSet::new());

    assert!(memory.stock(&earliest).is_some());
    assert!(memory.stock(&code("600102")).is_none());
    assert_eq!(memory.stock_count(), 8);
}

#[test]
fn price_memory_serde_round_trip_preserves_state() {
    let mut memory = PersonalPriceMemory::default();
    let stock = code("600101");
    memory.observe_price(&stock, price(1000), 120).unwrap();
    memory.observe_price(&stock, price(1300), 240).unwrap();
    memory.record_public_history_read(&stock, 300).unwrap();

    let serialized = serde_json::to_string(&memory).unwrap();
    let restored: PersonalPriceMemory = serde_json::from_str(&serialized).unwrap();
    assert_eq!(restored, memory);
}

//! 翻转迁移公共构造（双路径驱动器）。
#![allow(dead_code)]

use engine::money::Money;
use engine::runtime::drive::RuntimeDriver;
use engine::session::{NpcSetup, SessionSetup};
use engine::{
    Event, FloatAllocation, GameSession, SecurityCategory, StockCode, StockExchange, StockSpec,
    SIMULATION_POLICY_ID,
};

pub fn setup_1stock() -> SessionSetup {
    SessionSetup {
        stocks: vec![StockSpec {
            code: StockCode("600200".to_owned()),
            exchange: StockExchange::Shanghai,
            initial_price: Money::from_cents(1000),
            category: SecurityCategory::MainBoard,
            limit_pct: 0.10,
            tick: Money::from_cents(1),
            total_shares: 1_000_000,
            float_shares: 100_000,
        }],
        company_operations: None,
        groups: Vec::new(),
        npcs: NpcSetup {
            retail_count: 3,
            inst_count: 0,
            hot_count: 0,
            retail_cash_median: Money::from_cents(1_000_000),
        },
        config: engine::config::GameConfig::proposed_defaults(),
        strategy_params: engine::strategy::StrategyParams {
            retail: engine::strategy::RetailParams {
                arrival_rate: 0.0,
                order_size_mean: 100,
                chase_prob: 0.0,
            },
            inst: engine::strategy::InstParams {
                margin: 0.01,
                order_size: 100,
            },
            hot: engine::strategy::HotParams {
                lookback: 2,
                trend_threshold: 0.01,
                order_size: 100,
            },
        },
        ticks_per_day: 240,
        auction_ticks: 15,
        closing_auction_ticks: 3,
        history_len: 16,
        t1_enabled: true,
        float_allocation: FloatAllocation::random(),
        start_date: engine::calendar::CivilDate::from_ymd(2026, 1, 5).expect("合法日期"),
        simulation_policy_id: SIMULATION_POLICY_ID.to_owned(),
    }
}

/// 驱动 N tick 收集全部事件（runtime 路径）。
pub fn runtime_events(ticks: u64, seed: u64) -> (GameSession, Vec<Event>) {
    let mut session = GameSession::new(setup_1stock(), seed).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&session).expect("驱动器");
    driver.adopt_session_seq(&session);
    let mut all = Vec::new();
    let tpd = 240u64;
    for t in 1..=ticks {
        let events = driver
            .step_frame_equivalent(&mut session)
            .expect("runtime tick（含安装）");
        all.extend(events);
        driver
            .install_market_views(&mut session)
            .expect("行情/价格历史安装");
        driver.install_candles(&mut session).expect("K 安装");
        // 日界：runtime DayEnd（清簿/组日翻转）+ 会话交易日推进 +
        // 自然日推进（编排）+清簿撤单事件。
        if t % tpd == 0 {
            driver
                .day_end_with_runtime_market_day(&mut session)
                .unwrap_or_else(|e| panic!("日结编排失败（t={t}）：{e}"));
            all.extend(driver.take_drained_events());
        }
    }
    (session, all)
}

/// 旧管线同种子驱动。
pub fn pipeline_events(ticks: u64, seed: u64) -> (GameSession, Vec<Event>) {
    let mut session = GameSession::new(setup_1stock(), seed).expect("会话构造");
    let mut all = Vec::new();
    for t in 1..=ticks {
        let frame = session.step_frame().expect("旧管线 tick");
        all.extend(frame.facts.into_iter().map(|f| f.event));
        // 日界：旧管线的自然日推进是宿主责任（ADR-0025）——跨日驱动时
        // 显式调用（与 runtime 编排自动日结对齐口径）。
        if t % 240 == 0 {
            session.end_civil_day().expect("旧管线自然日推进");
        }
    }
    (session, all)
}

/// 逐订单终结闭环（Sol 二审修复集 4 + 三审漏检修复）：事件面可判定的
/// 真实闭环——
/// (1) 每条 OrderCanceled 的 (account, code, id) 必须先有同键
///     OrderAccepted，且撤销余量 ≤ 受理余量；
/// (2) 受理 id 全程不重复；
/// (3) **同一受理 id 的撤单事件恰一条**（Sol 三审反例：同账户受理
///     A/B 各 100、撤 A 两次、B 未终结——按账户聚合 200−200=0 恒过；
///     逐单去重后第二次撤 A 即红，B 漏终结由 (4) 的聚合差暴露）；
/// (4) 每账户+股的（受理总量−撤销总量）≤ 该账户该股被 Trade 触碰
///     的总量（挂簿量的成交消耗必有 Trade 腿触碰持有人——立即成
///     交腿只增不减触碰量，不等号方向恒安全）。
pub fn assert_order_lifecycle_closure(events: &[Event]) {
    use std::collections::BTreeMap;
    // (account, code, id) → 受理余量。
    let mut accepted: BTreeMap<(u64, u32, u64), u32> = BTreeMap::new();
    // (account, code, id) → 已见撤单（恰一条）。
    let mut canceled: BTreeMap<(u64, u32, u64), ()> = BTreeMap::new();
    // (account, code) → (受理总量, 撤销总量)。
    let mut per_acct: BTreeMap<(u64, u32), (u64, u64)> = BTreeMap::new();
    // (account, code) → Trade 触碰总量。
    let mut touching: BTreeMap<(u64, u32), u64> = BTreeMap::new();
    for e in events {
        match e {
            Event::OrderAccepted {
                account,
                code,
                id,
                remaining_qty,
                ..
            } => {
                let key = (account.0, code.0, id.0);
                assert!(
                    accepted.insert(key, *remaining_qty).is_none(),
                    "受理 id 不得重复：{key:?}"
                );
                per_acct.entry((account.0, code.0)).or_default().0 += u64::from(*remaining_qty);
            }
            Event::OrderCanceled {
                account,
                code,
                id,
                remaining_qty,
                ..
            } => {
                let key = (account.0, code.0, id.0);
                let acc_rem = *accepted
                    .get(&key)
                    .unwrap_or_else(|| panic!("撤销必须先有同键受理：{key:?}"));
                assert!(
                    *remaining_qty <= acc_rem,
                    "撤销余量超受理余量：{key:?} cancel={remaining_qty} accept={acc_rem}"
                );
                assert!(
                    canceled.insert(key, ()).is_none(),
                    "同一受理 id 的撤单事件必须恰一条（Sol 三审反例——逐单终结不重复）：{key:?}"
                );
                per_acct.entry((account.0, code.0)).or_default().1 += u64::from(*remaining_qty);
            }
            Event::Trade {
                maker,
                taker,
                code,
                qty,
                ..
            } => {
                *touching.entry((maker.0, code.0)).or_default() += u64::from(*qty);
                *touching.entry((taker.0, code.0)).or_default() += u64::from(*qty);
            }
            _ => {}
        }
    }
    for (key, (acc_sum, canc_sum)) in per_acct {
        let touch = touching.get(&key).copied().unwrap_or(0);
        assert!(
            acc_sum >= canc_sum && acc_sum - canc_sum <= touch,
            "账户/股 {key:?} 闭环破裂：受理{acc_sum}−撤销{canc_sum}={} 超过成交触碰{touch}",
            acc_sum - canc_sum
        );
    }
}

#[cfg(test)]
mod closure_tests {
    use super::assert_order_lifecycle_closure;
    use engine::{Event, Money, OrderId, StockId, AccountId};
    use engine::orderbook::Side;

    fn accept(seq: u64, id: u64, rem: u32) -> Event {
        Event::OrderAccepted {
            seq,
            account: AccountId(1),
            code: StockId(0),
            id: OrderId(id),
            side: Side::Buy,
            price: Money::from_cents(1000),
            remaining_qty: rem,
        }
    }

    fn cancel(seq: u64, id: u64, rem: u32) -> Event {
        Event::OrderCanceled {
            seq,
            account: AccountId(1),
            code: StockId(0),
            id: OrderId(id),
            remaining_qty: rem,
        }
    }

    // Sol 三审反例：同账户受理 A、B 各 100 股，撤 A 两次、B 完全未
    // 终结——按账户聚合的旧口径掩盖（200−200=0 ≤ touch 0 恒过）。
    // 逐订单口径必须红：同一 id 的撤单事件不得重复。
    #[test]
    fn sol3r_double_cancel_same_id_is_rejected() {
        let events = vec![
            accept(1, 1, 100),
            accept(2, 2, 100),
            cancel(3, 1, 100),
            cancel(4, 1, 100),
        ];
        let result = std::panic::catch_unwind(|| assert_order_lifecycle_closure(&events));
        assert!(result.is_err(), "同 id 双撤单必须被闭环断言拒绝（Sol 反例）");
    }

    // 正例：每张受理单恰一条撤单（无成交场景）——聚合差 0 ≤ 触碰 0。
    #[test]
    fn sol3r_one_cancel_per_order_passes() {
        let events = vec![
            accept(1, 1, 100),
            accept(2, 2, 100),
            cancel(3, 1, 100),
            cancel(4, 2, 100),
        ];
        assert_order_lifecycle_closure(&events);
    }

    // 反例变体：撤单余量按单不超受理（A 撤 150 > 受理 100）。
    #[test]
    fn sol3r_cancel_exceeding_accept_is_rejected() {
        let events = vec![accept(1, 1, 100), cancel(2, 1, 150)];
        let result = std::panic::catch_unwind(|| assert_order_lifecycle_closure(&events));
        assert!(result.is_err(), "撤单余量超受理必须被拒绝");
    }
}

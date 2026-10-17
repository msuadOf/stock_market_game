//! 语义再锚定（sol-103 第三类）：从将随旧管线退役的对账测试
//! （runtime_flip_batch1-5 等）提取 A 股语义断言，改写为 **runtime 单侧
//! 独立测试**（手算 oracle/恒等式，不依赖旧管线输出作参照——删旧引擎
//! 前的安全网）。对照 dying-tests-census.md §2 的 27 条清单。
//!
//! 覆盖映射（census 条目号）：
//! - seq/事件面 #27、日结 #16/#17、守恒 #13/#14/#15 →
//!   `seq_vocabulary_day_and_conservation_anchors`
//! - T+1 #1 → `t1_today_buy_frozen_and_day_end_unfreeze`（batch4#3 再锚）
//! - 日结 #18 → `day_end_drain_closes_all_resting`；#9 →
//!   `exact_closure_on_no_trade_fixture`
//! - 日结 #19 → `civil_day_end_failure_rolls_back_and_retries_once`
//! - 日结 #20 → `second_day_auction_window_resumes`
//! - 竞价 #21-#25（+#26 收盘相位）→ `auction_window_gates_…`
//! - 自成交 #6 → `trades_never_self_cross`
//! - 费用 #3/#4/#5 → `fee_settlement_formulas_…`（#3/#4 原场景为同
//!   卖方同拍累计审计；此处以买方单多腿累计补差换向承载同一
//!   「每单累计+下限一次」机制，卖方分腿费用同断言面覆盖）
//! - 生命周期 #12（执行序无关性·runtime 形态）→
//!   `scheduling_invariants_hold_across_runs`
//! （#2/#7/#8/#10/#11 已由常驻测试独立承载：desk_semantics_tests
//! t1_frozen_shares_cannot_sell_same_day 与 p1_auction_self_trade_*、
//! flip_helpers::assert_order_event_invariants+closure_tests、字段域与
//! orders_expire_at_ttl——不在本文件重复；#7 连续竞价自成交终止在
//! desk_semantics_tests r07 新增。）

mod common;

use common::flip_helpers::{
    assert_order_event_invariants, runtime_events, runtime_events_with_driver, setup_1stock,
};
use engine::runtime::drive::RuntimeDriver;
use engine::{Event, GameSession};

fn cash_total(s: &GameSession) -> i64 {
    (0..s.account_count() as u64)
        .filter_map(|i| s.account(engine::AccountId(i)).map(|a| a.cash().cents()))
        .sum()
}

/// 双腿实收费用合计（成交事实累计——独立于费用桶读面的第二来源）。
fn fees_from_trades(driver: &RuntimeDriver) -> i64 {
    driver
        .world_report_log()
        .iter()
        .flat_map(|r| r.desk_trades.iter())
        .map(|f| f.maker_fees_cents + f.taker_fees_cents)
        .sum()
}

/// #27+#17：事件 seq 全程严格单调、不重号、类型落在引擎合法词表内、
/// 事件面非空（跨日 245 tick——seq 不因日结重置）。
/// #16：跨一交易日 day 恰 +1。
/// #13/#14：守恒恒等式 Σ现金 + Σ成交事实费用 == 初始总量；#15：现金非负。
#[test]
fn seq_vocabulary_day_and_conservation_anchors() {
    let initial = GameSession::new(setup_1stock(), 601).expect("基线");
    let base = cash_total(&initial);
    let (session, driver, events) = runtime_events_with_driver(245, 601);
    // #16：day 恰 +1（240 tick + 日结）。
    assert_eq!(session.day(), 1, "跨一交易日 day 恰 +1");
    assert!(!events.is_empty(), "事件面非空（无静默吞事件）");
    let mut prev: Option<u64> = None;
    let mut seen = std::collections::BTreeSet::new();
    for e in &events {
        let seq = match e {
            Event::Trade { seq, .. }
            | Event::OrderAccepted { seq, .. }
            | Event::OrderCanceled { seq, .. }
            | Event::IntentRejected { seq, .. }
            | Event::AuctionTick { seq, .. }
            | Event::AuctionCompleted { seq, .. }
            | Event::PriceTick { seq, .. } => *seq,
            other => panic!("事件类型在引擎合法词表外：{other:?}"),
        };
        assert!(seen.insert(seq), "seq 不重号：{seq}");
        if let Some(p) = prev {
            assert!(seq > p, "seq 严格单调（跨日结不重置不回退）：{seq} <= {p}");
        }
        prev = Some(seq);
    }
    // #13/#14：Σ现金 + Σ双腿实收费用 == 初始总量（无铸造通道）。
    let fees = fees_from_trades(&driver);
    assert_eq!(
        cash_total(&session) + fees,
        base,
        "守恒恒等式：Σ现金+Σ成交费用 == 初始（费用 {fees} 分）"
    );
    for i in 0..session.account_count() as u64 {
        if let Some(a) = session.account(engine::AccountId(i)) {
            assert!(a.cash().cents() >= 0, "#15 账户 {i} 现金非负");
        }
    }
}

/// #1（T+1）：首日有当日买入 ⇒ 日结前 frozen_today>0（注入对 100 股，
/// 手算冻结恰 100）；日结后全账户 frozen_today 全零（解冻恰在日界）。
#[test]
fn t1_today_buy_frozen_and_day_end_unfreeze() {
    use common::flip_helpers::inject_crossing_pair_at_1000;
    let mut session = GameSession::new(setup_1stock(), 199).expect("会话");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器");
    driver.adopt_session_seq(&session);
    inject_crossing_pair_at_1000(&mut driver);
    driver
        .step_frame_equivalent(&mut session)
        .expect("成交拍");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    for _ in 2..=240 {
        driver.step_frame_equivalent(&mut session).expect("步进");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    // 玩家账户口径（local 0）：注入对归属确定，NPC 有机买入（他账户）
    // 不影响本 oracle——并发调度下不闪红。
    let frozen_before: u32 = driver.world_accounts_snapshot().expect("日结前快照")[0].1[0]
        .frozen_today
        .iter()
        .sum();
    assert_eq!(
        frozen_before, 100,
        "玩家首日当日买入 100 股 ⇒ 冻结恰 100（手算；>0 前置防空转）"
    );
    driver
        .day_end_with_runtime_market_day(&mut session)
        .expect("日结");
    let frozen_after: u32 = driver
        .world_accounts_snapshot()
        .expect("日结后快照")
        .into_iter()
        .flat_map(|(_, accounts)| accounts)
        .map(|a| a.frozen_today.iter().sum::<u32>())
        .sum();
    assert_eq!(frozen_after, 0, "T+1：日结解冻恰在日界（frozen_today 全零）");
}

/// #18：恰 240 tick（日终点口径）日终清簿后无悬挂挂单——逐单闭环
/// （撤销先受理/余量不超/id 不重/（受理−撤销）≤成交触碰）。
#[test]
fn day_end_drain_closes_all_resting() {
    let (_, events) = runtime_events(240, 602);
    assert_order_event_invariants(&events);
}

/// #9：无成交 fixture 上逐单终结精确核销——每张受理单恰一条终结撤单
/// （键集相等）、撤单余量==受理余量分毫不差（候选序搜索有受理且无
/// 成交的形态）。
#[test]
fn exact_closure_on_no_trade_fixture() {
    use std::collections::BTreeMap;
    let mut events = Vec::new();
    for seed in 300..340u64 {
        let (_, evs) = runtime_events(240, seed);
        let has_accept = evs.iter().any(|e| matches!(e, Event::OrderAccepted { .. }));
        let no_trade = !evs.iter().any(|e| matches!(e, Event::Trade { .. }));
        if has_accept && no_trade {
            events = evs;
            break;
        }
    }
    assert!(
        events.iter().any(|e| matches!(e, Event::OrderAccepted { .. })),
        "候选序 300..340 内存在有受理且无成交形态"
    );
    let mut orders: BTreeMap<(u64, u32, u64), (u32, u32, u32)> = BTreeMap::new();
    let mut trades = 0u64;
    for e in &events {
        match e {
            Event::OrderAccepted {
                account, code, id, remaining_qty, ..
            } => {
                orders.entry((account.0, code.0, id.0)).or_default().0 += *remaining_qty;
            }
            Event::OrderCanceled {
                account, code, id, remaining_qty, ..
            } => {
                let slot = orders.entry((account.0, code.0, id.0)).or_default();
                slot.1 += 1;
                slot.2 += *remaining_qty;
            }
            Event::Trade { .. } => trades += 1,
            _ => {}
        }
    }
    assert_eq!(trades, 0, "前置：无成交（精确口径适用）");
    assert!(!orders.is_empty(), "前置：有受理单");
    for (key, (accepted_rem, cancel_count, canceled_rem)) in &orders {
        assert_eq!(cancel_count, &1, "每张受理单恰一条终结撤单：{key:?}");
        assert_eq!(
            canceled_rem, accepted_rem,
            "无成交 ⇒ 撤单余量==受理余量（逐单核销分毫不差）：{key:?}"
        );
    }
}

/// #19：civil 日结失败全量回滚（会话 day/seq、driver seq、事件缓冲）
/// + 修复后重试恰一次成功（清簿撤单不重复）+ 重试后可续跑。
#[test]
fn civil_day_end_failure_rolls_back_and_retries_once() {
    use std::collections::BTreeSet;
    let mut session = GameSession::new(setup_1stock(), 306).expect("会话");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器");
    driver.adopt_session_seq(&session);
    for _ in 1..=240 {
        driver.step_frame_equivalent(&mut session).expect("步进");
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    // 制造 civil 失败：交易日计数偏离时钟期望（advance 后 5+1≠1）。
    let seq_before = session.current_seq();
    session.rollback_runtime_day_end(5, seq_before);
    let driver_seq_before = driver.next_seq();
    let err = driver
        .day_end_with_runtime_market_day(&mut session)
        .expect_err("civil 失败必须显式报错");
    assert!(!format!("{err:?}").is_empty(), "错误面可辨识");
    assert_eq!(session.day(), 5, "会话 day 回滚");
    assert_eq!(session.current_seq(), seq_before, "会话 seq 回滚");
    assert_eq!(driver.next_seq(), driver_seq_before, "driver seq 回滚");
    assert!(
        driver.take_drained_events().is_empty(),
        "失败尝试不得残留清簿事件"
    );
    session.rollback_runtime_day_end(0, session.current_seq());
    driver
        .day_end_with_runtime_market_day(&mut session)
        .expect("重试日结成功");
    assert_eq!(session.day(), 1, "重试后 day 恰 +1");
    let drained = driver.take_drained_events();
    let mut canceled_ids = BTreeSet::new();
    for e in &drained {
        if let Event::OrderCanceled { id, .. } = e {
            assert!(canceled_ids.insert(*id), "清簿撤单不重复（重试恰一次）：{id:?}");
        }
    }
    driver
        .step_frame_equivalent(&mut session)
        .expect("日结后可续跑");
}

/// #20：第二日开盘竞价窗重新出现 AuctionTick（tick 241+）。
#[test]
fn second_day_auction_window_resumes() {
    let (_, events) = runtime_events(260, 603);
    let day2_auction: Vec<u64> = events
        .iter()
        .filter_map(|e| match e {
            Event::AuctionTick { tick, .. } if *tick > 240 => Some(*tick),
            _ => None,
        })
        .collect();
    assert!(
        !day2_auction.is_empty(),
        "日结后相位重置——第二日开盘竞价窗（tick 241+）：{day2_auction:?}"
    );
}

/// #21-#25（+#26 收盘相位面）：开盘竞价窗事件/相位门禁/申报窗不撮合/
/// 清算在申报窗后/连续窗无竞价事件/收盘竞价相位事件在场。
#[test]
fn auction_window_gates_and_closing_phase_events() {
    let mut session = GameSession::new(setup_1stock(), 604).expect("会话");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器");
    driver.adopt_session_seq(&session);
    let mut per_tick: Vec<(u64, Vec<Event>)> = Vec::new();
    for t in 1..=30u64 {
        let events = driver.step_frame_equivalent(&mut session).expect("步进");
        per_tick.push((t, events));
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    // #25：30 tick 内必有竞价窗事件。
    assert!(
        per_tick
            .iter()
            .flat_map(|(_, es)| es.iter())
            .any(|e| matches!(e, Event::AuctionTick { .. })),
        "前 30 tick 应见开盘竞价事件"
    );
    for (t, events) in &per_tick {
        for e in events {
            if let Event::AuctionTick { phase, .. } | Event::AuctionCompleted { phase, .. } = e {
                // #21：开盘窗（tick≤15）竞价事件 phase=CallAuction。
                if *t <= 15 {
                    assert_eq!(
                        *phase,
                        engine::TradingPhase::CallAuction,
                        "开盘窗（tick {t}）竞价事件 phase"
                    );
                }
                // #23：连续竞价窗（17-236）不得有竞价事件。
                assert!(
                    !(*t > 16),
                    "连续竞价窗（tick {t}）不得有 AuctionTick/Completed：{e:?}"
                );
            }
            if *t <= 9 {
                // #24：申报窗（前 2/3=10 tick）只积累不撮合。
                assert!(
                    !matches!(e, Event::Trade { .. }),
                    "申报窗（tick {t}）内不撮合——零 Trade：{e:?}"
                );
            }
        }
    }
    // #22：开盘清算在申报窗（tick 10）后、域合法。
    let completed: Vec<(u64, Option<i64>, u64)> = per_tick
        .iter()
        .flat_map(|(t, es)| es.iter().map(move |e| (*t, e)))
        .filter_map(|(t, e)| match e {
            Event::AuctionCompleted {
                clearing_price,
                matched_volume,
                phase: engine::TradingPhase::CallAuction,
                ..
            } => Some((t, clearing_price.map(|p| p.cents()), *matched_volume)),
            _ => None,
        })
        .collect();
    assert!(!completed.is_empty(), "30 tick 内必有开盘清算");
    for (t, price, vol) in &completed {
        assert!(*t >= 10, "清算在申报窗后：tick {t}");
        match price {
            Some(p) => {
                assert!((900..=1100).contains(p), "清算价在首日带 [900,1100]：{p}");
                assert!(*vol > 0, "有价必有量");
            }
            None => assert_eq!(*vol, 0, "无交叉 ⇒ 无价无量"),
        }
    }
    // #26：收盘竞价相位事件（跨日驱动：日内 tick 237+ 为收盘窗）。
    let (_, events245) = runtime_events(245, 604);
    assert!(
        events245.iter().any(|e| matches!(
            e,
            Event::AuctionTick {
                phase: engine::TradingPhase::ClosingAuction,
                ..
            }
        )),
        "收盘竞价窗（tick 237+）应有 ClosingAuction 相位事件"
    );
}

/// #6：全部 Trade 的 maker≠taker（A 股自成交限制的事件面投影）。
/// 有机成交（种子 606 实测 4 笔）为主；并发调度使有机成交归零时以
/// 确定性注入对兜底（断言面不空转、不因调度抖动变红）。
#[test]
fn trades_never_self_cross() {
    use common::flip_helpers::inject_crossing_pair_at_1000;
    let (_, events) = runtime_events(245, 606);
    let mut trades: Vec<Event> = events
        .into_iter()
        .filter(|e| matches!(e, Event::Trade { .. }))
        .collect();
    if trades.is_empty() {
        // 本轮调度无有机成交——注入对手对（tick 10 竞价完成拍必成交）。
        let mut session = GameSession::new(setup_1stock(), 199).expect("会话");
        let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器");
        driver.adopt_session_seq(&session);
        inject_crossing_pair_at_1000(&mut driver);
        for _ in 0..12 {
            let evs = driver.step_frame_equivalent(&mut session).expect("步进");
            trades.extend(evs.into_iter().filter(|e| matches!(e, Event::Trade { .. })));
            driver.install_market_views(&mut session).expect("行情");
            driver.install_candles(&mut session).expect("K");
        }
    }
    assert!(
        !trades.is_empty(),
        "成交面必须非空（有机或注入——否则 maker≠taker 断言空转）"
    );
    for t in &trades {
        let Event::Trade { maker, taker, .. } = t else {
            unreachable!()
        };
        assert_ne!(maker, taker, "自成交限制：成交双腿账户不同");
    }
}

/// #3/#4/#5（费用）：同一手算 oracle 三面——
/// - 买方结算 = 毛额 + 佣金（max(rated, 500 分下限)）+ 过户（#5）；
/// - 卖方 = 毛额 −（佣金 + 印花 0.5‰ 仅卖方 + 过户）；
/// - 多腿费用：佣金下限**逐挂单**一次（首腿吃满）；同单多腿按累计
///   rated 补差（#3/#4 累计费用审计的事件面）。
///
/// 手算（对价 3_000 股@1000=3_000_000 分，两笔卖单 100+2900 FIFO）：
/// - 买方（单张 3000 股）：佣金累计 max(750,500)=750、过户 30 ⇒
///   实付 3_000_780；边际腿 1=500+1（下限吃满）、腿 2=250+29（补差）；
/// - 卖腿 1（单张 100 股）：佣金 max(25,500)=500、印花 50、过户 1；
/// - 卖腿 2（单张 2900 股）：佣金 max(725,500)=725、印花 1_450、
///   过户 29 ⇒ 卖方合计费用 2_755、净得 2_997_245。
#[test]
fn fee_settlement_formulas_with_minimum_commission_and_marginal_split() {
    let mut session = GameSession::new(setup_1stock(), 199).expect("会话");
    let player_cash0 = session
        .account(engine::AccountId(0))
        .expect("玩家")
        .cash()
        .cents();
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器");
    driver.adopt_session_seq(&session);
    let seller_shares0 = driver.world_accounts_snapshot().expect("初始快照")[0].1[1].shares[0];
    assert!(seller_shares0 >= 3_000, "前置：卖方 float 持仓充足（实测 {seller_shares0}）");
    let sell = |qty: u32| engine::ReceiptBearingIntent {
        owner: engine::AccountId(1),
        intent: engine::Intent::PlaceLimit {
            code: engine::StockId(0),
            side: engine::Side::Sell,
            price: engine::strategy::LimitPrice::Fixed(engine::Money::from_cents(1_000)),
            qty,
        },
        account_ordinal: 0,
        stock_ordinal: 0,
    };
    driver
        .inject_player_intents(
            &[
                sell(100),
                sell(2_900),
                engine::ReceiptBearingIntent {
                    owner: engine::AccountId(0),
                    intent: engine::Intent::PlaceLimit {
                        code: engine::StockId(0),
                        side: engine::Side::Buy,
                        price: engine::strategy::LimitPrice::Fixed(engine::Money::from_cents(
                            1_000,
                        )),
                        qty: 3_000,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                },
            ],
            &driver
                .world_public_views()
                .iter()
                .map(|v| v.daily_upper_limit)
                .collect::<Vec<_>>(),
        )
        .expect("注入");
    // 竞价窗内申报积累、完成拍（tick 10）撮合——驱动 12 拍全收集。
    let mut all_events = Vec::new();
    for _ in 0..12 {
        let evs = driver.step_frame_equivalent(&mut session).expect("步进");
        all_events.extend(evs);
        driver.install_market_views(&mut session).expect("行情");
        driver.install_candles(&mut session).expect("K");
    }
    assert!(
        all_events
            .iter()
            .filter(|e| matches!(e, Event::Trade { .. }))
            .count()
            >= 2,
        "两腿 FIFO 拆分（前置）"
    );
    let accounts = driver.world_accounts_snapshot().expect("组快照");
    let player = &accounts[0].1[0];
    let seller = &accounts[0].1[1];
    assert_eq!(
        player.cash,
        player_cash0 - 3_000_780,
        "#5 买方=毛额 3_000_000+佣金 750+过户 30"
    );
    assert_eq!(player.shares[0], 3_000, "玩家 3000 股");
    assert_eq!(
        seller.shares[0],
        seller_shares0 - 3_000,
        "卖方出 3000 股（初始 {seller_shares0}）"
    );
    // #3/#4：成交事实的逐腿实收费用（挂单费用审计的事件面；跨记录
    // 检索注入对的两腿——qty 100/2900 组合在本 fixture 唯一）。
    // 以 taker=玩家（local 0）过滤本注入对的两腿——NPC 有机成交
    // （qty 碰撞）不会误配（并发调度下不闪红）。
    let legs: Vec<(u32, i64, i64)> = driver
        .world_report_log()
        .iter()
        .flat_map(|r| r.desk_trades.iter())
        .filter(|f| f.taker == (0, 0))
        .map(|f| (f.qty, f.maker_fees_cents, f.taker_fees_cents))
        .collect();
    let leg_100 = legs
        .iter()
        .find(|(q, _, _)| *q == 100)
        .expect("卖腿 100 在事实流");
    let leg_2900 = legs
        .iter()
        .find(|(q, _, _)| *q == 2_900)
        .expect("卖腿 2900 在事实流");
    assert_eq!(leg_100.2, 501, "买腿 1：佣金下限 500+过户 1（单张累计吃满下限）");
    assert_eq!(leg_100.1, 551, "卖腿 1（100 股单）：佣金下限 500+印花 50+过户 1");
    assert_eq!(leg_2900.2, 279, "买腿 2：佣金补差 250（累计 rated 750−已收 500）+过户 29");
    assert_eq!(
        leg_2900.1, 2_204,
        "卖腿 2（2900 股单）：佣金 max(725,500)=725+印花 1_450+过户 29"
    );
}

/// #12（执行序无关性·runtime 形态）：std-thread 并发下调度序不可控，
/// 但**不变量**必须在任意调度下保持——同种子三跑各自满足守恒恒等式
/// 与逐单闭环。逐单闭环取 240 拍清簿口径（受理−撤销≤成交触碰仅在
/// 日终 Drain 后无合法在簿豁免——审计模式 c-2 的登记边界）。run 间
/// 不要求逐位一致（既定口径），只要求不变量成立。
#[test]
fn scheduling_invariants_hold_across_runs() {
    let initial = GameSession::new(setup_1stock(), 606).expect("基线");
    let base = cash_total(&initial);
    for run in 0..3 {
        let (session, driver, events) = runtime_events_with_driver(240, 606);
        assert_order_event_invariants(&events);
        let fees = fees_from_trades(&driver);
        assert_eq!(
            cash_total(&session) + fees,
            base,
            "run {run}：Σ现金+Σ费用==初始（费用 {fees}）"
        );
    }
}

/// sol-103 上报项 2（修复·红测试先行）：玩家 PlaceMarket 意图经
/// step() 默认路径不得静默丢弃（铁律二）。竞价相位面：市价申报在
/// 集合竞价只收限价 ⇒ 必须显式 IntentRejected(AuctionLimitOrderRequired)
/// ——此前 route_external_intents 对 PlaceMarket 静默 continue（无事件、
/// 无错误、无退款回执）。
#[test]
fn player_market_intent_is_never_silently_dropped_by_step() {
    let mut session = GameSession::new(setup_1stock(), 701).expect("会话");
    session
        .enqueue_player_intent(
            engine::AccountId(0),
            engine::Intent::PlaceMarket {
                code: engine::StockId(0),
                side: engine::Side::Buy,
                qty: 100,
            },
        )
        .expect("入队");
    let events = session.step().expect("步进（tick 1=开盘竞价申报窗）");
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::IntentRejected {
                account: engine::AccountId(0),
                reason: engine::RejectionReason::AuctionLimitOrderRequired,
                ..
            }
        )),
        "竞价相位市价申报必须显式拒收回执（不静默丢弃）：{events:?}"
    );
}

/// sol-103 上报项 2（修复·路由真接线）：连续相位玩家市价买入经
/// route_external_intents 进入 runtime 世界——按涨跌停价保护、逐档
/// 即成、余量不进簿（R16 玩家路径）。fixture：auction_ticks=0（tick 1
/// 即连续相位、NPC 计划未启动——seed 199 前 12 拍实测安静），对手盘
/// 与市价单**同批注入**（批内 FIFO：卖先挂、市价买随后吃单——确定性）。
/// 手算 oracle（连续竞价成交价=maker 价 1000）：买方实付 100_000+500+1
/// =100_501、余量 200 全额退款（买盘空）。附带锚定玩家符号价 Lowest
/// 卖的正向路由（解析=跌停 900——此前同函数把 Lowest 误路由 Highest）。
#[test]
fn player_market_and_symbolic_intents_route_into_runtime_world() {
    let mut setup = setup_1stock();
    setup.auction_ticks = 0;
    let mut session = GameSession::new(setup, 199).expect("会话");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器");
    driver.adopt_session_seq(&session);
    let player_cash0 = session
        .account(engine::AccountId(0))
        .expect("玩家")
        .cash()
        .cents();
    let upper: Vec<i64> = driver
        .world_public_views()
        .iter()
        .map(|v| v.daily_upper_limit)
        .collect();
    driver
        .inject_player_intents(
            &[
                // 对手盘：acct1（float 持仓充足）卖 100@1000。
                engine::ReceiptBearingIntent {
                    owner: engine::AccountId(1),
                    intent: engine::Intent::PlaceLimit {
                        code: engine::StockId(0),
                        side: engine::Side::Sell,
                        price: engine::strategy::LimitPrice::Fixed(engine::Money::from_cents(
                            1_000,
                        )),
                        qty: 100,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                },
                // 玩家市价买 300：按涨停保护逐档即成、余量不进簿。
                engine::ReceiptBearingIntent {
                    owner: engine::AccountId(0),
                    intent: engine::Intent::PlaceMarket {
                        code: engine::StockId(0),
                        side: engine::Side::Buy,
                        qty: 300,
                    },
                    account_ordinal: 0,
                    stock_ordinal: 0,
                },
            ],
            &upper,
        )
        .expect("注入");
    let events = driver
        .step_frame_equivalent(&mut session)
        .expect("市价拍");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    assert!(
        events.iter().any(|e| matches!(
            e,
            Event::Trade { price, qty, maker, taker, .. }
                if price.cents() == 1_000 && *qty == 100
                    && *maker == engine::AccountId(1) && *taker == engine::AccountId(0)
        )),
        "市价单必须真实路由成交（连续相位 maker 价 1000、taker=玩家）：{events:?}"
    );
    // 实体跨拍落账（R10 口径）：tick1 产 BoughtShares/CashRefund 实体，
    // 下一拍 start 应用——账面断言在空拍后取（desk 面断言不受影响）。
    let _ = driver
        .step_frame_equivalent(&mut session)
        .expect("落账拍");
    driver.install_market_views(&mut session).expect("行情");
    driver.install_candles(&mut session).expect("K");
    let accounts = driver.world_accounts_snapshot().expect("组快照");
    let player = &accounts[0].1[0];
    assert_eq!(player.shares[0], 100, "成交 100 股");
    assert_eq!(
        player.cash,
        player_cash0 - 100_501,
        "实付=对价+最低佣金 500+过户 1；余量 200 purse 全额退款"
    );
    let views = driver.world_public_views();
    assert!(
        views[0].depth_bids.is_empty(),
        "市价余量不进簿（R16 玩家路径）：{:?}",
        views[0].depth_bids
    );
    // 符号价正向路由（下一拍）：acct1 Lowest 卖解析=跌停 900 挂簿
    // （玩家当日买入受 T+1 冻结不可卖，用 float 持仓账户承载）。
    driver
        .inject_player_intents(
            &[engine::ReceiptBearingIntent {
                owner: engine::AccountId(1),
                intent: engine::Intent::PlaceLimit {
                    code: engine::StockId(0),
                    side: engine::Side::Sell,
                    price: engine::strategy::LimitPrice::Lowest,
                    qty: 100,
                },
                account_ordinal: 0,
                stock_ordinal: 0,
            }],
            &[],
        )
        .expect("注入符号卖");
    let _ = driver
        .step_frame_equivalent(&mut session)
        .expect("符号卖拍");
    driver.install_market_views(&mut session).expect("行情");
    let views = driver.world_public_views();
    // 手算：连续相位笼子生效（默认配置），参考价=本拍成交 1000 ⇒
    // 卖方下限取 min(1000×98%, 1000−10 档)=980 与跌停 900 的较严者
    // =980——Lowest 解析=**合法卖出下边界 980**（R13 笼子∧R19 带复合，
    // 非裸跌停；误路由形态=涨停 1100 不得出现）。
    assert_eq!(
        views[0].best_ask,
        Some(980),
        "Lowest 卖解析=卖出下边界（笼子 980∧跌停 900 取严）；此前误路由 Highest=1100"
    );
}

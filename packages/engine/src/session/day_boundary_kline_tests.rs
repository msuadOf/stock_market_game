//! 日界拍日K漂移修复批（B2 台账"main 既有漂移登记 1"）：
//! 生产时序 desk `roll_day`（tick=tpd 首拍开头）先于 session 日终迁移与
//! 日结 Drain——收盘竞价成交迟到 Drain 清算，install 面提交的当日 K
//! 拿不到收盘段（占位/部分丢失两形态），desk 面归档 K 同缺且收盘段
//! 污染次日活动 K；成交 receipt 的 `civil_date` 取下一拍组装时点=次日、
//! 日结静点 save/restore 丢（台账建议 1）。
//!
//! 语义合同（大 A 基线）：
//! - 收盘集合竞价成交（15:00/900 桶）计入**当日**日K（量/额/笔/OHLC）；
//! - install 面（session `candle_book` 历史 + `DayBoundary.closed_daily_
//!   candles`）与 desk 面（`market.candles` 归档）逐位一致；
//! - 次日活动 K 不被前日收盘段污染（跨日清零语义）；
//! - 收盘竞价成交回执（receipt）的 `civil_date` 归**当日**，且日结静点
//!   save/restore 不丢。

use crate::money::Money;
use crate::orderbook::Side;
use crate::strategy::LimitPrice;
use crate::{AccountId, GameSession, StockId};

/// 单股小日历 fixture：tpd=30 / auction=6（申报窗 day_tick 0..3、盘前
/// 4..5、开盘清算 tick4 拍开头、连续 6..26）/ closing=3（收盘窗 day_tick
/// 27..29；窗首拍受理单按连续末拍基准立即撮合，28/29 拍挂单留待 Drain
/// 清算）。日 0 = tick 1..30；tick30 拍开头 desk `roll_day`（tick30 本身
/// =日1 申报窗），tick30 拍步尾（ends_day，`tick()%tpd==0`）编排日终
/// 迁移与日结 Drain（收盘竞价清算在此成交）。
fn boundary_setup() -> crate::session::SessionSetup {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    // 开局可卖持仓来源（卖腿 fixture 依赖——quote_setup 原值 0）。
    setup.stocks[0].float_shares = 800_000;
    setup.npcs.retail_count = 2;
    setup.npcs.inst_count = 0;
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.ticks_per_day = 30;
    setup.auction_ticks = 6;
    setup.closing_auction_ticks = 3;
    // 交易日开局（B2 串行面测试同式——保留快照冻结的休市判定依赖日历）。
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").expect("测试日期合法");
    setup
}

/// 交叉限价意图对（双边同价限价，必交叉成交；价由调用方给定——
/// 1000=初始锚定价，1005=清算价≠锚定的区分性变体）。
fn crossing_intents_at(
    buyer: AccountId,
    seller: AccountId,
    price_cents: i64,
) -> Vec<crate::session::ReceiptBearingIntent> {
    vec![
        crate::session::ReceiptBearingIntent {
            owner: buyer,
            intent: crate::Intent::PlaceLimit {
                code: StockId(0),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(price_cents)),
                qty: 100,
            },
            account_ordinal: 0,
            stock_ordinal: 0,
        },
        crate::session::ReceiptBearingIntent {
            owner: seller,
            intent: crate::Intent::PlaceLimit {
                code: StockId(0),
                side: Side::Sell,
                price: LimitPrice::Fixed(Money::from_cents(price_cents)),
                qty: 100,
            },
            account_ordinal: 0,
            stock_ordinal: 0,
        },
    ]
}

/// 开局有可卖持仓的 NPC（float 分配）与另一账户配对。
fn trade_pair(session: &GameSession) -> (AccountId, AccountId) {
    let seller = [AccountId(1), AccountId(2)]
        .into_iter()
        .find(|id| {
            session
                .account(*id)
                .is_some_and(|a| a.sellable_qty(&StockId(0)) >= 500)
        })
        .expect("至少一个 NPC 开局持有股 0（float 分配）");
    let buyer = if seller == AccountId(1) {
        AccountId(2)
    } else {
        AccountId(1)
    };
    (buyer, seller)
}

/// 跑到第 `until` 拍；`inject_ticks` 指定各拍注入交叉意图对。返回
/// （session, driver, 各拍事件）。tick30 拍=日0 的日结拍。
fn run_boundary(
    setup: crate::session::SessionSetup,
    seed: u64,
    inject_ticks: &[u64],
    until: u64,
) -> (
    GameSession,
    crate::runtime::drive::RuntimeDriver,
    std::collections::BTreeMap<u64, Vec<crate::Event>>,
) {
    let mut session = GameSession::new(setup, seed).expect("会话构造");
    let (buyer, seller) = trade_pair(&session);
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session)
        .expect("驱动器构造");
    driver.adopt_session_seq(&session);
    let mut per_tick = std::collections::BTreeMap::new();
    for t in 1..=until {
        if inject_ticks.contains(&t) {
            let upper: Vec<i64> = driver
                .world_public_views()
                .iter()
                .map(|v| v.daily_upper_limit)
                .collect();
            driver
                .inject_player_intents(&crossing_intents_at(buyer, seller, 1000), &upper)
                .expect("意图注入");
        }
        let events = session
            .runtime_step_with_install(&mut driver)
            .unwrap_or_else(|e| panic!("tick{t} 步进失败：{e}"));
        per_tick.insert(t, events);
    }
    (session, driver, per_tick)
}

/// install 面日 0 K（candle_book 历史——日 0 尚为末条；日 1 未日结）。
fn install_day0_candle(session: &GameSession) -> crate::session::DailyCandle {
    session
        .candle_book_histories_for_runtime(&StockId(0))
        .last()
        .cloned()
        .expect("install 面必有日 0 日 K")
}

/// desk 面日 0 K（candles 归档末条——8 元组 time/O/H/L/C/volume/额/笔）。
fn desk_day0_candle(
    driver: &mut crate::runtime::drive::RuntimeDriver,
) -> (i64, i64, i64, i64, i64, u64, i64, u64) {
    let finals = driver.world_desk_finals_for_candles().expect("desk 面");
    let &(time, o, h, l, c, v, turnover, count) = finals[0]
        .1
        .last()
        .expect("desk 面必有日 0 归档 K");
    (time, o, h, l, c, v, turnover, count)
}

/// 日 K 逐位断言（量/额/笔/OHLC——三面共用的期望形态）。
#[track_caller]
fn assert_day_k(
    context: &str,
    volume: u64,
    turnover: i64,
    trade_count: u64,
    o: i64,
    h: i64,
    l: i64,
    c: i64,
    candle: &crate::session::DailyCandle,
) {
    assert_eq!(candle.volume, volume, "{context}: volume");
    let stats = candle
        .trade_stats
        .as_ref()
        .unwrap_or_else(|| panic!("{context}: 日 K 必带成交统计"));
    assert_eq!(stats.turnover_cents, turnover as u64, "{context}: turnover");
    assert_eq!(stats.trade_count, trade_count, "{context}: trade_count");
    assert_eq!(candle.open.cents(), o, "{context}: open");
    assert_eq!(candle.high.cents(), h, "{context}: high");
    assert_eq!(candle.low.cents(), l, "{context}: low");
    assert_eq!(candle.close.cents(), c, "{context}: close");
}

/// 场景一（占位/全丢形态）：全日仅收盘竞价成交——install 面旧提交
/// volume=0 占位（B2 探针实证），修复后三面均拿到 200 股真实 K；receipt
/// 的 civil_date 归当日；日结静点 save/restore 不丢。
#[test]
fn closing_only_trades_reach_day_kline_and_receipts() {
    // 仅 tick28/29 注入（收盘窗挂单——Drain 清算成交 200 股 @1000）。
    let (session, mut driver, per_tick) = run_boundary(boundary_setup(), 91, &[28, 29], 32);

    // install 面：日 0 K = 收盘段真实值（量 200/额 200000/笔 2/OHLC 1000）。
    let install = install_day0_candle(&session);
    assert_day_k("install", 200, 200_000, 2, 1000, 1000, 1000, 1000, &install);
    // desk 面：归档 K 与 install 面 逐位一致（desk 侧 K 无时间戳——
    // 投影 time 恒 0，量/额/笔/OHLC 为对账面）。
    let (_, o, h, l, c, v, _turnover, _count) = desk_day0_candle(&mut driver);
    assert_eq!((o, h, l, c, v), (1000, 1000, 1000, 1000, 200), "desk OHLCV");
    // desk 归档 K 无额/笔面（DeskCandle 结构不含 stats——投影恒 0）；
    // 额/笔经 install 面断言+bars 冻结对账（下）双重覆盖。
    // DayBoundary 事件面：closed_daily_candles 与 install 面一致。
    let boundary = per_tick[&30]
        .iter()
        .find_map(|e| match e {
            crate::Event::DayBoundary {
                closed_daily_candles,
                ..
            } => Some(closed_daily_candles),
            _ => None,
        })
        .expect("日结拍必有 DayBoundary 事件");
    let event_candle = boundary
        .get(&StockId(0))
        .expect("DayBoundary 必含股 0 日 K");
    assert_day_k("DayBoundary", 200, 200_000, 2, 1000, 1000, 1000, 1000, event_candle);
    // 保留快照：日 0 bars 总量=200，含 900（收盘竞价）桶。
    let day0 = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let retained = session
        .retained_market_days()
        .last()
        .expect("日 0 保留快照已冻结");
    assert_eq!(retained.0.date, day0, "保留快照归日 0");
    let bars = &retained.0.securities[&0].bars;
    let vol: u64 = bars.iter().map(|b| b.volume_shares).sum();
    assert_eq!(vol, 200, "保留 bars 总量=收盘段 200");
    assert!(
        bars.iter().any(|b| b.minute_of_day == 900),
        "收盘竞价 900 桶在列"
    );
    // 次日（日 1）活动 K 不被收盘段污染。
    assert_eq!(
        driver.world_public_views()[0].daily_active.map(|(_, _, _, _, v)| v),
        Some(0),
        "日 1 活动 K 零量（收盘段不跨日残留）"
    );
    assert!(
        session
            .runtime_active_candle(&StockId(0))
            .is_none_or(|candle| candle.volume == 0),
        "session 日 1 活动 K 零量"
    );
    // receipt：买腿两条，civil_date=当日（day0），价量正确。
    let (buyer, _) = trade_pair(&session);
    let confirmations = session
        .personal_trade_confirmations(buyer)
        .expect("买腿必有个人确认");
    let buys: Vec<_> = confirmations
        .iter()
        .filter(|row| row.civil_date == day0 && row.side == Side::Buy)
        .collect();
    assert_eq!(buys.len(), 2, "收盘竞价买腿恰两条：{confirmations:?}");
    for row in &buys {
        assert_eq!(row.price_cents, 1000);
        assert_eq!(row.quantity_shares, 100);
    }

    // 日结静点 save/restore：收盘 receipt 不丢。
    let (session2, driver2, _) = run_boundary(boundary_setup(), 91, &[28, 29], 30);
    let save = session2.save().expect("日结静点 save");
    let restored = GameSession::restore(&save).expect("restore");
    let (buyer2, _) = trade_pair(&restored);
    let restored_rows = restored
        .personal_trade_confirmations(buyer2)
        .expect("restore 后买腿确认在册");
    assert_eq!(
        restored_rows
            .iter()
            .filter(|row| row.civil_date == day0 && row.side == Side::Buy)
            .count(),
        2,
        "日结静点 save/restore 后收盘竞价 receipt 存活"
    );
    drop(driver2);
}

/// 场景二（部分丢失形态，此前由测试构造规避）：日内连续成交 + 收盘竞价
/// 成交——旧形态 install 面 K 只含日内段（candle=300 vs bars=500，冻结
/// 对账硬红）；修复后三面 500 股逐位一致、日结拍成功。
#[test]
fn intraday_plus_closing_day_kline_matches_bars_and_desk() {
    // tick24/25/26 连续段成交（3 笔 300 股）+ tick28/29 收盘窗挂单
    // （Drain 清算 2 笔 200 股）。
    let (session, mut driver, per_tick) = run_boundary(boundary_setup(), 92, &[24, 25, 26, 28, 29], 32);

    // 日结拍（tick30）成功即红证闭合（旧形态在冻结对账硬失败）。
    assert!(
        per_tick[&30]
            .iter()
            .any(|e| matches!(e, crate::Event::DayBoundary { .. })),
        "日结拍产出 DayBoundary"
    );
    // install 面：日内 300 + 收盘 200。
    let install = install_day0_candle(&session);
    assert_day_k("install", 500, 500_000, 5, 1000, 1000, 1000, 1000, &install);
    // desk 面 逐位一致（desk 侧 K 无时间戳——量/额/笔/OHLC 对账）。
    let (_, o, h, l, c, v, _turnover, _count) = desk_day0_candle(&mut driver);
    assert_eq!((o, h, l, c, v), (1000, 1000, 1000, 1000, 500));
    // DayBoundary 事件面一致。
    let boundary = per_tick[&30]
        .iter()
        .find_map(|e| match e {
            crate::Event::DayBoundary {
                closed_daily_candles,
                ..
            } => Some(closed_daily_candles),
            _ => None,
        })
        .expect("DayBoundary");
    assert_day_k(
        "DayBoundary",
        500,
        500_000,
        5,
        1000,
        1000,
        1000,
        1000,
        boundary.get(&StockId(0)).expect("股 0 日 K"),
    );
    // 保留快照：bars 总量=500 与日 K 逐位（冻结对账在全对账分支通过）。
    let retained = session
        .retained_market_days()
        .last()
        .expect("日 0 保留快照");
    let bars = &retained.0.securities[&0].bars;
    let vol: u64 = bars.iter().map(|b| b.volume_shares).sum();
    assert_eq!(vol, 500, "bars 总量=日 K 量");
    // 次日活动 K 零量（收盘段不污染）。
    assert_eq!(
        driver.world_public_views()[0].daily_active.map(|(_, _, _, _, v)| v),
        Some(0),
        "日 1 活动 K 零量"
    );
}


/// 场景三（复核建议 S3）：多日连续日界——日 1 收盘段同样归位，日 0
/// 冻结结果不被日 1 覆盖（retained 链两条、逐日独立）。
#[test]
fn second_day_boundary_keeps_days_independent() {
    // 日 0：收盘窗挂单（tick28/29→清算 200）；日 1（tick31..60）：连续段
    // tick52/53（200）+收盘窗 tick58/59（清算 200）。双时钟契约：纯 step
    // 路径不推进 civil（authoritative_tick_tests 同式——休市日/已收盘日
    // 先 end_civil_day 对齐交易日序，否则日 1 冻结被同日幂等检查跳过）。
    let mut session = GameSession::new(boundary_setup(), 93).expect("会话构造");
    let (buyer, seller) = trade_pair(&session);
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session)
        .expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for t in 1..=62u64 {
        while session.civil_clock().phase() == crate::session::CivilPhase::ClosedDay
            || session
                .civil_clock()
                .completed_trading_sessions_expected()
                .unwrap()
                != session.day() + 1
        {
            session.end_civil_day().expect("civil 推进");
        }
        if [28, 29, 52, 53, 58, 59].contains(&t) {
            let upper: Vec<i64> = driver
                .world_public_views()
                .iter()
                .map(|v| v.daily_upper_limit)
                .collect();
            driver
                .inject_player_intents(&crossing_intents_at(buyer, seller, 1000), &upper)
                .expect("意图注入");
        }
        session
            .runtime_step_with_install(&mut driver)
            .unwrap_or_else(|e| panic!("tick{t} 步进失败：{e}"));
    }
    let driver = driver;
    let retained = session.retained_market_days();
    assert_eq!(retained.len(), 2, "两日各冻一条");
    let day1_bars = &retained[1].0.securities[&0].bars;
    let vol: u64 = day1_bars.iter().map(|b| b.volume_shares).sum();
    assert_eq!(vol, 400, "日 1 bars=日内 200+收盘 200");
    // install 面：末条=日 1 终 K；其前=日 0 终 K（不被覆盖）。
    let hist = session.candle_book_histories_for_runtime(&StockId(0));
    assert_day_k(
        "install 日 1",
        400,
        400_000,
        4,
        1000,
        1000,
        1000,
        1000,
        hist.last().expect("日 1 K"),
    );
    assert_day_k(
        "install 日 0",
        200,
        200_000,
        2,
        1000,
        1000,
        1000,
        1000,
        &hist[hist.len() - 2],
    );
    // 日 2 活动 K 零量（第二日收盘段不跨日残留）。
    assert_eq!(
        driver.world_public_views()[0].daily_active.map(|(_, _, _, _, v)| v),
        Some(0),
        "日 2 活动 K 零量"
    );
}

/// 场景四（复核建议 S4）：收盘清算价≠昨收锚定——全日仅收盘竞价成交、
/// 双边同价 1005（>锚定 1000）：开盘价=首笔成交价=清算价（B4a 的
/// settle_fill 重锚定经本批 unroll 通道同样生效），额=1005×200。
#[test]
fn closing_only_candle_opens_at_clearing_price_not_anchor() {
    let mut session = GameSession::new(boundary_setup(), 94).expect("会话构造");
    let (buyer, seller) = trade_pair(&session);
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session(&mut session)
        .expect("驱动器构造");
    driver.adopt_session_seq(&session);
    for t in 1..=32u64 {
        if (28..=29).contains(&t) {
            let upper: Vec<i64> = driver
                .world_public_views()
                .iter()
                .map(|v| v.daily_upper_limit)
                .collect();
            driver
                .inject_player_intents(&crossing_intents_at(buyer, seller, 1005), &upper)
                .expect("意图注入");
        }
        session
            .runtime_step_with_install(&mut driver)
            .unwrap_or_else(|e| panic!("tick{t} 步进失败：{e}"));
    }
    let install = install_day0_candle(&session);
    assert_day_k(
        "install",
        200,
        201_000,
        2,
        1005,
        1005,
        1005,
        1005,
        &install,
    );
    // 次日昨收=官方收盘价 1005（涨跌停基准随 reroll 归正——读面抽查）。
    let views = driver.world_public_views();
    assert!(
        views[0].last_price == 1005,
        "清算后最新价=1005（实为 {}）",
        views[0].last_price
    );
}

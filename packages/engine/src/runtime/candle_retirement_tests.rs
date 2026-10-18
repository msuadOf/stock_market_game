//! candle_install 退役战役合同测试（设计稿
//! agents/view-campaign/plans/candle-install-retirement-design.md §3；
//! 第 20 任批 1——现状绿基线+注入面桩红先行）。
//!
//! 两刀：①active 观察 8 元组随 DeskReport 上行进发布版本（StockPublic
//! 扩 turnover/trade_count 2 字段——发布面唯一协议变更点），install
//! 改读发布版本，EchoCandles 生产往返退役（对账面保留）；②install
//! 循环瘦身（setup 外提+预计算）。
//!
//! 合同承载：合同 1=主等价锚（desk 权威对照——现状绿基线，刀 1 后必须
//! 仍绿）；合同 2/3/4=相位语义回归锚（占位 K fault2064 族/竞价窗恒空
//! fault2076 族/日结后清空）；旋钮 1/2=突变红判别（批 1 桩=读路径未接
//! 发布版本，旋钮无效果→旋钮测试红=注入面红先行；批 2 实现后开旋钮
//! 才产生篡改→红证翻转语义见各测试注释）。

use super::bridge_tests::setup_fixture;
use super::drive::RuntimeDriver;
use crate::GameSession;

/// 标准驱动夹具：跨竞价窗跑 `ticks` 拍（每拍步进+行情+K 安装——
/// 生产安装序同 drive_tests 各用例）。
fn drive_ticks(driver: &mut RuntimeDriver, session: &mut GameSession, ticks: usize) {
    for _ in 0..ticks {
        driver
            .step_frame_equivalent(session)
            .expect("步进");
        driver.install_market_views(session).expect("行情安装");
        driver.install_candles(session).expect("K 安装");
    }
}

/// desk 权威 active → session DailyCandle 的期望值（对账映射——与
/// install_candles_with_mode 的装载同式：time 占位 0、stats 来自
/// 8 元组第 7/8 位）。
fn desk_active_to_daily(a: (i64, i64, i64, i64, i64, u64, i64, u64)) -> crate::session::DailyCandle {
    let _ = a.0; // 8 元组首位为 time 占位（desk 侧恒 0；session 时间另取——对照不比 time）
    crate::session::DailyCandle {
        time: 0,
        open: crate::money::Money::from_cents(a.1),
        high: crate::money::Money::from_cents(a.2),
        low: crate::money::Money::from_cents(a.3),
        close: crate::money::Money::from_cents(a.4),
        volume: a.5,
        trade_stats: Some(crate::session::DailyTradeStats {
            turnover_cents: a.6.max(0) as u64,
            trade_count: a.7,
        }),
    }
}

/// 跨过申报窗后注入对手双方限价交叉单制造成交（d3a2c 手法：开局
/// float 分配里有券的 NPC 做卖腿、另一 NPC 做买腿——限价 1000=初始
/// 价交叉必成交；同 owner 买卖不构成对手）。两轮注入+各 2 拍——
/// active 带成交量（stats 判别前提）。
fn inject_crossing_intents(driver: &mut RuntimeDriver, session: &mut GameSession) {
    use crate::orderbook::Side;
    let seller = [crate::AccountId(1), crate::AccountId(2)]
        .into_iter()
        .find(|id| {
            session
                .account(*id)
                .is_some_and(|a| a.sellable_qty(&crate::StockId(0)) >= 100)
        })
        .expect("至少一个 NPC 开局持有股 0（float 分配）");
    let buyer = if seller == crate::AccountId(1) {
        crate::AccountId(2)
    } else {
        crate::AccountId(1)
    };
    for _ in 0..2 {
        let intents = vec![
            crate::session::ReceiptBearingIntent {
                owner: buyer,
                intent: crate::Intent::PlaceLimit {
                    code: crate::StockId(0),
                    side: Side::Buy,
                    price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(1000)),
                    qty: 100,
                },
                account_ordinal: 0,
                stock_ordinal: 0,
            },
            crate::session::ReceiptBearingIntent {
                owner: seller,
                intent: crate::Intent::PlaceLimit {
                    code: crate::StockId(0),
                    side: Side::Sell,
                    price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(1000)),
                    qty: 100,
                },
                account_ordinal: 0,
                stock_ordinal: 0,
            },
        ];
        let upper: Vec<i64> = driver
            .world_public_views()
            .iter()
            .map(|v| v.last_price + 1)
            .collect();
        driver
            .inject_player_intents(&intents, &upper)
            .expect("注入对手单");
        drive_ticks(driver, session, 2);
    }
}

/// 合同 1（主等价锚）：install 后 session active K 与 desk 权威逐位
/// 一致（OHLCV+turnover+count——stats 2 字段的贯通判别面）。现状绿
/// 基线（安装源=desk 往返）；刀 1 后安装源=发布版本，必须仍绿。
#[test]
fn candle_install_matches_desk_authority() {
    let mut session = GameSession::new(setup_fixture(), 41).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    drive_ticks(&mut driver, &mut session, 25);
    inject_crossing_intents(&mut driver, &mut session);
    // 对账前提：fixture 须产生成交（active 在册+volume>0——stats 贯通
    // 的可观察前提；无成交形态由合同 2 承载）。
    let finals = driver
        .world_desk_finals_for_candles()
        .expect("desk 权威对账面");
    let traded: Vec<_> = finals
        .iter()
        .filter(|(_, _, active)| active.is_some_and(|a| a.5 > 0))
        .collect();
    assert!(
        !traded.is_empty(),
        "fixture 须有当日成交（active 在册且 volume>0）——合同前提"
    );
    for (stock, _, active) in &finals {
        let code = crate::StockId(u32::from(*stock));
        let installed = session.runtime_active_candle(&code);
        match active {
            Some(a) => {
                let got = installed.expect("desk active 在册 ⇒ session active 在册");
                let expected = desk_active_to_daily(*a);
                // OHLCV 逐位（Money 不含 PartialEq 语义时按分量——
                // DailyCandle 字段直接比较）。
                assert_eq!(
                    (got.open.cents(), got.high.cents(), got.low.cents(), got.close.cents(), got.volume),
                    (expected.open.cents(), expected.high.cents(), expected.low.cents(), expected.close.cents(), expected.volume),
                    "股 {code:?} active OHLCV 安装==desk 权威"
                );
                assert_eq!(
                    got.trade_stats.as_ref().map(|s| (s.turnover_cents, s.trade_count)),
                    expected.trade_stats.as_ref().map(|s| (s.turnover_cents, s.trade_count)),
                    "股 {code:?} trade_stats（turnover/trade_count）安装==desk 权威——stats 2 字段贯通判别面"
                );
            }
            None => {
                // desk 无 active（无成交）⇒ session 侧应为占位 K 或
                // 申报窗清空（相位语义由合同 2/3 承载——此处仅登记
                /// 不误断 None）。
                let _ = installed;
            }
        }
    }
}

/// 合同 2（占位 K 回归锚，fault2064 族）：desk 无 active（当日无成交）
/// 且非申报窗/非日结拍 ⇒ session active=占位 K（last_price 四值+volume
/// 0——无成交日不缺条）。
#[test]
fn candle_placeholder_on_no_trade_stock() {
    let mut session = GameSession::new(setup_fixture(), 42).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    drive_ticks(&mut driver, &mut session, 25);
    let finals = driver
        .world_desk_finals_for_candles()
        .expect("desk 权威对账面");
    let views = driver.world_public_views().to_vec();
    // 至少一股无成交（fixture 成交稀疏——找 desk active=None 的股）；
    // 全股成交时跳过前提（fixture 演化使然——如实登记，相位窗判定
    // 仍执行：本测试的守卫面=占位形态不回归）。
    for (stock, _, active) in &finals {
        if active.is_some() {
            continue;
        }
        let code = crate::StockId(u32::from(*stock));
        let last = views[*stock as usize].last_price;
        match session.runtime_active_candle(&code) {
            Some(k) => {
                assert_eq!(
                    (k.open.cents(), k.high.cents(), k.low.cents(), k.close.cents()),
                    (last, last, last, last),
                    "股 {code:?} 占位 K 四值==last_price（无成交日不缺条——fault2064 族）"
                );
                assert_eq!(k.volume, 0, "占位 K 零量");
            }
            None => {
                // 申报窗内清空合法（相位基准）——窗口内不误断；本测试
                /// 跑 25 拍已跨申报窗（auction_entry=10），None=语义
                /// 回归，显式红。
                panic!(
                    "股 {code:?} 跨申报窗后 desk 无成交股的 active 须为占位 K（fault2064 族回归）"
                );
            }
        }
    }
}

/// 合同 3（竞价申报窗恒空，fault2076 族）：day_tick < auction_entry
/// 的拍，session active 必须为 None（申报窗静点）。
#[test]
fn candle_auction_window_active_empty() {
    let mut session = GameSession::new(setup_fixture(), 43).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    // 申报窗内 3 拍（fixture auction_ticks 默认 15 ⇒ entry=10）。
    drive_ticks(&mut driver, &mut session, 3);
    for stock in 0u32..2 {
        let code = crate::StockId(stock);
        assert!(
            session.runtime_active_candle(&code).is_none(),
            "股 {code:?} 竞价申报窗内 active 恒空（fault2076 族）"
        );
    }
}

/// 合同 4（post_day_end 补装清空）：`install_candles_with_mode(true)`
/// 分支语义——active_must_be_empty 恒 true ⇒ 全股清空（日界静点）。
/// 该分支的生产调用点=session 内嵌 runtime 模式的 ends_day 段
/// （failure.rs day_end_candles）；本合同以直调承载分支语义（等价
/// 判别面=清空行为本身，不依赖完整日结编排）。
#[test]
fn candle_post_day_end_clear() {
    let mut session = GameSession::new(setup_fixture(), 44).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    drive_ticks(&mut driver, &mut session, 25);
    inject_crossing_intents(&mut driver, &mut session);
    // 前提：至少一股 active 在册（清空才有可观察面）。
    let mut had_active = false;
    for stock in 0u32..2 {
        if session
            .runtime_active_candle(&crate::StockId(stock))
            .is_some()
        {
            had_active = true;
        }
    }
    assert!(had_active, "补装前须有 active 在册（清空判别前提）");
    driver
        .install_candles_with_mode(&mut session, true)
        .expect("post_day_end 补装");
    for stock in 0u32..2 {
        let code = crate::StockId(stock);
        assert!(
            session.runtime_active_candle(&code).is_none(),
            "股 {code:?} post_day_end 补装后 active 清空（日界静点——发布版本残留不得复活）"
        );
    }
}

/// 旋钮 1（突变红判别）：开 `candle_knob_tamper_active_for_test` 后
/// 安装必须出现 +1 分偏差——判别 install 数据源真为发布版本读路径
/// （批 1 桩：读路径未接，旋钮无效果 ⇒ 无偏差 ⇒ 本测试红=注入面红
/// 先行；批 2 后语义=开旋钮产生偏差红证翻转——本测试在批 2 时改为
/// 断言"偏差出现后 desk 对照红"的负控形态）。
#[test]
fn candle_knob1_tamper_proves_publication_source() {
    let mut session = GameSession::new(setup_fixture(), 45).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    driver.candle_knob_tamper_active_for_test();
    drive_ticks(&mut driver, &mut session, 25);
    let finals = driver
        .world_desk_finals_for_candles()
        .expect("desk 权威对账面");
    let mut tampered = 0usize;
    for (stock, _, active) in &finals {
        let Some(a) = active else { continue };
        let code = crate::StockId(u32::from(*stock));
        let Some(got) = session.runtime_active_candle(&code) else {
            continue;
        };
        if got.close.cents() != a.4 {
            tampered += 1;
        }
    }
    assert!(
        tampered > 0,
        "旋钮 1 开启后至少一股 active close 偏离 desk 权威（批 1 红证：读路径未接发布版本，旋钮无效）"
    );
}

/// 旋钮 2（突变红判别）：开 `candle_knob_drop_turnover_for_test` 后
/// 安装的 trade_stats.turnover 必须归零偏离——判别 stats 2 字段贯通
/// （批 1 桩红先行；批 2 负控翻转同旋钮 1）。
#[test]
fn candle_knob2_drop_turnover_proves_stats_flow() {
    let mut session = GameSession::new(setup_fixture(), 46).expect("会话构造");
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造");
    driver.adopt_session_seq(&session);
    driver.candle_knob_drop_turnover_for_test();
    drive_ticks(&mut driver, &mut session, 25);
    let finals = driver
        .world_desk_finals_for_candles()
        .expect("desk 权威对账面");
    let mut dropped = 0usize;
    for (stock, _, active) in &finals {
        let Some(a) = active else { continue };
        if a.6 <= 0 {
            continue; // 无成交额股无判别力
        }
        let code = crate::StockId(u32::from(*stock));
        let Some(got) = session.runtime_active_candle(&code) else {
            continue;
        };
        if got.trade_stats.as_ref().is_some_and(|s| s.turnover_cents == 0) {
            dropped += 1;
        }
    }
    assert!(
        dropped > 0,
        "旋钮 2 开启后至少一股 turnover 归零偏离 desk 权威（批 1 红证：stats 读路径未接，旋钮无效）"
    );
}

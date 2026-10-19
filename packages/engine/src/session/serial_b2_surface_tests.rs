//! B2 串行数据面（wasm 旧管线）出数测试。
//!
//! W3 接线实证缺陷 1 红证：`step_runtime_enabled()` 在 wasm32 恒 false
//! ⇒ web 产品走串行管线（旧管线），但 B2 三安装面（market_minute_bars
//! 安装 / receipts 入账 / retained_market_days 冻结）全在 runtime 路径
//! （step_via_runtime 安装链），串行侧 `finalize_continuous_tick` 不装
//! ⇒ wasm 四查询面恒空数据。
//!
//! 修复合同：串行 finalize 复用 B2 **同一套**纯函数与 session 镜像面做
//! 等价安装（单一事实源——分钟桶聚合/键推导与 desk 同函数；receipts
//! 与 retained 复用 runtime_append_trade_receipts / set_runtime_retained_
//! day 同一累积面）。
//!
//! 探针式双进程（翻转批 batch8 同款基建）：`step_runtime_enabled` 的
//! OnceLock 进程级缓存使本进程无法中途切串行——主入口 spawn 子进程
//! （`ENGINE_STEP_RUNTIME=0`）承载真串行断言；子进程内 `step()` /
//! `end_civil_day()` 走的正是 wasm32 的分叉路径。

use super::GameSession;
use crate::session::npc_working_quote_tests::quote_setup;

/// 子进程防递归标记。
const SERIAL_PROBE: &str = "B2_SERIAL_SURFACE_PROBE";

/// 静默局（tpd=3、retail=1）——retained_snapshot_tests 同式 fixture。
fn b2_session(start: &str) -> GameSession {
    let mut setup = quote_setup(0);
    setup.start_date = crate::CivilDate::from_iso(start).unwrap();
    setup.ticks_per_day = 3;
    setup.npcs.inst_count = 0;
    setup.npcs.retail_count = 1;
    setup.stocks[0].float_shares = 1_000;
    GameSession::new(setup, 71).unwrap()
}

/// 玩家买 100 股成交（seed 挂卖对手盘+玩家限价买），真串行 step()。
fn execute_buy(session: &mut GameSession) -> crate::StockId {
    let code = crate::StockId(0);
    let mut events = Vec::new();
    session.seed_order_for_test(
        crate::AccountId(1),
        crate::Intent::PlaceLimit {
            code: code.clone(),
            side: crate::orderbook::Side::Sell,
            price: crate::LimitPrice::Fixed(crate::money::Money::from_cents(1000)),
            qty: 100,
        },
        &mut events,
    );
    session
        .hydrate_or_validate_envelope_ledger()
        .expect("seed 单账本 hydrate");
    session
        .enqueue_player_intent(
            crate::AccountId(0),
            crate::Intent::PlaceLimit {
                code: code.clone(),
                side: crate::orderbook::Side::Buy,
                price: crate::LimitPrice::Fixed(crate::money::Money::from_cents(1005)),
                qty: 100,
            },
        )
        .unwrap();
    let step_events = session.step().expect("首步成交（串行 step）");
    assert!(
        step_events
            .iter()
            .any(|e| matches!(e, crate::Event::Trade { .. })),
        "首步必须发生成交（对手盘已 seed）"
    );
    code
}

/// 串行路径四查询面出数（真串行子进程探针主体）。
fn serial_four_surfaces_probe() {
    let mut session = b2_session("2030-01-02");
    let code = execute_buy(&mut session);

    // 面板一：当日分钟K（G2 实时视图）——串行安装后 bars 非空且量/额/
    // 笔与真实成交一致（100 股 @1000 分）。
    let current = session
        .current_minute_history(&crate::session::CurrentMinuteHistoryRequest {
            stock: code.clone(),
        })
        .unwrap();
    assert!(current.trading, "开市日 trading=true");
    assert!(!current.bars.is_empty(), "串行路径当日分钟桶必须出数");
    assert_eq!(current.bars[0].volume_shares, 100);
    assert_eq!(current.bars[0].turnover_cents, 100_000);
    assert_eq!(current.bars[0].trade_count, 1);
    assert_eq!(current.bars[0].open, crate::money::Money::from_cents(1000));

    // 面板二/三：个人交割单（G5/G6）——买腿入账，费用三分项为旧管线
    // envelope 实收权威（佣金>0）。
    let confirmations = session
        .personal_trade_confirmations(crate::AccountId(0))
        .expect("串行路径个人确认日志在册");
    assert_eq!(confirmations.len(), 1, "玩家买腿恰一条");
    let receipt = &confirmations[0];
    assert_eq!(receipt.stock, code);
    assert_eq!(receipt.side, crate::orderbook::Side::Buy);
    assert_eq!(receipt.price_cents, 1000);
    assert_eq!(receipt.quantity_shares, 100);
    assert_eq!(receipt.gross_cents, 100_000);
    assert!(
        receipt.actual_fees.commission > crate::money::Money::ZERO,
        "买腿实收佣金必须真实入账（非零）"
    );
    assert_eq!(
        receipt.civil_date,
        crate::CivilDate::from_iso("2030-01-02").unwrap()
    );

    // 日结跑满 tpd（第 3 步步后 tick=tpd → 日终迁移）+ 自然日推进。
    for _ in 0..2 {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();

    // 面板四：保留历史（G8）——交易日冻结 Traded + bars 非空 + 日 K
    // stats 与分钟桶互检逐字段一致。
    let page = session
        .market_history_page(&crate::session::MarketHistoryRequest {
            stock: code.clone(),
            date_from: crate::CivilDate::from_iso("2030-01-02").unwrap(),
            date_to: crate::CivilDate::from_iso("2030-01-02").unwrap(),
            after: None,
            page_size: 7,
        })
        .unwrap();
    assert_eq!(page.entries.len(), 1, "单日范围单条目");
    let entry = &page.entries[0];
    assert_eq!(
        entry.availability,
        crate::session::MarketHistoryAvailability::Traded,
        "已结算开市日=Traded"
    );
    assert!(!entry.bars.is_empty(), "保留链冻结真实分钟 bars");
    assert_eq!(entry.bars[0].volume_shares, 100);
    let candle = entry.daily_candle.as_ref().expect("日 K 在档");
    let volume: u64 = entry.bars.iter().map(|bar| bar.volume_shares).sum();
    let turnover: u128 = entry.bars.iter().map(|bar| bar.turnover_cents).sum();
    let count: u64 = entry.bars.iter().map(|bar| bar.trade_count).sum();
    assert_eq!(candle.volume, volume, "串行日 K 量==分钟桶累计");
    let stats = candle.trade_stats.as_ref().expect("日 K stats 在档");
    assert_eq!(u128::from(stats.turnover_cents), turnover, "日 K 额==分钟桶累计");
    assert_eq!(stats.trade_count, count, "日 K 笔数==分钟桶累计");
}

/// 串行路径休市日保留条目（end_civil_day 串行分支）。
fn serial_closed_day_probe() {
    let mut session = b2_session("2030-01-05");
    session.end_civil_day().unwrap();
    let retained = session.retained_market_days().to_vec();
    assert_eq!(retained.len(), 1, "休市日补恰一条");
    let day = &retained[0].0;
    assert_eq!(day.date, crate::CivilDate::from_iso("2030-01-05").unwrap());
    let session_0 = day.securities.get(&0).expect("休市日也留证券条目");
    assert!(!session_0.trading, "休市 trading=false");
    assert!(session_0.bars.is_empty(), "休市恒空桶");
    // 幂等：同日不重复；连续第二休市日恰加一条。
    session.end_civil_day().unwrap();
    assert_eq!(session.retained_market_days().len(), 2, "连续两休市日恰两条");
}

/// 串行路径存档往返：B2 三面（桶/确认/保留链）随 save 序列化并在
/// restore 后可查（wasm 本地存档链路的数据面连续性）。
fn serial_save_restore_probe() {
    let mut session = b2_session("2030-01-02");
    let _code = execute_buy(&mut session);
    for _ in 0..2 {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        serde_json::to_value(restored.retained_market_days()).unwrap(),
        serde_json::to_value(session.retained_market_days()).unwrap(),
        "保留链 roundtrip 逐位"
    );
    assert_eq!(
        serde_json::to_value(restored.personal_trade_confirmations(crate::AccountId(0))).unwrap(),
        serde_json::to_value(session.personal_trade_confirmations(crate::AccountId(0))).unwrap(),
        "个人确认 roundtrip 逐位"
    );
    assert_eq!(
        restored.receipt_watermark(),
        session.receipt_watermark(),
        "回执水位 roundtrip"
    );
}

/// 真串行（ENGINE_STEP_RUNTIME=0 子进程）四查询面出数主入口。
#[test]
fn serial_pipeline_produces_all_four_query_surfaces() {
    if std::env::var(SERIAL_PROBE).is_ok() {
        assert!(
            !crate::runtime::step_runtime_enabled(),
            "子进程必须为串行形态（env=0）"
        );
        serial_four_surfaces_probe();
        serial_closed_day_probe();
        serial_save_restore_probe();
        return;
    }
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("session::serial_b2_surface_tests::serial_pipeline_produces_all_four_query_surfaces")
        .arg("--nocapture")
        .env("ENGINE_STEP_RUNTIME", "0")
        .env(SERIAL_PROBE, "1")
        .output()
        .expect("串行探针子进程启动");
    assert!(
        out.status.success(),
        "真串行子进程断言失败：\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
}

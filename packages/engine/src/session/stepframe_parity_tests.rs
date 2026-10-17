//! step_frame 接轨件 4 的两路径对照（C 类——同 fixture 同 seed 下
//! runtime 分叉与旧管线直调的 TickFrame 契约面对照）。
//!
//! 等价面（dying-tests-census 口径）：frame.tick/seq_from/seq_to 推进、
//! 事件类型集合（多股 fixture 含 Trade/受理/PriceTick）、
//! timeseries_payload 结构、validate 通过、权威账户面。不逐位对拍
//! 决策流（并发模型不同——两路径的 NPC 决策流不可要求逐位一致）。

use super::ProtocolSession;

fn parity_setup(auction_ticks: u64) -> crate::session::SessionSetup {
    // 引擎内已验证的 quote_setup（sol 系玩家单受理锚定的 fixture）。
    // 两场景：0=全天连续（PriceTick/受理面）；6=竞价窗（AuctionTick/
    /// AuctionCompleted/PreOpen 拒收面+转连续）。
    crate::session::npc_working_quote_tests::quote_setup(auction_ticks)
}

/// 一拍的契约面摘要（对照判别面——dying-tests-census 口径：事件
/// **类型集合**（不含数量——两路径受理/竞价事件密度形态不同，数量
/// 非等价面）、时序结构在场性；seq 数值面各自 validate 承载）。
struct FrameDigest {
    tick: u64,
    kind_keys: std::collections::BTreeSet<&'static str>,
    has_market_snap: bool,
    has_active_candle: bool,
    has_auction_points: bool,
    continuous_points: usize,
}

fn digest(frame: &super::TickFrame) -> FrameDigest {
    let mut kinds = std::collections::BTreeMap::new();
    for fact in &frame.facts {
        let name = match &fact.event {
            crate::Event::Trade { .. } => "Trade",
            crate::Event::PriceTick { .. } => "PriceTick",
            crate::Event::AuctionTick { .. } => "AuctionTick",
            crate::Event::AuctionCompleted { .. } => "AuctionCompleted",
            crate::Event::OrderAccepted { .. } => "OrderAccepted",
            crate::Event::OrderCanceled { .. } => "OrderCanceled",
            crate::Event::IntentRejected { .. } => "IntentRejected",
            crate::Event::DayBoundary { .. } => "DayBoundary",
            _ => "Other",
        };
        *kinds.entry(name).or_insert(0) += 1;
    }
    FrameDigest {
        tick: frame.tick,
        kind_keys: kinds.into_keys().collect(),
        has_market_snap: frame.timeseries_payload.markets.len() == 2,
        has_active_candle: !frame.timeseries_payload.active_daily_candles.is_empty(),
        has_auction_points: !frame.timeseries_payload.auction_points.is_empty(),
        continuous_points: frame.timeseries_payload.continuous_points.len(),
    }
}

/// runtime 分叉与旧管线同 fixture 对照：多拍驱动（越过竞价窗+跨日界），
/// 每拍断言两路径的契约面逐字段相等（事件**类型集合**——数量受 NPC
/// 决策流差异影响不做逐位断言，类型构成与帧结构必须一致）。
#[test]
fn stepframe4_runtime_and_legacy_frame_contract_faces_match() {
    for (auction_ticks, ticks) in [(0u64, 8u64), (6, 30)] {
        parity_scenario(auction_ticks, ticks);
    }
}

/// Sol 终审整改 2：**真实成交 parity**——同 fixture 同 seed 两路径各经
/// 真实撮合成交（NPC 卖腿经 seed_order_for_test 挂旧簿：legacy 臂直接
/// 消费、runtime 臂构造期 adopt 迁入；玩家买腿同拍注入），期末断言
/// 玩家**现金/持仓/T+1 冻结/成本基**四面逐位相等，且与独立推算
/// （成交额+费用口径）一致。
#[test]
fn solfinal2_real_trade_parity_cash_position_t1_cost() {
    // Sol 过程 review 补强：确认 runtime 臂真的启用——环境变量=0 时
    // 两臂都走旧路径（parity 变成旧对旧的假绿），此处显式断言分叉开
    // 启；=0 模式下本对照跳过（两臂同径无判别力，如实报告）。
    if !crate::runtime::step_runtime_enabled() {
        eprintln!("ENGINE_STEP_RUNTIME=0：两臂同走旧管线，成交 parity 无判别力——跳过（默认模式的运行承载真对照）");
        return;
    }
    let mut make = || {
        let code = crate::StockId(0);
        let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
        // retail NPC（经历账在 retail_experience——fixture 经历注入面可用；
        // inst 的经历域另构，不混入本对照）。
        setup.npcs.inst_count = 0;
        setup.npcs.retail_count = 1;
        let mut session = ProtocolSession::new(setup, 62).unwrap();
        // NPC 卖腿的持仓基（fixture 注入——两臂同构同值）。
        let npc = crate::AccountId(1);
        let mut positions = std::collections::BTreeMap::new();
        positions.insert(
            crate::StockId(0),
            crate::account::Position::from_restored_parts(1_000, 0, 1_000_000, 0),
        );
        session
            .game_mut()
            .fixture_account_mut(&npc)
            .expect("NPC 在名册")
            .fixture_set_positions(positions);
        // 持仓经历条目配套（旧管线 SettlementShadow 要求 active holding
        /// epoch——fixture 直设持仓绕过建仓链时需补；两臂同构）。
        session
            .game_mut()
            .runtime_hold_experience_init(&npc, &code, 1_000, 1_000)
            .expect("经历条目初始化");
        session
    };
    let mut rt = make();
    let mut legacy = make();
    let code = crate::StockId(0);
    // 玩家步前现金（两臂同构——独立推算基线动态取，不硬编码）。
    let player_cash_before = legacy.game().save().expect("存档").snapshot.accounts[&crate::AccountId(0)].cash.cents();
    // 卖方步前现金（卖方现金 oracle 基线）。
    let rt_seller_cash_before = rt.game().save().expect("存档").snapshot.accounts[&crate::AccountId(1)].cash.cents();
    let legacy_seller_cash_before = legacy.game().save().expect("存档").snapshot.accounts[&crate::AccountId(1)].cash.cents();
    assert_eq!(rt_seller_cash_before, legacy_seller_cash_before, "卖方步前现金两臂同构");
    // NPC 卖 100@1000 挂旧簿（两臂同拍同形；受理事件面不在断言内）。
    let mut events = Vec::new();
    rt.game_mut()
        .seed_order_for_test(crate::AccountId(1), sell_limit(&code, 1000, 100), &mut events);
    let mut events = Vec::new();
    legacy.game_mut().seed_order_for_test(
        crate::AccountId(1),
        sell_limit(&code, 1000, 100),
        &mut events,
    );
    // 玩家买 100@1000（两臂同拍入队）。
    for session in [&mut rt, &mut legacy] {
        session
            .enqueue_player_intent(
                crate::AccountId(0),
                crate::Intent::PlaceLimit {
                    code,
                    side: crate::Side::Buy,
                    price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(1000)),
                    qty: 100,
                },
            )
            .expect("玩家意图入队");
    }
    // 两臂各走生产路径驱动（legacy=旧管线 contract 入口；rt=分叉）。
    let legacy_frame = legacy
        .step_frame_pipeline_contract()
        .expect("legacy 步进（旧簿撮合）");
    let rt_frame = rt.step_frame().expect("runtime 步进（adopt 迁入+desk 撮合）");
    rt_frame.validate().expect("runtime 帧校验");
    // 成交确实发生（两臂各恰一笔 Trade 100@1000）。
    for (name, frame) in [("legacy", &legacy_frame), ("rt", &rt_frame)] {
        let trades: Vec<_> = frame
            .facts
            .iter()
            .filter(|f| matches!(&f.event, crate::Event::Trade { qty, .. } if *qty == 100))
            .collect();
        assert_eq!(trades.len(), 1, "{name} 臂恰一笔成交：{trades:?}");
    }
    // 买方四面断言（现金/持仓/T+1/成本基逐位相等）。
    let snap = |s: &ProtocolSession, id: crate::AccountId| {
        let a = s.game().save().expect("存档读面").snapshot.accounts[&id].clone();
        (
            a.cash.cents(),
            a.positions
                .get(&code)
                .map(|p| (p.qty, p.t1_locked, p.invested_cents, p.recovered_cents)),
        )
    };
    let player = crate::AccountId(0);
    let npc = crate::AccountId(1);
    let rt_player = snap(&rt, player);
    let legacy_player = snap(&legacy, player);
    assert_eq!(
        rt_player, legacy_player,
        "买方四面（现金/持仓/T+1/成本基）两路径逐位相等"
    );
    let (cash, position) = rt_player;
    let (qty, t1, invested, recovered) = position.expect("玩家持仓建立");
    assert_eq!((qty, t1), (100, 100), "持仓 100 且当日 T+1 全冻结");
    assert_eq!(invested, 100_000, "成本基=成交额（费不入 invested 口径）");
    assert_eq!(recovered, 0);
    // Sol 过程 review 补强：费用=精确值（引擎费则手算 oracle——
    // 成交额 100_000 分：佣金万 2.5 最低 500 分、过户费双边 0.01‰=1 分）。
    let commission = 500i64;
    let transfer = 1i64;
    assert_eq!(
        cash,
        player_cash_before - 100_000 - commission - transfer,
        "买方现金=步前−成交额−佣金 500−过户 10（精确 oracle）"
    );
    // Sol 过程 review 补强：卖方三面（现金/余仓/成本基 recovered）。
    let rt_seller = snap(&rt, npc);
    let legacy_seller = snap(&legacy, npc);
    assert_eq!(
        rt_seller, legacy_seller,
        "卖方三面（现金/余仓/成本基）两路径逐位相等"
    );
    let (seller_cash, seller_position) = rt_seller;
    let (seller_qty, seller_t1, seller_invested, seller_recovered) =
        seller_position.expect("卖方余仓在场");
    assert_eq!(
        (seller_qty, seller_t1),
        (900, 0),
        "卖方余仓 900（1000−100），T+1 不涉（非当日买入）"
    );
    assert_eq!(seller_recovered, 100_000, "卖方 recovered=成交额（费不入账口径）");
    // 卖方现金=步前+净得（成交额−佣金−印花税；印花税=卖方 0.5‰=50 分）。
    let stamp_tax = 50i64;
    // 净得=100_000−500−50−1（佣金/印花/过户）。
    let seller_net = 100_000 - commission - stamp_tax - transfer;
    assert!(
        cash >= 0 && seller_cash >= 0,
        "双侧现金非负：买 {cash} 卖 {seller_cash}"
    );
    // 卖方现金精确 oracle：步前+净得（步前经对称注入前的会话读取）。
    let rt_seller_before = rt_seller_cash_before;
    assert_eq!(
        seller_cash,
        rt_seller_before + seller_net,
        "卖方现金=步前+净得 99_449（成交额−佣金 500−印花 50−过户 1）"
    );
    let _ = seller_invested;
}

fn sell_limit(code: &crate::StockId, price: i64, qty: u32) -> crate::Intent {
    crate::Intent::PlaceLimit {
        code: code.clone(),
        side: crate::Side::Sell,
        price: crate::strategy::LimitPrice::Fixed(crate::money::Money::from_cents(price)),
        qty,
    }
}

fn parity_scenario(auction_ticks: u64, ticks: u64) {
    // runtime 分叉臂（step_frame 经分叉入口）。
    let mut rt = ProtocolSession::new(parity_setup(auction_ticks), 62).unwrap();
    // 旧管线臂（直调 execute_authoritative_tick_with_projection——与
    // step_frame_pipeline_contract 同式，但本测试在 protocol 模块内
    // 直调内部入口保持两臂同处一线）。
    let mut legacy = ProtocolSession::new(parity_setup(auction_ticks), 62).unwrap();
    let code = crate::StockId(0);
    for t in 1..=ticks {
        // 玩家限价单（两臂同拍同序 enqueue——受理事件面的确定对照源；
        // 无对手（零 NPC）→挂簿不成交，PhaseTick/受理构成稳定对照）。
        for session in [&mut rt, &mut legacy] {
            session
                .enqueue_player_intent(
                    crate::AccountId(0),
                    crate::Intent::PlaceLimit {
                        code,
                        side: crate::Side::Buy,
                        price: crate::strategy::LimitPrice::Fixed(
                            crate::money::Money::from_cents(1000),
                        ),
                        qty: 100,
                    },
                )
                .expect("玩家意图入队");
        }
        let rt_frame = rt.step_frame().expect("runtime 分叉步进");
        rt_frame.validate().expect("runtime 帧校验");
        let legacy_frame = legacy
            .step_frame_pipeline_contract()
            .expect("旧管线步进");
        for fact in &legacy_frame.facts {
            if matches!(fact.event, crate::Event::IntentRejected { .. }) {
                eprintln!("tick {t} legacy rejected: {:?}", fact.event);
            }
        }
        let rt_digest = digest(&rt_frame);
        let legacy_digest = digest(&legacy_frame);
        assert_eq!(rt_digest.tick, legacy_digest.tick, "tick {t}：帧 tick 相等");
        assert_eq!(
            rt_digest.kind_keys, legacy_digest.kind_keys,
            "tick {t}：事件类型集合相等（数量非等价面——受理/竞价事件密度形态不同）：rt={:?} legacy={:?}",
            rt_digest.kind_keys, legacy_digest.kind_keys
        );
        assert_eq!(
            rt_digest.has_market_snap, legacy_digest.has_market_snap,
            "tick {t}：双股行情快照在场"
        );
        assert_eq!(
            rt_digest.has_active_candle, legacy_digest.has_active_candle,
            "tick {t}：活动 K 在场性一致"
        );
        assert_eq!(
            rt_digest.has_auction_points, legacy_digest.has_auction_points,
            "tick {t}：竞价时序点在场性一致"
        );
        assert_eq!(
            rt_digest.continuous_points, legacy_digest.continuous_points,
            "tick {t}：连续点覆盖股数一致"
        );
    }
    // 期末权威面：两路径的玩家账户快照一致（账户面等价）。
    let rt_snap = rt.game().runtime_snapshot();
    let legacy_snap = legacy.game().runtime_snapshot();
    assert_eq!(rt_snap.tick, legacy_snap.tick, "期末 tick 一致");
    assert_eq!(rt_snap.day, legacy_snap.day, "期末 day 一致");
    let rt_player = rt_snap.accounts.get(&crate::AccountId(0));
    let legacy_player = legacy_snap.accounts.get(&crate::AccountId(0));
    assert_eq!(
        rt_player.map(|a| a.cash.cents()),
        legacy_player.map(|a| a.cash.cents()),
        "期末玩家现金一致"
    );
}

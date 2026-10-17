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

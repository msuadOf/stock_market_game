//! 撮合域事件桥（批 8b 项 2 地基）：runtime 双腿成交事实 → 旧引擎
//! `Event::Trade` 形状 + 协调者全局 seq 分配。
//!
//! **覆盖面（诚实登记）**：Trade 事件的 seq 分配与字段映射（TickFrame
//! 撮合域核心）；事件稳定键（`EventStableKey` 身份域）、撤单/到期/竞价
//! 事件类型、`TickTimeseriesPayload`（MarketSnap/DailyCandle/AuctionPoint/
//! ContinuousPoint）与 `EventFact` 完整组装留给批 8c——切换 `step_frame`
//! 前必须补齐（2600 集成测试锚定该契约）。
//!
//! 语义对齐：`Event::Trade{seq 单调, code 路由, price, qty, maker, taker}`
//! （session.rs `Event` 枚举）；seq 由协调者唯一分配（F §6.1）。

use super::protocol::DeskTradeFact;
use crate::Event;
use crate::StockId;

/// 把一批双腿成交事实映射为 `Event::Trade`（按事实序；seq 分配器
/// **先加后取**——第一条取得 `next_seq+1`，与旧管线 `collect_events`
/// 同式，游标语义=最后已用 seq）。返回 (events, next_seq_after)。
///
/// maker/taker 的 runtime `Owner=(组,名册下标)` → 引擎 `AccountId`：
/// 批 8c 账户面接线前的映射缺省为 `AccountId(组<<32|下标)` 复合——
/// 语义为"runtime 内部身份的稳定投影"，与旧引擎账户 id 空间的**对齐**
/// 属账户面接线（登记）。
pub fn project_trade_events(trades: &[DeskTradeFact], next_seq: &mut u64) -> Vec<Event> {
    let mut events = Vec::with_capacity(trades.len());
    for t in trades {
        let seq = take_seq(next_seq);
        events.push(Event::Trade {
            seq,
            code: StockId(u32::from(t.stock)),
            price: crate::money::Money::from_cents(t.price),
            qty: t.qty,
            maker: owner_to_account(t.maker),
            taker: owner_to_account(t.taker),
        });
    }
    events
}

/// seq 分配原语（step_frame 接轨件 2）：**先加后取**——第一条取得
/// 游标+1，终值=最后已用 seq（对齐旧管线 `collect_events(facts,
/// state.seq)` 的契约——TickFrame 半开区间 `(seq_from, seq_to]`，
/// 空批游标不动）。此前"先取后加"使 runtime 事件全体比旧管线少 1。
fn take_seq(next_seq: &mut u64) -> u64 {
    *next_seq = next_seq
        .checked_add(1)
        .expect("事件 seq 溢出（u64 单调计数）");
    *next_seq
}

/// runtime Owner → 引擎 AccountId（复合投影；账户面对齐留批 8c）。
pub fn owner_to_account(owner: super::Owner) -> crate::AccountId {
    crate::AccountId((u64::from(owner.0) << 32) | u64::from(owner.1))
}

// ═══════════════════════════════════════════════════════════════════
// 批 8c：TickTimeseriesPayload 映射（撮合域）
// ═══════════════════════════════════════════════════════════════════

use super::protocol::StockPublic;
use crate::money::Money;
use crate::session::protocol::ContinuousPoint;
use crate::{DailyCandle, MarketSnap, TradingPhase};

/// 公共态 → 旧引擎 `MarketSnap`（last/昨收/买卖一/五档深度）。
/// `last_close` 由交易部日界重置维护，公共态不携带——调用方从 DeskFinal
/// market 或存档基线传入（批 8c：公共态扩 last_close 留映射完备化）。
pub fn project_market_snap(view: &StockPublic, last_close: i64) -> MarketSnap {
    MarketSnap {
        last_price: Money::from_cents(view.last_price),
        last_close: Money::from_cents(last_close.max(1)),
        best_bid: view.best_bid.map(Money::from_cents),
        best_ask: view.best_ask.map(Money::from_cents),
        bids: view
            .depth_bids
            .iter()
            .map(|(p, q)| (Money::from_cents(*p), *q))
            .collect(),
        asks: view
            .depth_asks
            .iter()
            .map(|(p, q)| (Money::from_cents(*p), *q))
            .collect(),
    }
}

/// runtime 日 K → 旧引擎 `DailyCandle`（time=日终 tick 近似，登记）。
pub fn project_daily_candle(candle: super::desk::DeskCandle, time: i64) -> DailyCandle {
    DailyCandle {
        time,
        open: Money::from_cents(candle.open),
        high: Money::from_cents(candle.high),
        low: Money::from_cents(candle.low),
        close: Money::from_cents(candle.close),
        volume: candle.volume,
        trade_stats: None,
    }
}

/// 公共态 → `ContinuousPoint`（连续竞价点：tick/相位/最新价/累计量）。
pub fn project_continuous_point(
    view: &StockPublic,
    tick: u64,
    phase: TradingPhase,
) -> ContinuousPoint {
    ContinuousPoint {
        tick,
        phase,
        last_price: Money::from_cents(view.last_price),
        cumulative_volume: view.volume_today,
        bids: view
            .depth_bids
            .iter()
            .map(|(p, q)| (Money::from_cents(*p), *q))
            .collect(),
        asks: view
            .depth_asks
            .iter()
            .map(|(p, q)| (Money::from_cents(*p), *q))
            .collect(),
    }
}

// ═══════════════════════════════════════════════════════════════════
// 批 8e：撤单/到期/受理事件映射（事件契约补完）
// ═══════════════════════════════════════════════════════════════════

use super::protocol::DeskLifecycleFact;
use super::{DeskReceipt, Owner, StockIdx};
use crate::orderbook::OrderId;
use crate::{RejectionReason, Side};

/// 生命周期事件事实（映射输入聚合）。
#[derive(Clone, Debug)]
pub struct LifecycleEvents {
    /// OrderCanceled 输入。
    pub cancels: Vec<(Owner, StockIdx, u64, u32)>,
    /// OrderAccepted 输入。
    pub accepts: Vec<(Owner, StockIdx, u64, Side, i64, u32)>,
    /// IntentRejected 输入。
    pub rejections: Vec<(Owner, StockIdx, u64, RejectionReason)>,
}

/// 从交易部生命周期事实+回执聚合映射输入。
pub fn collect_lifecycle(facts: &[DeskLifecycleFact], receipts: &[DeskReceipt]) -> LifecycleEvents {
    let mut out = LifecycleEvents {
        cancels: vec![],
        accepts: vec![],
        rejections: vec![],
    };
    for f in facts {
        match f.kind {
            super::protocol::LifecycleKind::Cancelled => {
                out.cancels
                    .push((f.owner, f.stock, f.order_serial, f.remaining))
            }
            super::protocol::LifecycleKind::Expired => {
                out.cancels
                    .push((f.owner, f.stock, f.order_serial, f.remaining))
            }
            super::protocol::LifecycleKind::Accepted => out.accepts.push((
                f.owner,
                f.stock,
                f.order_serial,
                f.side,
                f.price,
                f.remaining,
            )),
        }
    }
    for r in receipts {
        out.rejections
            .push((r.owner, r.order.0, r.order.1, rejection_reason(r.kind)));
    }
    out
}

/// `ReceiptKind`（runtime 域）→ `RejectionReason`（引擎事件域）。
pub fn rejection_reason(kind: super::ReceiptKind) -> RejectionReason {
    use super::ReceiptKind as RK;
    match kind {
        RK::PriceOutOfBand => RejectionReason::LimitExceeded,
        RK::NotRoundLot => RejectionReason::InvalidQuantity,
        RK::ExceedsOrderCap => RejectionReason::InvalidQuantity,
        RK::PhaseRejected => RejectionReason::AuctionOrderEntryClosed,
        RK::CancelWindowClosed => RejectionReason::AuctionOrderNotCancelable,
        RK::AlreadyFilled => RejectionReason::OrderAlreadyFilled,
        RK::AuctionLimitOrderRequired => RejectionReason::AuctionLimitOrderRequired,
        RK::OrderNotFound => RejectionReason::OrderNotFound,
        RK::InsufficientCash => RejectionReason::InsufficientCash,
        RK::InsufficientShares => RejectionReason::InsufficientShares,
    }
}

/// 生命周期事实 → 引擎事件（撤单/到期→OrderCanceled；受理→OrderAccepted；
/// 回执→IntentRejected）。OrderRef 复合标识 `(stock, serial)` → 引擎
/// `OrderId`（serial 低 32 位复合——批 8e 近似映射，登记：与旧引擎
/// OrderId 空间对齐属存档映射批）。
pub fn project_lifecycle_events(input: &LifecycleEvents, next_seq: &mut u64) -> Vec<Event> {
    let mut events = Vec::new();
    // step_frame 接轨件 2：先加后取（同 take_seq——对齐旧管线 collect 语义）。
    let take = |next_seq: &mut u64| -> u64 {
        *next_seq = next_seq.checked_add(1).expect("事件 seq 溢出");
        *next_seq
    };
    for (owner, stock, serial, remaining) in &input.cancels {
        events.push(Event::OrderCanceled {
            seq: take(next_seq),
            account: owner_to_account(*owner),
            code: StockId(u32::from(*stock)),
            id: OrderId(*serial),
            remaining_qty: *remaining,
        });
    }
    for (owner, stock, serial, side, price, remaining) in &input.accepts {
        events.push(Event::OrderAccepted {
            seq: take(next_seq),
            account: owner_to_account(*owner),
            code: StockId(u32::from(*stock)),
            id: OrderId(*serial),
            side: *side,
            price: Money::from_cents(*price),
            remaining_qty: *remaining,
        });
    }
    for (owner, stock, serial, reason) in &input.rejections {
        events.push(Event::IntentRejected {
            seq: take(next_seq),
            account: owner_to_account(*owner),
            code: StockId(u32::from(*stock)),
            reason: reason.clone(),
        });
    }
    events
}

// ═══════════════════════════════════════════════════════════════════
// 批 8f：竞价事件映射（AuctionTick/AuctionCompleted）
// ═══════════════════════════════════════════════════════════════════

use super::protocol::DeskAuctionFact;

/// 竞价事实 → 引擎事件（竞价窗每轮 AuctionTick；完成撮合 AuctionCompleted，
/// clearing_price 显式携带）。seq 分配先加后取（同 take_seq——旧管线
/// collect 语义）。
pub fn project_auction_events(facts: &[DeskAuctionFact], next_seq: &mut u64) -> Vec<Event> {
    let mut events = Vec::with_capacity(facts.len());
    for f in facts {
        let seq = take_seq(next_seq);
        if f.completed {
            events.push(Event::AuctionCompleted {
                seq,
                tick: f.tick,
                phase: f.phase,
                code: StockId(u32::from(f.stock)),
                clearing_price: f.indicative_price.map(Money::from_cents),
                matched_volume: f.matched_volume,
            });
        } else {
            events.push(Event::AuctionTick {
                seq,
                tick: f.tick,
                phase: f.phase,
                code: StockId(u32::from(f.stock)),
                indicative_price: f.indicative_price.map(Money::from_cents),
                matched_volume: f.matched_volume,
                imbalance: f.imbalance,
            });
        }
    }
    events
}

/// TickFrame 组装产物（批 8h：事件+时序快照串联面）。
///
/// `events`：已映射引擎事件（seq 单调）；`views`：本收束点公共态
/// （MarketSnap/ContinuousPoint/DailyCandle 映射输入）。稳定键/
/// EventFact 化留翻转最后一关（登记）。
pub struct FrameProjection {
    /// 引擎事件（seq 单调序）。
    pub events: Vec<Event>,
    /// 全股公共态（时序映射输入）。
    pub views: Vec<super::protocol::StockPublic>,
}

// ═══════════════════════════════════════════════════════════════════
// 批 8h·部分2：EventFact 化（local_event_index 语义复刻）
// ═══════════════════════════════════════════════════════════════════

/// (phase_rank, entity, source) 三元分组键——`EventStableKey` 身份域。
/// 旧管线的 sealed_index 是**受理序**（每账户单调）——runtime 的等价物
/// 是 OrderRef.1（交易部本地单调序号，全局唯一）：同一身份域内单调递增，
/// 满足稳定键唯一性合同（`validate_unique_keys`）。
pub fn event_facts_with_stable_keys(
    events: Vec<Event>,
) -> Vec<crate::session::protocol::EventFact> {
    use crate::session::pipeline::EventStableKey;
    // 身份域内分配器；runtime 复刻口径：
    let mut counters: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    events
        .into_iter()
        .map(|event| {
            let bucket = identity_bucket(&event);
            let index = counters.entry(bucket).or_insert(0);
            let idx = *index;
            *index += 1;
            crate::session::protocol::EventFact {
                key: EventStableKey::for_event(&event, idx),
                event,
            }
        })
        .collect()
}

/// 事件的身份域桶名（for_event 三元映射的手工复刻：Trade→股域 Sealed、
/// 竞价/行情→股域 PriceTick、账户面四变体（受理/撤单/拒收/结算错误）→
/// **账户域 Sealed**——域不含 code，同账户跨股事件共享局部序号链；
/// 此前按 (account, code) 分桶使同账户跨股事件各自 idx=0 → 稳定键
/// 重复（双股日结拍 drained 撤单显红））。
fn identity_bucket(event: &Event) -> String {
    match event {
        Event::Trade { code, .. } => format!("trade:{code:?}"),
        Event::AuctionTick { code, .. }
        | Event::AuctionCompleted { code, .. }
        | Event::PriceTick { code, .. } => format!("market:{code:?}"),
        Event::OrderCanceled { account, .. }
        | Event::OrderAccepted { account, .. }
        | Event::IntentRejected { account, .. }
        | Event::SettlementError { account, .. } => format!("account_sealed:{account:?}"),
        other => format!("other:{other:?}"),
    }
}

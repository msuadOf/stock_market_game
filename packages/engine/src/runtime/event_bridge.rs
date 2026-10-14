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

/// 把一批双腿成交事实映射为 `Event::Trade`（按事实序；seq 从
/// `next_seq` 起单调分配）。返回 (events, next_seq_after)。
///
/// maker/taker 的 runtime `Owner=(组,名册下标)` → 引擎 `AccountId`：
/// 批 8c 账户面接线前的映射缺省为 `AccountId(组<<32|下标)` 复合——
/// 语义为"runtime 内部身份的稳定投影"，与旧引擎账户 id 空间的**对齐**
/// 属账户面接线（登记）。
pub fn project_trade_events(trades: &[DeskTradeFact], next_seq: &mut u64) -> Vec<Event> {
    let mut events = Vec::with_capacity(trades.len());
    for t in trades {
        let seq = *next_seq;
        *next_seq = seq.checked_add(1).expect("事件 seq 溢出（u64 单调计数）");
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

/// runtime Owner → 引擎 AccountId（复合投影；账户面对齐留批 8c）。
pub fn owner_to_account(owner: super::Owner) -> crate::AccountId {
    crate::AccountId((u64::from(owner.0) << 32) | u64::from(owner.1))
}

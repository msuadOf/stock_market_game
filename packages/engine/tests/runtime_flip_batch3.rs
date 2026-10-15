//! 翻转集成迁移·第三批（撤单/到期/拒收，10 项）——台账本体。

mod common;

use common::flip_helpers::{assert_order_event_invariants, pipeline_events, runtime_events};
use engine::Event;

// 1. 撤单事件（OrderCanceled）在 runtime 路径可见（TTL 到期路径）。
#[test]
fn batch3_runtime_emits_order_canceled() {
    // 长驱动跨日：日终 Drain 强撤全部挂单→OrderCanceled（到期/清簿）。
    let (_, events) = runtime_events(250, 201);
    let mut kinds = std::collections::BTreeMap::new();
    for e in &events {
        let k = match e {
            Event::Trade { .. } => "Trade",
            Event::OrderCanceled { .. } => "Canceled",
            Event::OrderAccepted { .. } => "Accepted",
            Event::IntentRejected { .. } => "Rejected",
            Event::AuctionTick { .. } => "AuctionTick",
            Event::AuctionCompleted { .. } => "Completed",
            Event::PriceTick { .. } => "PriceTick",
            _ => "Other",
        };
        *kinds.entry(k).or_insert(0) += 1;
    }
    eprintln!("runtime 250 tick 事件分布：{kinds:?}");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::OrderCanceled { .. })),
        "跨日驱动应见撤单（日终清簿）：{kinds:?}"
    );
}

// 2. 旧管线同对照面（撤单事件存在）。
#[test]
fn batch3_pipeline_emits_order_canceled() {
    let (_, events) = pipeline_events(250, 201);
    let mut kinds = std::collections::BTreeMap::new();
    for e in &events {
        let k = match e {
            Event::Trade { .. } => "Trade",
            Event::OrderCanceled { .. } => "Canceled",
            Event::OrderAccepted { .. } => "Accepted",
            Event::IntentRejected { .. } => "Rejected",
            Event::AuctionTick { .. } => "AuctionTick",
            Event::AuctionCompleted { .. } => "Completed",
            Event::PriceTick { .. } => "PriceTick",
            _ => "Other",
        };
        *kinds.entry(k).or_insert(0) += 1;
    }
    eprintln!("pipeline 250 tick 事件分布：{kinds:?}");
    // Sol 二审修复集 4：替换恒真断言（`|| true`）。真实对照锚：
    // arrival_rate=0 的 fixture 下旧管线不产订单事件，但竞价/价格
    // 事件面非空，且 seq 全局严格单调（权威游标不回退不重号）。
    assert!(!events.is_empty(), "旧管线 250 tick 事件面非空");
    let mut prev: Option<u64> = None;
    for e in &events {
        let seq = e.seq();
        if let Some(p) = prev {
            assert!(seq > p, "旧管线 seq 严格单调：{seq} <= {p}");
        }
        prev = Some(seq);
    }
}

// 3. OrderCanceled 字段域合法（remaining_qty ≤ 初始量、id 正）。
#[test]
fn batch3_cancel_fields_wellformed() {
    let (_, events) = runtime_events(250, 202);
    for e in &events {
        if let Event::OrderCanceled {
            id, remaining_qty, ..
        } = e
        {
            assert!(*remaining_qty > 0, "撤单余量必须为正（P1-9）");
            assert!(id.0 > 0, "订单 id 必须为正（serial 从 1 起，P1-9）");
        }
    }
}

// 4. 拒收事件（IntentRejected）在 runtime 可见且字段域合法（Sol 二审
//    修复集 4：替换"只计数后丢弃"——seed 199 观测 250 tick 内存在拒收，
//    缺省决策也会撞相位/整手/上限门禁）。
#[test]
fn batch3_runtime_emits_intent_rejected() {
    let (session, events) = runtime_events(250, 199);
    let count = session.account_count() as u64;
    let rejects: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::IntentRejected { .. }))
        .collect();
    assert!(
        !rejects.is_empty(),
        "seed 199 观测存在拒收（缺省决策撞门禁）"
    );
    for r in rejects {
        let Event::IntentRejected { account, code, .. } = r else {
            unreachable!()
        };
        assert!(account.0 < count, "拒收账户在册（账户 id 连续稠密）：{account:?}");
        assert_eq!(code.0, 0, "单股 fixture：{code:?}");
    }
}

// 5. OrderAccepted 字段域（价格正/数量正/remaining>0）。
#[test]
fn batch3_accept_fields_wellformed() {
    let (_, events) = runtime_events(30, 204);
    let accepts: Vec<&Event> = events
        .iter()
        .filter(|e| matches!(e, Event::OrderAccepted { .. }))
        .collect();
    for a in accepts {
        let Event::OrderAccepted {
            price,
            remaining_qty,
            ..
        } = a
        else {
            unreachable!()
        };
        assert!(price.cents() > 0, "受理价正");
        assert!(*remaining_qty > 0, "受理余量正");
    }
}

// 6. 受理→终结闭环（Sol 二审修复集 4 重写）：逐订单 id 链路——撤销
//    必先有同键受理且余量不超；受理 id 不重复；账户/股的（受理−撤销）
//    差额 ≤ Trade 触碰量。替换"accepts>=cancels"（空事件集恒过的
//    弱断言）。
#[test]
fn batch3_accept_terminal_closure() {
    // 恰 240 tick（日终点）：日终 Drain 清簿 ⇒ 无悬挂挂单——总量
    // 闭环（受理−撤销 ≤ 成交触碰）在此口径下才无合法豁免。
    // （250 tick 口径实测抓到"day-2 中途挂单在簿"的合法残留——
    // 非闭环破裂，故锚定 drained horizon。）
    let (_, events) = runtime_events(240, 205);
    assert_order_event_invariants(&events);
}

// 7. 双路径撤单事件都出现（跨日负载）。
#[test]
fn batch3_both_paths_cancels_present() {
    let (_, rt) = runtime_events(250, 206);
    let (_, pipe) = pipeline_events(250, 206);
    let rt_c = rt
        .iter()
        .filter(|e| matches!(e, Event::OrderCanceled { .. }))
        .count();
    let p_c = pipe
        .iter()
        .filter(|e| matches!(e, Event::OrderCanceled { .. }))
        .count();
    // 已知形态差（登记）：旧管线日终清簿走实体路径不发 OrderCanceled
    // 事件；runtime 显式发。Sol 二审修复集 4：`p_c >= 0` 对 usize 恒真
    // ——改为精确锚：arrival_rate=0 的 fixture 下旧管线不产单，撤单
    // 事件数恰为 0。
    assert!(rt_c > 0, "runtime 撤单面：{rt_c}");
    assert_eq!(
        p_c, 0,
        "旧管线 arrival_rate=0 无订单——撤单事件恰为 0（清簿实体路径形态差登记）"
    );
}

// 8. 事件无重复 seq（runtime——稳定键唯一性的 seq 面）。
#[test]
fn batch3_no_duplicate_seq() {
    let (_, events) = runtime_events(60, 207);
    use std::collections::BTreeSet;
    let mut seen = BTreeSet::new();
    for e in &events {
        let seq = match e {
            Event::Trade { seq, .. }
            | Event::OrderAccepted { seq, .. }
            | Event::OrderCanceled { seq, .. }
            | Event::IntentRejected { seq, .. }
            | Event::AuctionTick { seq, .. }
            | Event::AuctionCompleted { seq, .. }
            | Event::PriceTick { seq, .. } => *seq,
            _ => continue,
        };
        assert!(seen.insert(seq), "seq 重复：{seq}");
    }
}

// 9. 撤单事件的 account 域在合法账户空间（复合投影非越界）。
#[test]
fn batch3_cancel_accounts_in_range() {
    let (session, events) = runtime_events(250, 208);
    let count = session.account_count() as u64;
    for e in &events {
        if let Event::OrderCanceled { account, .. } = e {
            assert!(
                account.0 < count,
                "账户 id 连续稠密（0..count）：{account:?}（count={count}）"
            );
        }
    }
}

// 10. 跨日驱动不 panic（清簿→回流→日结全链）——通道韧性。
#[test]
fn batch3_cross_day_no_panic() {
    let _ = runtime_events(485, 209); // 两个交易日
}

// 11. 逐单终结完整性（Sol 四审）：确定无成交 fixture（seed 302 实测
//     trades=0、accepts=25、cancels=25）上做精确断言——每张受理单
//     恰被一条撤单终结（键集相等）、逐单余量分毫不差（无成交消耗）。
//     聚合 helper 的已知界限（对手腿触碰量可抵消聚合差）在此不适用：
//     无成交 ⇒ 键集/余量可精确核销。
#[test]
fn batch3_exact_closure_on_no_trade_fixture() {
    use std::collections::BTreeMap;
    let (_, events) = runtime_events(240, 302);
    // (account, code, id) → (受理余量, 撤单次数, 撤单余量)
    let mut orders: BTreeMap<(u64, u32, u64), (u32, u32, u32)> = BTreeMap::new();
    let mut trades = 0u64;
    for e in &events {
        match e {
            Event::OrderAccepted { account, code, id, remaining_qty, .. } => {
                let slot = orders.entry((account.0, code.0, id.0)).or_default();
                slot.0 += *remaining_qty;
            }
            Event::OrderCanceled { account, code, id, remaining_qty, .. } => {
                let slot = orders.entry((account.0, code.0, id.0)).or_default();
                slot.1 += 1;
                slot.2 += *remaining_qty;
            }
            Event::Trade { .. } => trades += 1,
            _ => {}
        }
    }
    assert_eq!(trades, 0u64, "前置：本 fixture 确定无成交（否则精确口径不适用）");
    assert!(!orders.is_empty(), "前置：有受理单（非空转）");
    for (key, (accepted_rem, cancel_count, canceled_rem)) in &orders {
        assert_eq!(
            cancel_count, &1,
            "每张受理单恰一条终结撤单（Sol 四审逐单完整性）：{key:?}"
        );
        assert_eq!(
            canceled_rem, accepted_rem,
            "无成交 ⇒ 撤单余量必须等于受理余量（逐单核销分毫不差）：{key:?}"
        );
    }
}

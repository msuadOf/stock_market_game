//! 只读 causal sideband：保留单股实际操作顺序，不参与撮合或资源转移。

use super::{OrderRef, Owner, StockIdx};
use crate::Side;

/// 本轮真实 NPC 观察；Owner 与 kind 由 Session 对真实名册再次校验。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupDecisionTrace {
    pub owner: Owner,
    pub kind: crate::AccountKind,
    pub record: std::sync::Arc<crate::diagnostics::NpcDecisionTraceRecord>,
    /// 当次真实根与 AllocationResult 的原事实；Echo 不生产此增量。
    pub causal_facts: Vec<crate::diagnostics::causal::CausalFactKind>,
    /// 同一账户在本次 review/open/quote 之前的真实计划状态；仅候选阶段消费。
    pub plan_before: std::collections::BTreeMap<crate::PlanId, String>,
}

/// 实际生产路径携带的来源；未接通路径仍显式拒绝。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationSource {
    PlayerExternal,
    InstitutionPlan { plan: crate::PlanId, observed_tick: u64 },
    ObservedNpc { kind: crate::AccountKind, observed_tick: u64 },
    InstitutionCancel,
    NonPlayerUnresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskQuote {
    pub bid_cents: Option<i64>,
    pub ask_cents: Option<i64>,
    pub bid_depth: u64,
    pub ask_depth: u64,
}

/// 从真实 DeskOrder 在累计更新前采集的单腿事实。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskExecutionLeg {
    pub order: OrderRef,
    pub owner: Owner,
    pub value_before: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeskCausalKind {
    Submitted {
        order: OrderRef,
        owner: Owner,
        side: Side,
        qty: u32,
        source: OperationSource,
        before: DeskQuote,
    },
    Cancelled {
        order: OrderRef,
        owner: Owner,
        qty: u32,
        source: OperationSource,
    },
    Execution {
        ordinal: u64,
        seller: DeskExecutionLeg,
        buyer: DeskExecutionLeg,
        qty: u32,
        price_cents: i64,
        side: Option<Side>,
        before: DeskQuote,
    },
    DayEnded { order: OrderRef, owner: Owner, qty: u32 },
    MarketRemainder { order: OrderRef, owner: Owner, qty: u32 },
    Quote(DeskQuote),
    /// 中间 slice 尚未接通的真实路径；不能生成看似完整的报告。
    Unsupported { detail: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskCausalFact {
    pub context: super::retail_lifecycle::BatchContext,
    pub kind: DeskCausalKind,
}

/// 对照同一原始 DeskReport，局部 ordinal 同时证明覆盖与真实单股次序。
pub(super) fn validate_executions(
    stock: StockIdx,
    causal: &[DeskCausalFact],
    trades: &[super::protocol::DeskTradeFact],
    auction: bool,
) -> Result<(), super::protocol::RuntimeError> {
    let fail = |detail| super::protocol::RuntimeError::ProtocolInvariant { detail };
    let holes = causal.iter().any(|fact| matches!(fact.kind, DeskCausalKind::Unsupported { .. }));
    let mut expected = 0_u64;
    for fact in causal {
        if fact.context.stock() != stock { return Err(fail(format!("causal 原报告股身份错配：stock={stock} fact={fact:?}"))); }
        if let DeskCausalKind::Execution { ordinal, seller, buyer, qty, price_cents, side, .. } = fact.kind {
            if auction != side.is_none() { return Err(fail(format!("causal Execution 主动方向与原报告 auction 错配：stock={stock}"))); }
            if ordinal < expected || (!holes && ordinal != expected) {
                return Err(fail(format!("causal Execution ordinal 非真实 Encounter order：stock={stock} expected={expected} actual={ordinal}")));
            }
            let valid_order = |order: OrderRef| order.0 == stock && order.1 <= u64::from(u32::MAX);
            if !valid_order(seller.order) || !valid_order(buyer.order) {
                return Err(fail(format!("causal Execution OrderRef 超出报告身份域：stock={stock}")));
            }
            let trade = usize::try_from(ordinal).ok().and_then(|index| trades.get(index))
                .ok_or_else(|| fail(format!("causal Execution ordinal 无真实 Trade：stock={stock} ordinal={ordinal}")))?;
            if trade.stock != stock || trade.maker != seller.owner || trade.taker != buyer.owner
                || trade.maker_order_serial != super::group::desk_order_id(seller.order.0, seller.order.1)
                || trade.taker_order_serial != super::group::desk_order_id(buyer.order.0, buyer.order.1)
                || trade.qty != qty || trade.price != price_cents || match side { Some(side) => trade.taker_side != side, None => trade.taker_side != Side::Buy } {
                return Err(fail(format!("causal Execution 与真实 Trade 错配：stock={stock} ordinal={ordinal}")));
            }
            expected = ordinal.checked_add(1)
                .ok_or_else(|| fail("causal Execution ordinal 溢出".to_owned()))?;
        }
    }
    if !holes && usize::try_from(expected).ok() != Some(trades.len()) {
        return Err(fail(format!("causal Execution 缺失真实 Trade：stock={stock} count={expected} trades={}", trades.len())));
    }
    Ok(())
}

/// 日终余单必须与同一原始报告的真实终止逐项对应，不能漏记或把主动撤单混入。
pub(super) fn validate_day_ends(
    stock: StockIdx,
    causal: &[DeskCausalFact],
    lifecycle: &[super::protocol::DeskLifecycleFact],
) -> Result<(), super::protocol::RuntimeError> {
    let mut actual = causal.iter().filter_map(|fact| match fact.kind {
        DeskCausalKind::DayEnded { order, owner, qty } => Some((order, owner, qty)), _ => None,
    }).collect::<Vec<_>>();
    let mut expected = lifecycle.iter().filter(|fact| matches!(fact.kind, super::protocol::LifecycleKind::Cancelled))
        .map(|fact| ((fact.stock, fact.order_serial & u64::from(u32::MAX)), fact.owner, fact.remaining)).collect::<Vec<_>>();
    actual.sort_unstable(); expected.sort_unstable();
    if lifecycle.iter().any(|fact| fact.stock != stock || fact.order_serial >> 32 != u64::from(stock))
        || actual != expected || actual.iter().any(|(order, _, qty)| order.0 != stock || *qty == 0)
        || actual.windows(2).any(|pair| pair[0].0 == pair[1].0) {
        return Err(super::protocol::RuntimeError::ProtocolInvariant {
            detail: format!("Drain DayEnded 与原报告真实余单终止错配：stock={stock} actual={actual:?} expected={expected:?}"),
        });
    }
    Ok(())
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;
    use crate::runtime::group::{CancelSpec, GroupScript, PlaceSpec, TickPlan};
    use crate::runtime::skeleton_tests::basic_spec;
    use crate::runtime::world::World;

    fn world() -> World { world_with_ttl(1_000_000) }

    fn world_with_ttl(ttl: u64) -> World {
        let script = Box::new(|_group| {
            Box::new(|tick: u64, _round: u32| {
                let mut plan = TickPlan::default();
                if tick == 1 {
                    plan.places.push(PlaceSpec { local: 1, stock: 0, side: Side::Buy, price: 995, qty: 100 });
                } else if tick == 2 {
                    plan.cancels.push(CancelSpec { local: 1, stock: 0 });
                }
                plan
            }) as GroupScript
        });
        let mut spec = basic_spec(1, 1, 4, script);
        spec.order_ttl_ticks = ttl;
        spec.ticks_per_day = 30;
        spec.auction_ticks = 0;
        World::spawn(
            spec,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap()
    }

    #[test]
    fn d1_non_player_place_and_cancel_all_keep_unresolved_real_sources() {
        let mut world = world();
        world.run_ticks(2).unwrap();
        assert_eq!(world.report_log[0].desk_causal.len(), 2);
        assert!(matches!(world.report_log[0].desk_causal[0].kind,
            DeskCausalKind::Submitted { qty: 100, source: OperationSource::NonPlayerUnresolved, .. }));
        assert!(matches!(world.report_log[1].desk_causal[0].kind,
            DeskCausalKind::Cancelled { qty: 100, source: OperationSource::NonPlayerUnresolved, .. }));
        assert!(!world.report_log[1].desk_causal.iter().any(|fact| matches!(fact.kind, DeskCausalKind::DayEnded { .. })), "普通 CancelAll 不能变成日终失效");
        world.shutdown().unwrap();
    }

    #[test]
    fn d1_desk_abort_drops_unpublished_sideband_before_same_tick_retry() {
        let mut world = world();
        world.set_validator(Box::new(|_| Err("D1 Abort".to_owned())));
        assert!(world.run_ticks(1).is_err());
        assert!(world.report_log.is_empty());
        world.set_validator(crate::runtime::coordinator::no_op_validator());
        world.run_ticks(1).unwrap();
        assert_eq!(world.report_log.len(), 1);
        assert_eq!(world.report_log[0].desk_causal.len(), 2);
        assert_eq!(world.report_log[0].desk_causal.iter().filter(|fact| matches!(fact.kind, DeskCausalKind::Submitted { .. })).count(), 1);
        world.shutdown().unwrap();
    }
    #[test]
    fn d1_actual_expiry_keeps_an_explicit_unsupported_marker() {
        let mut world = world_with_ttl(1);
        world.run_ticks(2).unwrap();
        assert!(world.report_log[1].desk_causal.iter().any(|fact| matches!(fact.kind, DeskCausalKind::Unsupported { detail } if detail.contains("Expired"))), "真实到期不能静默丢掉诊断生命周期");
        world.shutdown().unwrap();
    }

    fn continuous_reports() -> Vec<super::super::world::TickRecord> {
        let script = Box::new(|_group| Box::new(|tick: u64, _round: u32| {
            let mut plan = TickPlan::default();
            if tick == 1 {
                // 两段完整操作互不共用订单；第二股重复本地 serial。
                for stock in [0, 1] {
                    for (local, side, price) in [(0, Side::Sell, 990), (1, Side::Buy, 990), (2, Side::Buy, 980), (3, Side::Sell, 980)] {
                        plan.places.push(PlaceSpec { local, stock, side, price, qty: 100 });
                    }
                }
            }
            plan
        }) as GroupScript);
        let mut spec = basic_spec(1, 2, 4, script);
        spec.closing_auction_ticks = 0;
        let mut world = World::spawn(
            spec,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        world.run_ticks(1).unwrap();
        let records = world.report_log.clone();
        world.shutdown().unwrap();
        records
    }

    #[test]
    fn d2_real_report_matches_full_encoded_identity_both_directions_and_stock_reset() {
        let records = continuous_reports();
        for stock in [0, 1] {
            let causal = records[0].desk_causal.iter().copied().filter(|f| f.context.stock() == stock).collect::<Vec<_>>();
            let trades = records[0].desk_trades.iter().copied().filter(|f| f.stock == stock).collect::<Vec<_>>();
            validate_executions(stock, &causal, &trades, false).unwrap();
            let executions = causal.iter().filter_map(|f| match f.kind {
                DeskCausalKind::Execution { ordinal, seller, buyer, side, .. } => Some((ordinal, seller, buyer, side)), _ => None,
            }).collect::<Vec<_>>();
            assert_eq!(executions.len(), 2);
            assert_eq!((executions[0].0, executions[1].0), (0, 1));
            assert_eq!((executions[0].3, executions[1].3), (Some(Side::Buy), Some(Side::Sell)));
            assert_eq!(trades[0].maker_order_serial, (u64::from(stock) << 32) | executions[0].1.order.1);
            if stock == 1 { assert!(trades[0].maker_order_serial > u64::from(u32::MAX)); }
        }
    }

    #[test]
    fn d2_original_report_rejects_missing_duplicate_reversed_and_tuple_mismatch() {
        let records = continuous_reports();
        let causal = records[0].desk_causal.iter().copied().filter(|f| f.context.stock() == 1).collect::<Vec<_>>();
        let trades = records[0].desk_trades.iter().copied().filter(|f| f.stock == 1).collect::<Vec<_>>();
        let execution = causal.iter().position(|f| matches!(f.kind, DeskCausalKind::Execution { .. })).unwrap();
        let mut missing = causal.clone(); missing.remove(execution);
        assert!(validate_executions(1, &missing, &trades, false).is_err());
        let mut duplicate = causal.clone(); duplicate.insert(execution, causal[execution]);
        assert!(validate_executions(1, &duplicate, &trades, false).is_err());
        // 每段两张新单各保有 pre/post Quote，交换整个独立成交片段，仅破坏 ordinal 次序。
        let second = causal.iter().enumerate().filter(|(_, f)| matches!(f.kind, DeskCausalKind::Submitted { .. })).nth(2).unwrap().0;
        let mut reversed = causal[second..].to_vec(); reversed.extend_from_slice(&causal[..second]);
        let error = validate_executions(1, &reversed, &trades, false).unwrap_err();
        assert!(format!("{error:?}").contains("Encounter order"));
        for mutation in 0..8 {
            let mut bad = causal.clone();
            if let DeskCausalKind::Execution { ordinal, seller, buyer, qty, price_cents, side, .. } = &mut bad[execution].kind {
                match mutation {
                    0 => seller.order.0 = 0,
                    1 => seller.order.1 += 1,
                    2 => buyer.owner.1 += 1,
                    3 => *qty += 1,
                    4 => *price_cents += 1,
                    5 => *side = Some(Side::Sell),
                    6 => *ordinal = 10,
                    _ => seller.order.1 = u64::from(u32::MAX) + 1,
                }
            }
            assert!(validate_executions(1, &bad, &trades, false).is_err(), "必须拒绝tuple变体{mutation}");
        }
    }

    #[test]
    fn d2_real_world_continue_resets_report_ordinal_and_keeps_before_chain() {
        let script = Box::new(|_group| {
            Box::new(|tick: u64, round: u32| {
                let mut plan = TickPlan::default();
                if tick == 1 && round == 0 {
                    plan.places.push(PlaceSpec {
                        local: 0,
                        stock: 1,
                        side: Side::Sell,
                        price: 995,
                        qty: 100,
                    });
                    plan.places.push(PlaceSpec {
                        local: 1,
                        stock: 1,
                        side: Side::Buy,
                        price: 1000,
                        qty: 200,
                    });
                    plan.continuation = true;
                } else if tick == 1 && round == 1 {
                    plan.places.push(PlaceSpec {
                        local: 2,
                        stock: 1,
                        side: Side::Sell,
                        price: 1000,
                        qty: 100,
                    });
                }
                plan
            }) as GroupScript
        });
        let mut spec = basic_spec(1, 2, 3, script);
        spec.closing_auction_ticks = 0;
        let mut world = World::spawn(
            spec,
            crate::calendar::TradingCalendar::current_default_calendar()
                .expect("显式 raw Calendar 政策合法"),
        )
        .unwrap();
        world.run_ticks(1).unwrap();
        let record = &world.report_log[0];
        assert_eq!(record.rounds, 1);
        let executions = record
            .desk_causal
            .iter()
            .filter_map(|fact| match fact.kind {
                DeskCausalKind::Execution { ordinal, buyer, .. } => {
                    Some((match fact.context { super::super::retail_lifecycle::BatchContext::Tick { round, .. } => round, _ => panic!("真实 Tick 报告不得含 Drain") }, ordinal, buyer.order, buyer.value_before))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(executions.len(), 2);
        assert_eq!(
            (executions[0].0, executions[0].1, executions[0].3),
            (0, 0, 0)
        );
        assert_eq!(
            (executions[1].0, executions[1].1, executions[1].3),
            (1, 0, 99_500)
        );
        assert_eq!(executions[0].2, executions[1].2);
        world.shutdown().unwrap();
    }

}

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "diagnostics_report_fault_tests.rs"]
pub(crate) mod report_fault_tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "diagnostics_drain_tests.rs"]
mod drain_tests;

//! 只读 causal sideband：保留单股实际操作顺序，不参与撮合或资源转移。

use super::{OrderRef, Owner, StockIdx};
use crate::Side;

/// D1 只解析玩家外部操作；其他来源由后续真实计划归因接通。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperationSource {
    PlayerExternal,
    NonPlayerUnresolved,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskQuote {
    pub bid_cents: Option<i64>,
    pub ask_cents: Option<i64>,
    pub bid_depth: u64,
    pub ask_depth: u64,
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
    Quote(DeskQuote),
    /// 中间 slice 尚未接通的真实路径；不能生成看似完整的报告。
    Unsupported { detail: &'static str },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeskCausalFact {
    pub tick: u64,
    pub round: u32,
    pub stock: StockIdx,
    pub kind: DeskCausalKind,
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
        World::spawn(spec).unwrap()
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

}

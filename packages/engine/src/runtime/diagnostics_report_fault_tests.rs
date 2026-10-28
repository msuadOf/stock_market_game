//! 只删除协调线程收到的真实 Execution；不伪造 report、trade 或领域错误。

use super::{DeskCausalKind, DeskExecutionLeg};
use crate::runtime::protocol::DeskReport;
use std::cell::RefCell;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RemovalEvidence {
    pub tick: u64,
    pub round: u32,
    pub stock: u16,
    pub seller: DeskExecutionLeg,
    pub buyer: DeskExecutionLeg,
    pub qty: u32,
    pub price_cents: i64,
    pub original_trades: usize,
    pub executions_before: usize,
    pub executions_after: usize,
}

#[derive(Default)]
struct HookState {
    armed_stock: Option<u16>,
    evidence: Option<RemovalEvidence>,
    drain: bool,
    drain_evidence: Option<(super::DeskCausalFact, usize)>,
}

thread_local! {
    static HOOK: RefCell<HookState> = RefCell::new(HookState::default());
}

pub(crate) struct HookArm;

impl HookArm {
    pub(crate) fn new(stock: u16) -> Self {
        HOOK.with(|state| {
            let mut state = state.borrow_mut();
            assert!(state.armed_stock.is_none(), "report hook 禁止嵌套 arm");
            state.armed_stock = Some(stock);
            state.evidence = None;
            state.drain = false;
            state.drain_evidence = None;
        });
        Self
    }

    pub(crate) fn new_drain(stock: u16) -> Self {
        let arm = Self::new(stock);
        HOOK.with(|state| state.borrow_mut().drain = true);
        arm
    }

    pub(crate) fn drain_evidence(&self) -> (super::DeskCausalFact, usize) {
        HOOK.with(|state| state.borrow().drain_evidence.expect("Drain hook 必须命中真实成交"))
    }

    pub(crate) fn evidence(&self) -> RemovalEvidence {
        HOOK.with(|state| state.borrow().evidence.clone().expect("report hook 必须命中真实成交"))
    }
}

impl Drop for HookArm {
    fn drop(&mut self) {
        HOOK.with(|state| state.borrow_mut().armed_stock = None);
    }
}

pub(crate) fn remove_execution_once(mut report: DeskReport) -> DeskReport {
    HOOK.with(|state| {
        let mut state = state.borrow_mut();
        if state.drain || state.armed_stock != Some(report.stock) {
            return;
        }
        let Some(index) = report.causal.iter().position(|fact| matches!(fact.kind, DeskCausalKind::Execution { .. })) else {
            return;
        };
        let executions_before = report.causal.iter().filter(|fact| matches!(fact.kind, DeskCausalKind::Execution { .. })).count();
        let original_trades = report.trades.clone();
        let removed = report.causal.remove(index);
        let DeskCausalKind::Execution { seller, buyer, qty, price_cents, .. } = removed.kind else {
            unreachable!("position 已确认真实 Execution");
        };
        let executions_after = report.causal.iter().filter(|fact| matches!(fact.kind, DeskCausalKind::Execution { .. })).count();
        assert_eq!(report.trades, original_trades, "不得改变真实成交");
        assert_eq!(executions_before, executions_after + 1);
        state.evidence = Some(RemovalEvidence {
            tick: report.tick, round: report.round, stock: report.stock,
            seller, buyer, qty, price_cents,
            original_trades: original_trades.len(), executions_before, executions_after,
        });
        state.armed_stock = None;
    });
    report
}

/// 与普通 report hook 共用同一测试线程入口，只删除真实 Drain 报告的 Execution。
pub(crate) fn remove_drain_execution_once(stock: u16, causal: &mut Vec<super::DeskCausalFact>, trades: &[crate::runtime::protocol::DeskTradeFact]) {
    HOOK.with(|state| {
        let mut state = state.borrow_mut();
        if !state.drain || state.armed_stock != Some(stock) { return; }
        let Some(index) = causal.iter().position(|fact| matches!(fact.kind, DeskCausalKind::Execution { .. })) else { return; };
        let removed = causal.remove(index);
        state.drain_evidence = Some((removed, trades.len()));
        state.armed_stock = None;
    });
}

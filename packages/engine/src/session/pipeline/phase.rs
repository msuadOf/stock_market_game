/// TickPhase 职责词汇；rank 不依赖声明顺序。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickPhase {
    ExpiryShadow,
    SealAllocationSnapshot,
    DecisionAndContinuation,
    AccountValidation,
    StockProcessing,
    ReceiptAggregation,
    SettlementShadow,
    DerivationAudit,
    PreCommitValidation,
    CommitTick,
}

impl TickPhase {
    pub const ALL: [Self; 10] = [
        Self::ExpiryShadow,
        Self::SealAllocationSnapshot,
        Self::DecisionAndContinuation,
        Self::AccountValidation,
        Self::StockProcessing,
        Self::ReceiptAggregation,
        Self::SettlementShadow,
        Self::DerivationAudit,
        Self::PreCommitValidation,
        Self::CommitTick,
    ];

    pub const fn rank(self) -> u8 {
        match self {
            Self::ExpiryShadow => 0,
            Self::SealAllocationSnapshot => 1,
            Self::DecisionAndContinuation => 2,
            Self::AccountValidation => 3,
            Self::StockProcessing => 4,
            Self::ReceiptAggregation => 5,
            Self::SettlementShadow => 6,
            Self::DerivationAudit => 7,
            Self::PreCommitValidation => 8,
            Self::CommitTick => 9,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::ExpiryShadow => "expiry_shadow",
            Self::SealAllocationSnapshot => "seal_allocation_snapshot",
            Self::DecisionAndContinuation => "decision_and_continuation",
            Self::AccountValidation => "account_validation",
            Self::StockProcessing => "stock_processing",
            Self::ReceiptAggregation => "receipt_aggregation",
            Self::SettlementShadow => "settlement_shadow",
            Self::DerivationAudit => "derivation_audit",
            Self::PreCommitValidation => "pre_commit_validation",
            Self::CommitTick => "commit_tick",
        }
    }
}

/// 当前 tick 的交易制度截点；不是运行时可配置 feature flag。
/// 开盘窗口的不可撤边界由原始 elapsed/cancelable ticks 决定。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::session) struct PhasePolicy {
    phase: crate::TradingPhase,
    elapsed_ticks: u64,
    cancelable_ticks: u64,
}

impl PhasePolicy {
    pub(super) const fn new(
        phase: crate::TradingPhase,
        elapsed_ticks: u64,
        cancelable_ticks: u64,
    ) -> Self {
        Self {
            phase,
            elapsed_ticks,
            cancelable_ticks,
        }
    }

    pub(in crate::session) fn capture(session: &crate::GameSession) -> Self {
        Self::new(
            session.phase(),
            session.tick() % session.state.setup.ticks_per_day,
            session.state.setup.auction_ticks / 3,
        )
    }

    pub(in crate::session) fn capture_on(view: &super::npc_split::SessionReadView<'_>) -> Self {
        Self::new(
            view.phase(),
            view.tick % view.setup.ticks_per_day,
            view.setup.auction_ticks / 3,
        )
    }

    pub(super) const fn phase(self) -> crate::TradingPhase {
        self.phase
    }

    pub(super) const fn allows_cancel(self) -> bool {
        match self.phase {
            crate::TradingPhase::CallAuction => self.elapsed_ticks < self.cancelable_ticks,
            crate::TradingPhase::Continuous => true,
            crate::TradingPhase::PreOpen | crate::TradingPhase::ClosingAuction => false,
        }
    }

    pub(super) fn place_rejection(self, kind: super::PlaceKind) -> Option<crate::RejectionReason> {
        match self.phase {
            crate::TradingPhase::PreOpen => Some(crate::RejectionReason::AuctionOrderEntryClosed),
            crate::TradingPhase::CallAuction | crate::TradingPhase::ClosingAuction
                if kind == super::PlaceKind::Market =>
            {
                Some(crate::RejectionReason::AuctionLimitOrderRequired)
            }
            crate::TradingPhase::CallAuction
            | crate::TradingPhase::ClosingAuction
            | crate::TradingPhase::Continuous => None,
        }
    }

    pub(super) const fn applies_price_cage(self, enabled: bool) -> bool {
        enabled && matches!(self.phase, crate::TradingPhase::Continuous)
    }

    pub(in crate::session) const fn matching_mode(self) -> crate::orderbook::MatchingMode {
        match self.phase {
            crate::TradingPhase::CallAuction | crate::TradingPhase::ClosingAuction => {
                crate::orderbook::MatchingMode::Auction
            }
            crate::TradingPhase::PreOpen | crate::TradingPhase::Continuous => {
                crate::orderbook::MatchingMode::Continuous
            }
        }
    }
}

/// 唯一真实 book 的只读公开投影；不持有订单或可变权威。
pub(in crate::session) struct PublicBookProjection {
    pub(in crate::session) best_bid: Option<crate::Money>,
    pub(in crate::session) best_ask: Option<crate::Money>,
    pub(in crate::session) bids: Vec<(crate::Money, u64)>,
    pub(in crate::session) asks: Vec<(crate::Money, u64)>,
}

impl PhasePolicy {
    pub(in crate::session) fn project_public_book(self, market: &crate::Market) -> PublicBookProjection {
        let (bids, asks) = match self.phase {
            crate::TradingPhase::CallAuction => (Vec::new(), Vec::new()),
            crate::TradingPhase::ClosingAuction => market.depth_before_sequence(market.auction_public_cutoff_sequence()),
            crate::TradingPhase::PreOpen | crate::TradingPhase::Continuous => (market.bid_depth(), market.ask_depth()),
        };
        PublicBookProjection { best_bid: bids.first().map(|(price, _)| *price),
            best_ask: asks.first().map(|(price, _)| *price), bids, asks }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct MarketTickBoundary {
    pub(super) policy: PhasePolicy,
    pub(super) tick_after: u64,
    pub(super) finishes_auction: bool,
    pub(super) ends_day: bool,
    pub(super) completed_minutes: Option<u16>,
}

impl PhasePolicy {
    pub(super) const fn is_auction(self) -> bool {
        matches!(
            self.phase,
            crate::TradingPhase::CallAuction | crate::TradingPhase::ClosingAuction
        )
    }

    pub(super) fn boundary(
        self,
        session: &crate::GameSession,
    ) -> Result<MarketTickBoundary, crate::session::StepFatal> {
        let invariant = |description: &str| crate::session::StepFatal::InvariantViolation {
            description: description.to_owned(),
            location: "pipeline::PhasePolicy::boundary".to_owned(),
        };
        if self != Self::capture(session) {
            return Err(invariant("PhasePolicy 与 candidate 阶段截点不符"));
        }
        let setup = &session.state.setup;
        let tick_after = session
            .tick()
            .checked_add(1)
            .ok_or_else(|| invariant("市场 tick 溢出"))?;
        let ends_day = tick_after.is_multiple_of(setup.ticks_per_day);
        if ends_day && session.day() == u32::MAX {
            return Err(invariant("交易日序号溢出"));
        }
        let finishes_auction = match self.phase {
            crate::TradingPhase::CallAuction => {
                tick_after % setup.ticks_per_day == setup.auction_ticks - setup.auction_ticks / 3
            }
            crate::TradingPhase::ClosingAuction => ends_day,
            crate::TradingPhase::PreOpen | crate::TradingPhase::Continuous => false,
        };
        if ends_day && self.phase == crate::TradingPhase::PreOpen {
            return Err(invariant("PreOpen 不应跨越日界"));
        }
        let completed_minutes = if self.phase == crate::TradingPhase::Continuous {
            let continuous_ticks = setup
                .ticks_per_day
                .checked_sub(setup.auction_ticks)
                .and_then(|ticks| ticks.checked_sub(setup.closing_auction_ticks))
                .ok_or_else(|| invariant("连续竞价时段配置非法"))?;
            let elapsed = self
                .elapsed_ticks
                .checked_sub(setup.auction_ticks)
                .and_then(|ticks| ticks.checked_add(1))
                .ok_or_else(|| invariant("连续竞价时钟非法"))?;
            Some(
                crate::session::completed_market_minute_count(elapsed, continuous_ticks)
                    .map_err(|error| invariant(&format!("连续竞价分钟映射失败：{error}")))?,
            )
        } else {
            None
        };
        Ok(MarketTickBoundary {
            policy: self,
            tick_after,
            finishes_auction,
            ends_day,
            completed_minutes,
        })
    }
}

#[cfg(test)]
mod policy_tests {
    use super::*;
    use crate::session::pipeline::PlaceKind;
    use crate::{RejectionReason, TradingPhase};

    #[test]
    fn phase_policy_preserves_entry_cancel_and_price_cage_boundaries() {
        for (phase, elapsed, cancel_until, place, cancel, cage) in [
            (TradingPhase::CallAuction, 4, 5, None, true, false),
            (TradingPhase::CallAuction, 5, 5, None, false, false),
            (
                TradingPhase::PreOpen,
                10,
                5,
                Some(RejectionReason::AuctionOrderEntryClosed),
                false,
                false,
            ),
            (TradingPhase::Continuous, 15, 5, None, true, true),
            (TradingPhase::ClosingAuction, 25, 5, None, false, false),
        ] {
            let policy = PhasePolicy::new(phase, elapsed, cancel_until);
            assert_eq!(policy.place_rejection(PlaceKind::Limit), place);
            assert_eq!(policy.allows_cancel(), cancel);
            assert_eq!(policy.applies_price_cage(true), cage);
            assert!(!policy.applies_price_cage(false));
            assert_eq!(
                policy.place_rejection(PlaceKind::Market),
                match phase {
                    TradingPhase::CallAuction | TradingPhase::ClosingAuction =>
                        Some(RejectionReason::AuctionLimitOrderRequired),
                    TradingPhase::PreOpen => Some(RejectionReason::AuctionOrderEntryClosed),
                    TradingPhase::Continuous => None,
                }
            );
        }
    }
}

use std::collections::BTreeSet;

use crate::config::A_SHARE_BOARD_LOT;
use crate::experience::PersonalWatchlist;
use crate::{Money, StockCode};

use super::signals::div_round_half_even;
use super::types::{CandidateError, SignalScore};

/// 用个体调整步幅响应本次信号；步幅不限制最终仓位，目标可达到全现金或全仓。
pub fn target_position_weight_bp(
    current_weight_bp: u32,
    score: SignalScore,
    position_step_bp: u32,
) -> Result<u32, CandidateError> {
    for value in [current_weight_bp, position_step_bp] {
        if value > 10_000 {
            return Err(CandidateError::InvalidPositionFraction { value });
        }
    }
    let delta = div_round_half_even(
        i128::from(score.value()) * i128::from(position_step_bp),
        10_000,
    );
    let target = (i128::from(current_weight_bp) + delta).clamp(0, 10_000);
    u32::try_from(target).map_err(|_| CandidateError::ArithmeticOverflow {
        step: "target weight",
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuantityRounding {
    Exact,
    BoardLotDown { raw_qty: u32 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TargetShareQuantity {
    pub target_qty: u32,
    pub rounding: QuantityRounding,
}

pub fn target_share_quantity(
    target_weight_bp: u32,
    equity: Money,
    price: Money,
    lot_size: u32,
) -> Result<TargetShareQuantity, CandidateError> {
    if target_weight_bp > 10_000 {
        return Err(CandidateError::InvalidPositionFraction {
            value: target_weight_bp,
        });
    }
    require_positive("account equity", equity)?;
    require_positive("stock price", price)?;
    if lot_size != A_SHARE_BOARD_LOT {
        return Err(CandidateError::InvalidBoardLotSize { lot_size });
    }
    let target_value = div_round_half_even(
        i128::from(equity.cents()) * i128::from(target_weight_bp),
        10_000,
    );
    let raw_qty = target_value / i128::from(price.cents());
    let raw_qty = u32::try_from(raw_qty).map_err(|_| CandidateError::ArithmeticOverflow {
        step: "target share quantity",
    })?;
    let target_qty = raw_qty - raw_qty % lot_size;
    let rounding = if target_qty == raw_qty {
        QuantityRounding::Exact
    } else {
        QuantityRounding::BoardLotDown { raw_qty }
    };
    Ok(TargetShareQuantity {
        target_qty,
        rounding,
    })
}

/// 单次候选的仓位意向；股本约束先收缩权重，随后按 A 股整手换算。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidateTargetProposal {
    target_weight_bp: u32,
    target_qty: u32,
    rounding: QuantityRounding,
}

impl CandidateTargetProposal {
    #[allow(clippy::too_many_arguments)]
    pub fn from_score(
        current_weight_bp: u32,
        score: SignalScore,
        position_step_bp: u32,
        equity: Money,
        price: Money,
        max_holdable_shares: u64,
        lot_size: u32,
    ) -> Result<Self, CandidateError> {
        let weight = target_position_weight_bp(current_weight_bp, score, position_step_bp)?;
        require_positive("account equity", equity)?;
        require_positive("stock price", price)?;
        let max_holdable_shares = max_holdable_shares.min(u64::from(u32::MAX));
        let max_holdable_bp = u32::try_from(
            ((i128::from(max_holdable_shares) * i128::from(price.cents())) * 10_000
                / i128::from(equity.cents()))
            .clamp(0, 10_000),
        )
        .expect("clamped weight fits u32");
        let target_weight_bp = weight.min(max_holdable_bp);
        let quantity = target_share_quantity(target_weight_bp, equity, price, lot_size)?;
        Ok(Self {
            target_weight_bp,
            target_qty: quantity.target_qty,
            rounding: quantity.rounding,
        })
    }

    pub const fn target_weight_bp(self) -> u32 {
        self.target_weight_bp
    }
    pub const fn target_qty(self) -> u32 {
        self.target_qty
    }
    pub const fn rounding(self) -> QuantityRounding {
        self.rounding
    }
}

pub fn eligible_candidates(
    held: &BTreeSet<StockCode>,
    watchlist: &PersonalWatchlist,
    discovered_this_round: &BTreeSet<StockCode>,
) -> BTreeSet<StockCode> {
    held.iter()
        .chain(watchlist.stocks.keys())
        .chain(discovered_this_round)
        .cloned()
        .collect()
}

fn require_positive(field: &'static str, money: Money) -> Result<(), CandidateError> {
    if money.cents() <= 0 {
        return Err(CandidateError::NonPositiveMoney {
            field,
            cents: money.cents(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod proposal_tests {
    use super::*;

    #[test]
    fn proposal_caps_non_board_lot_share_limit_before_rounding() {
        let proposal = CandidateTargetProposal::from_score(
            0,
            SignalScore::new(10_000).unwrap(),
            10_000,
            Money::from_cents(100_000),
            Money::from_cents(100),
            255,
            100,
        )
        .unwrap();
        assert_eq!(proposal.target_weight_bp(), 2550);
        assert_eq!(proposal.target_qty(), 200);
        assert_eq!(
            proposal.rounding(),
            QuantityRounding::BoardLotDown { raw_qty: 255 }
        );
    }

    #[test]
    fn proposal_preserves_cash_and_exact_board_lot_targets() {
        for (score, expected) in [(-10_000, 0), (10_000, 500)] {
            let proposal = CandidateTargetProposal::from_score(
                5000,
                SignalScore::new(score).unwrap(),
                5000,
                Money::from_cents(50_000),
                Money::from_cents(100),
                1000,
                100,
            )
            .unwrap();
            assert_eq!(proposal.target_qty(), expected);
            assert_eq!(proposal.rounding(), QuantityRounding::Exact);
        }
    }

    #[test]
    fn proposal_rejects_invalid_weight_before_invalid_quantity_inputs() {
        assert_eq!(
            CandidateTargetProposal::from_score(
                10_001,
                SignalScore::new(0).unwrap(),
                1,
                Money::ZERO,
                Money::ZERO,
                0,
                1,
            ),
            Err(CandidateError::InvalidPositionFraction { value: 10_001 })
        );
    }
}

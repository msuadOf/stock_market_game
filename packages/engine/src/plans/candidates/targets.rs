use std::collections::BTreeSet;

use crate::config::A_SHARE_BOARD_LOT;
use crate::experience::PersonalWatchlist;
use crate::{Money, StockCode};

use super::signals::div_round_half_even;
use super::types::{CandidateError, SignalScore};

pub fn target_position_weight_bp(
    current_weight_bp: u32,
    score: SignalScore,
    max_stock_fraction_bp: u32,
) -> Result<u32, CandidateError> {
    for value in [current_weight_bp, max_stock_fraction_bp] {
        if value > 10_000 {
            return Err(CandidateError::InvalidPositionFraction { value });
        }
    }
    let delta = div_round_half_even(
        i128::from(score.value()) * i128::from(max_stock_fraction_bp),
        40_000,
    );
    let target =
        (i128::from(current_weight_bp) + delta).clamp(0, i128::from(max_stock_fraction_bp));
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

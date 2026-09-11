use crate::experience::{ExperienceError, ExperienceMoment, RetailExperienceState};
use crate::{Money, StockCode};

use super::div_round_half_even;
use super::types::AllocationExperience;

pub struct ExperienceHolding<'a> {
    pub code: &'a StockCode,
    pub cost: Money,
}

pub struct ExperienceReadRequest<'a> {
    pub experience: &'a RetailExperienceState,
    pub as_of: &'a ExperienceMoment,
    pub equity: Money,
    pub holding: Option<ExperienceHolding<'a>>,
}

pub fn read_allocation_experience(
    request: &ExperienceReadRequest<'_>,
) -> Result<AllocationExperience, ExperienceError> {
    let failure_influence = request.experience.failure_influence(request.as_of)?;
    let long_stuck = request
        .holding
        .as_ref()
        .map(|holding| {
            request
                .experience
                .is_long_stuck(holding.code, holding.cost, request.as_of)
        })
        .transpose()?
        .unwrap_or(false);
    let drawdown_bp = match request
        .experience
        .experience_drawdown_from_peak(request.equity)
    {
        Some(_) => request.experience.peak_equity.map(|peak| {
            let decline = i128::from(peak.cents()) - i128::from(request.equity.cents());
            let rounded = div_round_half_even(decline * 10_000, i128::from(peak.cents()));
            // SAFE-EXPECT: the value is clamped to the u32 subset 0..=10_000.
            u32::try_from(rounded.clamp(0, 10_000)).expect("clamped drawdown fits u32")
        }),
        None => None,
    };
    Ok(AllocationExperience {
        failure_influence,
        long_stuck,
        drawdown_bp,
    })
}

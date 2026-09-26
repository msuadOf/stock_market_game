use crate::experience::{ExperienceError, ExperienceMoment, RetailExperienceState};
use crate::{Money, StockCode};

use super::types::AllocationExperience;

pub struct ExperienceHolding<'a> {
    pub code: &'a StockCode,
    pub cost: Money,
}

pub struct ExperienceReadRequest<'a> {
    pub experience: &'a RetailExperienceState,
    pub as_of: &'a ExperienceMoment,
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
    Ok(AllocationExperience {
        failure_influence,
        long_stuck,
    })
}

use super::SimpleEnvironmentConfig;
use super::period::SettlementCycle;
use crate::{
    accounting::AccountingAmount,
    company::{CompanySystemError, rng::OperatingRng},
};

pub(super) fn advance_environment(
    previous: i32,
    config: &SimpleEnvironmentConfig,
    rng: &mut OperatingRng,
    cycle: SettlementCycle,
) -> Result<i32, CompanySystemError> {
    let persisted = AccountingAmount::from_cents(i128::from(previous))
        .apply_basis_points(config.persistence_bp)?
        .cents();
    let bound = config.noise.bound(cycle);
    let shock = rng.range_i64(-i64::from(bound), i64::from(bound));
    i32::try_from(persisted + i128::from(shock))
        .map_err(|_| CompanySystemError::Invalid("Simple 环境变化超过 i32 基点范围".into()))
}

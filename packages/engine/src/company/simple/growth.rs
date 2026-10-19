use crate::{accounting::AccountingAmount, company::CompanySystemError};
use num_bigint::BigUint;

pub const GROWTH_FACTOR_SCALE: u64 = 1_000_000_000;

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct GrowthFactor(u64);

impl GrowthFactor {
    pub fn from_annual_basis_points(
        annual_growth_bp: i32,
        months: u8,
    ) -> Result<Self, CompanySystemError> {
        if annual_growth_bp < -10_000 || !(1..=12).contains(&months) {
            return Err(CompanySystemError::Invalid(
                "年化增长率不得低于 -100%，换算跨度必须为 1..12 自然月".into(),
            ));
        }
        let annual_scaled =
            (i64::from(annual_growth_bp) + 10_000) as u64 * (GROWTH_FACTOR_SCALE / 10_000);
        let exponent = u32::from(months);
        let target = BigUint::from(annual_scaled).pow(exponent)
            * BigUint::from(GROWTH_FACTOR_SCALE).pow(12 - exponent);
        let floor = target.nth_root(12);
        let midpoint = ((&floor << 1_u32) + BigUint::from(1_u8)).pow(12);
        let midpoint_target = target << 12_u32;
        let digits = floor.to_u64_digits();
        let mut value = match digits.as_slice() {
            [] => 0,
            [value] => *value,
            _ => return Err(CompanySystemError::Invalid("复利定点因子超出 u64".into())),
        };
        if midpoint < midpoint_target || (midpoint == midpoint_target && !value.is_multiple_of(2)) {
            value = value
                .checked_add(1)
                .ok_or_else(|| CompanySystemError::Invalid("复利定点因子进位溢出".into()))?;
        }
        Ok(Self(value))
    }
    pub fn nanos(self) -> u64 {
        self.0
    }
    pub(crate) fn with_basis_point_noise(self, noise: i32) -> Result<Self, CompanySystemError> {
        let factor =
            i128::from(self.0) + i128::from(noise) * i128::from(GROWTH_FACTOR_SCALE / 10_000);
        u64::try_from(factor)
            .map(Self)
            .map_err(|_| CompanySystemError::Invalid("期间扰动造成负增长因子或超出范围".into()))
    }
    pub fn compose(self, next: Self) -> Result<Self, CompanySystemError> {
        let product = u128::from(self.0) * u128::from(next.0);
        let rounded = round_quotient(product, u128::from(GROWTH_FACTOR_SCALE))?;
        u64::try_from(rounded)
            .map(Self)
            .map_err(|_| CompanySystemError::Invalid("复合定点因子超出 u64".into()))
    }
    pub fn apply(self, amount: AccountingAmount) -> Result<AccountingAmount, CompanySystemError> {
        let magnitude = amount.cents().unsigned_abs();
        let scale = u128::from(GROWTH_FACTOR_SCALE);
        let whole = (magnitude / scale)
            .checked_mul(u128::from(self.0))
            .ok_or_else(|| CompanySystemError::Invalid("复利金额超出范围".into()))?;
        let fractional_product = (magnitude % scale) * u128::from(self.0);
        let remainder = fractional_product % scale;
        let mut total = whole
            .checked_add(fractional_product / scale)
            .ok_or_else(|| CompanySystemError::Invalid("复利金额超出范围".into()))?;
        if remainder * 2 > scale || (remainder * 2 == scale && !total.is_multiple_of(2)) {
            total = total
                .checked_add(1)
                .ok_or_else(|| CompanySystemError::Invalid("复利金额进位溢出".into()))?;
        }
        let signed = if amount.is_negative() && total == (1_u128 << 127) {
            i128::MIN
        } else {
            let positive = i128::try_from(total).map_err(|_| {
                CompanySystemError::Invalid("复利金额超出 AccountingAmount 范围".into())
            })?;
            if amount.is_negative() {
                -positive
            } else {
                positive
            }
        };
        Ok(AccountingAmount::from_cents(signed))
    }
}

fn round_quotient(product: u128, denominator: u128) -> Result<u128, CompanySystemError> {
    let quotient = product / denominator;
    let remainder = product % denominator;
    if remainder * 2 > denominator || (remainder * 2 == denominator && !quotient.is_multiple_of(2))
    {
        quotient
            .checked_add(1)
            .ok_or_else(|| CompanySystemError::Invalid("定点比例进位溢出".into()))
    } else {
        Ok(quotient)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn annual_compound_uses_exact_period_roots_not_linear_division() {
        for (months, expected) in [
            (1, 1_009_488_793),
            (3, 1_028_737_345),
            (6, 1_058_300_524),
            (12, 1_120_000_000),
        ] {
            assert_eq!(
                GrowthFactor::from_annual_basis_points(1200, months)
                    .unwrap()
                    .nanos(),
                expected
            );
        }
    }
    #[test]
    fn negative_and_zero_annual_growth_have_explicit_nonnegative_factors() {
        assert_eq!(
            GrowthFactor::from_annual_basis_points(0, 1)
                .unwrap()
                .nanos(),
            GROWTH_FACTOR_SCALE
        );
        assert_eq!(
            GrowthFactor::from_annual_basis_points(-10_000, 3)
                .unwrap()
                .nanos(),
            0
        );
        assert!(GrowthFactor::from_annual_basis_points(-10_001, 1).is_err());
        assert!(GrowthFactor::from_annual_basis_points(1200, 0).is_err());
        assert!(GrowthFactor::from_annual_basis_points(1200, 13).is_err());
    }
    #[test]
    fn nonstandard_trend_segment_months_are_compounded_by_actual_duration() {
        let first = GrowthFactor::from_annual_basis_points(1200, 2).unwrap();
        let second = GrowthFactor::from_annual_basis_points(-1000, 1).unwrap();
        let compound = first.compose(second).unwrap();
        assert!(compound.nanos() > GROWTH_FACTOR_SCALE);
        assert!(
            compound.nanos()
                < GrowthFactor::from_annual_basis_points(1200, 3)
                    .unwrap()
                    .nanos()
        );
        assert_ne!(
            compound,
            GrowthFactor::from_annual_basis_points(-1000, 3).unwrap()
        );
    }
    #[test]
    fn fixed_point_amount_rounds_total_half_even_and_preserves_i128_boundary() {
        let factor = GrowthFactor(1_500_000_000);
        assert_eq!(
            factor.apply(AccountingAmount::from_cents(1)).unwrap(),
            AccountingAmount::from_cents(2)
        );
        assert_eq!(
            factor.apply(AccountingAmount::from_cents(3)).unwrap(),
            AccountingAmount::from_cents(4)
        );
        assert_eq!(
            GrowthFactor(GROWTH_FACTOR_SCALE)
                .apply(AccountingAmount::MAX)
                .unwrap(),
            AccountingAmount::MAX
        );
        assert!(factor.apply(AccountingAmount::MAX).is_err());
    }
    #[test]
    fn factor_conversion_never_rounds_the_accounting_amount_through_float() {
        let amount = AccountingAmount::from_cents(9_007_199_254_740_993);
        let exact_annual = GrowthFactor::from_annual_basis_points(1200, 12).unwrap();
        assert_eq!(
            exact_annual.apply(amount).unwrap().cents(),
            10_088_063_165_309_912
        );
    }

    #[test]
    fn signed_amount_minimum_and_negative_ties_keep_exact_half_even_semantics() {
        let minimum = AccountingAmount::from_cents(i128::MIN);
        assert_eq!(
            GrowthFactor(GROWTH_FACTOR_SCALE).apply(minimum).unwrap(),
            minimum
        );
        assert!(GrowthFactor(1_500_000_000).apply(minimum).is_err());
        assert_eq!(
            GrowthFactor(1_500_000_000)
                .apply(AccountingAmount::from_cents(-1))
                .unwrap()
                .cents(),
            -2
        );
        assert_eq!(
            GrowthFactor(1_500_000_000)
                .apply(AccountingAmount::from_cents(-3))
                .unwrap()
                .cents(),
            -4
        );
    }
}

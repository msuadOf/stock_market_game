use super::ContractGroupState;
use crate::accounting::{AccountingAmount, AccountingError};
use crate::company::insurance::{InsuranceBooks, InsuranceError};

fn invalid(detail: impl Into<String>) -> InsuranceError {
    InsuranceError::InvalidRestoredState {
        detail: detail.into(),
    }
}

fn signed_sum(terms: &[(AccountingAmount, bool)]) -> Result<AccountingAmount, AccountingError> {
    let overflow = || AccountingError::AmountOverflow {
        op: "Insurance restored sum",
        detail: format!("{terms:?}"),
    };
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    for (amount, added) in terms {
        let magnitude = amount.cents().unsigned_abs();
        if magnitude == 0 {
            continue;
        }
        if amount.is_positive() == *added {
            positive.push(magnitude);
        } else {
            negative.push(magnitude);
        }
    }
    while let (Some(positive_tail), Some(negative_tail)) =
        (positive.last_mut(), negative.last_mut())
    {
        let cancelled = (*positive_tail).min(*negative_tail);
        *positive_tail -= cancelled;
        *negative_tail -= cancelled;
        let positive_empty = *positive_tail == 0;
        let negative_empty = *negative_tail == 0;
        if positive_empty {
            positive.pop();
        }
        if negative_empty {
            negative.pop();
        }
    }
    let is_negative = !negative.is_empty();
    let remaining = if is_negative { negative } else { positive };
    let magnitude = remaining.into_iter().try_fold(0_u128, |total, amount| {
        total.checked_add(amount).ok_or_else(overflow)
    })?;
    let cents = if is_negative {
        -i128::try_from(magnitude - 1).map_err(|_| overflow())? - 1
    } else {
        i128::try_from(magnitude).map_err(|_| overflow())?
    };
    Ok(AccountingAmount::from_cents(cents))
}

impl ContractGroupState {
    pub fn validate_restore(&self) -> Result<(), InsuranceError> {
        let measurement = &self.measurement;
        if measurement.coverage_end <= measurement.coverage_start
            || measurement.units_total <= 0
            || measurement.units_total
                != measurement
                    .coverage_end
                    .days_since(measurement.coverage_start)
            || !(0..=measurement.units_total).contains(&measurement.units_released)
        {
            return Err(invalid(format!(
                "coverage {}..{} disagrees with service units {}/{}",
                measurement.coverage_start,
                measurement.coverage_end,
                measurement.units_released,
                measurement.units_total
            )));
        }
        if !self.premium.is_positive() || self.premium_collected > self.premium {
            return Err(invalid(format!(
                "premium {:?}, premium_collected {:?}",
                self.premium, self.premium_collected
            )));
        }
        for (field, amount) in [
            ("premium_collected", self.premium_collected),
            (
                "expected_claims_remaining",
                measurement.expected_claims_remaining,
            ),
            (
                "risk_adjustment_remaining",
                measurement.risk_adjustment_remaining,
            ),
            ("csm", measurement.csm),
            ("loss_component", measurement.loss_component),
            ("finance_remaining", measurement.finance_remaining),
            ("day_one_loss", measurement.day_one_loss),
            ("released_revenue", measurement.released_revenue),
            ("released_finance", measurement.released_finance),
            ("reestimated_csm", measurement.reestimated_csm),
        ] {
            if amount.is_negative() {
                return Err(invalid(format!(
                    "{field} must be nonnegative, got {amount:?}"
                )));
            }
        }
        if measurement.csm.is_positive() && measurement.loss_component.is_positive() {
            return Err(invalid("csm and loss_component cannot both be positive"));
        }
        if measurement.units_released == measurement.units_total
            && [
                measurement.expected_claims_remaining,
                measurement.risk_adjustment_remaining,
                measurement.csm,
                measurement.loss_component,
                measurement.finance_remaining,
            ]
            .iter()
            .any(|amount| !amount.is_zero())
        {
            return Err(invalid(
                "completed coverage has nonzero remaining components",
            ));
        }
        for (field, carry) in [
            ("carried_claims", measurement.carried_claims),
            (
                "carried_risk_adjustment",
                measurement.carried_risk_adjustment,
            ),
            ("carried_csm", measurement.carried_csm),
            ("carried_finance", measurement.carried_finance),
            ("carried_loss", measurement.carried_loss),
        ] {
            if carry.units().unsigned_abs() > (measurement.units_total / 2) as u128 {
                return Err(invalid(format!(
                    "{field} {:?} exceeds half of {} service units",
                    carry, measurement.units_total
                )));
            }
        }
        let components = signed_sum(&[
            (measurement.expected_claims_remaining, true),
            (measurement.risk_adjustment_remaining, true),
            (measurement.csm, true),
            (measurement.finance_remaining, false),
        ])?;
        let reconciled = signed_sum(&[
            (self.premium, true),
            (measurement.day_one_loss, true),
            (measurement.remeasure_loss, true),
            (measurement.remeasure_finance, true),
            (measurement.released_finance, true),
            (measurement.released_revenue, false),
        ])?;
        if components != reconciled {
            return Err(invalid(format!(
                "GMM components {components:?} disagree with reconciliation {reconciled:?}"
            )));
        }
        for (id, claim) in self.claims.iter() {
            if !claim.incurred().is_positive()
                || claim.paid().is_negative()
                || claim.paid() > claim.incurred()
            {
                return Err(invalid(format!(
                    "claim {id:?} has incurred {:?}, paid {:?}",
                    claim.incurred(),
                    claim.paid()
                )));
            }
        }
        Ok(())
    }
}

impl InsuranceBooks {
    pub fn validate_restore(&self) -> Result<(), InsuranceError> {
        self.discount.validate()?;
        for (id, group) in &self.groups {
            group
                .validate_restore()
                .map_err(|error| invalid(format!("group {id:?}: {error}")))?;
            self.ensure_counterparty(group.policyholder())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signed_sum_handles_minimum_cancellation_and_real_overflow() {
        let minimum = AccountingAmount::from_cents(i128::MIN);
        let one = AccountingAmount::from_cents(1);
        assert_eq!(signed_sum(&[(minimum, true)]), Ok(minimum));
        assert_eq!(
            signed_sum(&[(minimum, true), (AccountingAmount::MAX, true), (one, true)]),
            Ok(AccountingAmount::ZERO)
        );
        assert_eq!(
            signed_sum(&[(AccountingAmount::MAX, true), (one, true), (one, false)]),
            Ok(AccountingAmount::MAX)
        );
        for terms in [
            vec![(AccountingAmount::MAX, true), (one, true)],
            vec![(minimum, true), (one, false)],
            vec![(minimum, false)],
        ] {
            assert!(matches!(
                signed_sum(&terms),
                Err(AccountingError::AmountOverflow { .. })
            ));
        }
    }
}

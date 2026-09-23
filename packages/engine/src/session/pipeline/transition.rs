use super::{FeeComponents, ReceiptDelta, ResVec};
use crate::{session::buy_order_reservation, session::fee_delta, GameConfig, Money};

#[derive(Clone, Copy)]
pub struct BuyFillInput<'a> {
    pub config: &'a GameConfig,
    pub limit: Money,
    pub fill_qty: u32,
    pub remaining_qty_after: u32,
    pub filled_value_before: Money,
    pub gross_delta: Money,
    pub live_before: ResVec,
}

#[derive(Clone, Copy)]
pub struct SellFillInput<'a> {
    pub config: &'a GameConfig,
    pub fill_qty: u32,
    pub remaining_qty_after: u32,
    pub filled_value_before: Money,
    pub gross_delta: Money,
    pub nominal_before: FeeComponents,
    pub charged_before: FeeComponents,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FillTransition {
    pub delta: ReceiptDelta,
    pub nominal: FeeComponents,
    pub nominal_after: FeeComponents,
    pub charged: FeeComponents,
    pub charged_after: FeeComponents,
    pub deliver_qty: u32,
    pub deliver_cash: Money,
}

impl FillTransition {
    pub fn buy(input: BuyFillInput<'_>) -> Result<Self, super::super::StepFatal> {
        require_positive(input.gross_delta, "buy gross delta must be positive")?;
        require_positive_qty(input.fill_qty, "buy fill quantity must be positive")?;
        let filled_value_after = input
            .filled_value_before
            .add(input.gross_delta)
            .map_err(invariant)?;
        let nominal_before = buyer_nominal_total(input.config, input.filled_value_before)?;
        let nominal_after = buyer_nominal_total(input.config, filled_value_after)?;
        let nominal = nominal_after.checked_sub(nominal_before)?;
        let spent_cash = input.gross_delta.add(nominal.total()?).map_err(invariant)?;
        let live_after_cash = buy_order_reservation(
            input.config,
            input.limit,
            input.remaining_qty_after,
            filled_value_after,
        )
        .map_err(invariant)?;
        let remaining_after_spend = input.live_before.cash.sub(spent_cash).map_err(invariant)?;
        let released_cash = remaining_after_spend
            .sub(live_after_cash)
            .map_err(invariant)?;
        require_nonnegative(released_cash, "buy reservation does not cover transition")?;

        Ok(Self {
            delta: ReceiptDelta::sealed(
                ResVec::new(spent_cash, 0),
                ResVec::new(released_cash, 0),
                ResVec::new(live_after_cash, 0),
            ),
            nominal,
            nominal_after,
            charged: nominal,
            charged_after: nominal_after,
            deliver_qty: input.fill_qty,
            deliver_cash: Money::ZERO,
        })
    }

    pub fn sell(input: SellFillInput<'_>) -> Result<Self, super::super::StepFatal> {
        require_positive(input.gross_delta, "sell gross delta must be positive")?;
        require_positive_qty(input.fill_qty, "sell fill quantity must be positive")?;
        let nominal_before = seller_nominal_total(input.config, input.filled_value_before)?;
        if input.nominal_before != nominal_before {
            return Err(invariant_message(
                "seller nominal audit does not match filled value",
            ));
        }
        validate_charged_chain(input.charged_before, nominal_before)?;
        if input.charged_before.total()? > input.filled_value_before {
            return Err(invariant_message(
                "seller charged history exceeds cumulative gross value",
            ));
        }
        let filled_value_after = input
            .filled_value_before
            .add(input.gross_delta)
            .map_err(invariant)?;
        let nominal_after = seller_nominal_total(input.config, filled_value_after)?;
        let nominal = nominal_after.checked_sub(nominal_before)?;
        let unpaid = nominal_after.checked_sub(input.charged_before)?;
        let charge_cap = unpaid.total()?.min(input.gross_delta);
        let charged = allocate_charged_components(unpaid, charge_cap)?;
        let charged_after = input.charged_before.checked_add(charged)?;
        validate_charged_chain(charged_after, nominal_after)?;
        let deliver_cash = input.gross_delta.sub(charge_cap).map_err(invariant)?;

        Ok(Self {
            delta: ReceiptDelta::sealed(
                ResVec::new(Money::ZERO, input.fill_qty),
                ResVec::ZERO,
                ResVec::new(Money::ZERO, input.remaining_qty_after),
            ),
            nominal,
            nominal_after,
            charged,
            charged_after,
            deliver_qty: 0,
            deliver_cash,
        })
    }
}

impl FeeComponents {
    fn checked_add(self, other: Self) -> Result<Self, super::super::StepFatal> {
        Ok(Self {
            commission: self.commission.add(other.commission).map_err(invariant)?,
            stamp_tax: self.stamp_tax.add(other.stamp_tax).map_err(invariant)?,
            transfer_fee: self
                .transfer_fee
                .add(other.transfer_fee)
                .map_err(invariant)?,
        })
    }

    fn checked_sub(self, other: Self) -> Result<Self, super::super::StepFatal> {
        let components = Self {
            commission: self.commission.sub(other.commission).map_err(invariant)?,
            stamp_tax: self.stamp_tax.sub(other.stamp_tax).map_err(invariant)?,
            transfer_fee: self
                .transfer_fee
                .sub(other.transfer_fee)
                .map_err(invariant)?,
        };
        validate_nonnegative_components(components)?;
        Ok(components)
    }
}

fn buyer_nominal_total(
    config: &GameConfig,
    filled_value: Money,
) -> Result<FeeComponents, super::super::StepFatal> {
    if filled_value == Money::ZERO {
        return Ok(FeeComponents::ZERO);
    }
    require_positive(filled_value, "buyer filled value must be nonnegative")?;
    Ok(FeeComponents {
        commission: fee_delta(Money::ZERO, filled_value, |amount| {
            config.commission(amount)
        })
        .map_err(invariant)?,
        stamp_tax: Money::ZERO,
        transfer_fee: fee_delta(Money::ZERO, filled_value, |amount| {
            config.transfer_fee(amount)
        })
        .map_err(invariant)?,
    })
}

fn seller_nominal_total(
    config: &GameConfig,
    filled_value: Money,
) -> Result<FeeComponents, super::super::StepFatal> {
    if filled_value == Money::ZERO {
        return Ok(FeeComponents::ZERO);
    }
    require_positive(filled_value, "seller filled value must be nonnegative")?;
    Ok(FeeComponents {
        commission: fee_delta(Money::ZERO, filled_value, |amount| {
            config.commission(amount)
        })
        .map_err(invariant)?,
        stamp_tax: fee_delta(Money::ZERO, filled_value, |amount| config.stamp_tax(amount))
            .map_err(invariant)?,
        transfer_fee: fee_delta(Money::ZERO, filled_value, |amount| {
            config.transfer_fee(amount)
        })
        .map_err(invariant)?,
    })
}

fn allocate_charged_components(
    unpaid: FeeComponents,
    charge_cap: Money,
) -> Result<FeeComponents, super::super::StepFatal> {
    let commission = unpaid.commission.min(charge_cap);
    let after_commission = charge_cap.sub(commission).map_err(invariant)?;
    let stamp_tax = unpaid.stamp_tax.min(after_commission);
    let after_stamp_tax = after_commission.sub(stamp_tax).map_err(invariant)?;
    let transfer_fee = unpaid.transfer_fee.min(after_stamp_tax);
    let charged = FeeComponents {
        commission,
        stamp_tax,
        transfer_fee,
    };
    if charged.total()? != charge_cap {
        return Err(invariant_message(
            "seller charge allocation does not exhaust capped charge",
        ));
    }
    Ok(charged)
}

fn validate_charged_chain(
    charged: FeeComponents,
    nominal: FeeComponents,
) -> Result<(), super::super::StepFatal> {
    validate_nonnegative_components(charged)?;
    validate_nonnegative_components(nominal)?;
    let _ = nominal.checked_sub(charged)?;
    Ok(())
}

fn validate_nonnegative_components(
    components: FeeComponents,
) -> Result<(), super::super::StepFatal> {
    for amount in [
        components.commission,
        components.stamp_tax,
        components.transfer_fee,
    ] {
        require_nonnegative(amount, "fee component must be nonnegative")?;
    }
    Ok(())
}

fn require_positive(value: Money, description: &str) -> Result<(), super::super::StepFatal> {
    if value.cents() <= 0 {
        return Err(invariant_message(description));
    }
    Ok(())
}

fn require_positive_qty(value: u32, description: &str) -> Result<(), super::super::StepFatal> {
    if value == 0 {
        return Err(invariant_message(description));
    }
    Ok(())
}

fn require_nonnegative(value: Money, description: &str) -> Result<(), super::super::StepFatal> {
    if value.cents() < 0 {
        return Err(invariant_message(description));
    }
    Ok(())
}

fn invariant(error: crate::MoneyError) -> super::super::StepFatal {
    invariant_message(&error.to_string())
}

fn invariant_message(description: &str) -> super::super::StepFatal {
    super::super::StepFatal::InvariantViolation {
        description: description.to_owned(),
        location: "pipeline::transition".to_owned(),
    }
}

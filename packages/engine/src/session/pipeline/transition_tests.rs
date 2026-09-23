use super::{transition::*, FeeComponents, ResVec};
use crate::{GameConfig, Money};

#[test]
fn buy_fill_reuses_reservation_and_releases_price_improvement() {
    let config = GameConfig::proposed_defaults();

    let transition = FillTransition::buy(BuyFillInput {
        config: &config,
        limit: Money::from_cents(1_000),
        fill_qty: 100,
        remaining_qty_after: 100,
        filled_value_before: Money::ZERO,
        gross_delta: Money::from_cents(90_000),
        live_before: ResVec::new(Money::from_cents(200_502), 0),
    })
    .unwrap();

    assert_eq!(
        transition.delta.spent,
        ResVec::new(Money::from_cents(90_501), 0)
    );
    assert_eq!(
        transition.delta.live_after,
        ResVec::new(Money::from_cents(100_001), 0)
    );
    assert_eq!(
        transition.delta.released,
        ResVec::new(Money::from_cents(10_000), 0)
    );
    assert_eq!(transition.deliver_qty, 100);
    assert_eq!(transition.deliver_cash, Money::ZERO);
}

#[test]
fn buy_fill_keeps_nominal_and_charged_audit_cumulative_across_legs() {
    let config = GameConfig::proposed_defaults();
    let first = FillTransition::buy(BuyFillInput {
        config: &config,
        limit: Money::from_cents(1_000),
        fill_qty: 100,
        remaining_qty_after: 100,
        filled_value_before: Money::ZERO,
        gross_delta: Money::from_cents(90_000),
        live_before: ResVec::new(Money::from_cents(200_502), 0),
    })
    .unwrap();
    let second = FillTransition::buy(BuyFillInput {
        config: &config,
        limit: Money::from_cents(1_000),
        fill_qty: 100,
        remaining_qty_after: 0,
        filled_value_before: Money::from_cents(90_000),
        gross_delta: Money::from_cents(90_000),
        live_before: first.delta.live_after,
    })
    .unwrap();

    assert_eq!(first.nominal_after.total().unwrap(), Money::from_cents(501));
    assert_eq!(second.nominal.total().unwrap(), Money::from_cents(1));
    assert_eq!(
        second.nominal_after.total().unwrap(),
        Money::from_cents(502)
    );
    assert_eq!(second.charged_after, second.nominal_after);
    assert_eq!(second.deliver_qty, 100);
}

#[test]
fn sell_fills_cap_actual_charges_and_recover_nominal_fee_arrears() {
    let config = GameConfig::proposed_defaults();
    let first = sell_fill(
        &config,
        Money::ZERO,
        FeeComponents::ZERO,
        FeeComponents::ZERO,
        100,
        2,
    );
    let second = sell_fill(
        &config,
        Money::from_cents(100),
        first.nominal_after,
        first.charged_after,
        100,
        1,
    );
    let third = sell_fill(
        &config,
        Money::from_cents(200),
        second.nominal_after,
        second.charged_after,
        1_000,
        0,
    );

    assert_eq!(first.charged.total().unwrap(), Money::from_cents(100));
    assert_eq!(second.charged.total().unwrap(), Money::from_cents(100));
    assert_eq!(third.charged.total().unwrap(), Money::from_cents(301));
    assert_eq!(first.deliver_cash, Money::ZERO);
    assert_eq!(second.deliver_cash, Money::ZERO);
    assert_eq!(third.deliver_cash, Money::from_cents(699));
    assert_eq!(first.deliver_qty, 0);
    assert_eq!(second.deliver_qty, 0);
    assert_eq!(third.deliver_qty, 0);
    assert_eq!(first.delta.spent.cash, Money::ZERO);
    assert_eq!(second.delta.spent.cash, Money::ZERO);
    assert_eq!(third.delta.spent.cash, Money::ZERO);
    assert_eq!(first.delta.spent.shares, 1);
    assert_eq!(second.delta.spent.shares, 1);
    assert_eq!(third.delta.spent.shares, 1);
    assert_eq!(first.delta.live_after.shares, 2);
    assert_eq!(second.delta.live_after.shares, 1);
    assert_eq!(third.delta.live_after.shares, 0);
    assert_eq!(first.delta.live_after.cash, Money::ZERO);
    assert_eq!(second.delta.live_after.cash, Money::ZERO);
    assert_eq!(third.delta.live_after.cash, Money::ZERO);
}

#[test]
fn sell_fill_allocates_capped_charge_commission_then_stamp_then_transfer() {
    let config = GameConfig::proposed_defaults();
    let first = sell_fill(
        &config,
        Money::ZERO,
        FeeComponents::ZERO,
        FeeComponents::ZERO,
        100,
        2,
    );
    let second = sell_fill(
        &config,
        Money::from_cents(100),
        first.nominal_after,
        first.charged_after,
        100,
        1,
    );
    let third = sell_fill(
        &config,
        Money::from_cents(200),
        second.nominal_after,
        second.charged_after,
        1_000,
        0,
    );

    assert_eq!(third.charged.commission, Money::from_cents(300));
    assert_eq!(third.charged.stamp_tax, Money::from_cents(1));
    assert_eq!(third.charged.transfer_fee, Money::ZERO);
    assert_eq!(third.charged_after, third.nominal_after);
}

#[test]
fn fill_transitions_reject_invalid_reservation_and_fee_audit_chains() {
    let config = GameConfig::proposed_defaults();
    let invalid_buy = FillTransition::buy(BuyFillInput {
        config: &config,
        limit: Money::from_cents(1_000),
        fill_qty: 100,
        remaining_qty_after: 100,
        filled_value_before: Money::ZERO,
        gross_delta: Money::from_cents(90_000),
        live_before: ResVec::new(Money::from_cents(1), 0),
    });
    let invalid_sell = FillTransition::sell(SellFillInput {
        config: &config,
        fill_qty: 1,
        remaining_qty_after: 0,
        filled_value_before: Money::from_cents(100),
        gross_delta: Money::from_cents(100),
        nominal_before: FeeComponents::ZERO,
        charged_before: FeeComponents::ZERO,
    });
    let nominal_first_leg = sell_fill(
        &config,
        Money::ZERO,
        FeeComponents::ZERO,
        FeeComponents::ZERO,
        100,
        0,
    )
    .nominal_after;
    let invalid_charge = FillTransition::sell(SellFillInput {
        config: &config,
        fill_qty: 1,
        remaining_qty_after: 0,
        filled_value_before: Money::from_cents(100),
        gross_delta: Money::from_cents(100),
        nominal_before: nominal_first_leg,
        charged_before: FeeComponents {
            commission: Money::from_cents(501),
            ..FeeComponents::ZERO
        },
    });

    assert!(invalid_buy.is_err());
    assert!(invalid_sell.is_err());
    assert!(invalid_charge.is_err());
}

#[test]
fn sell_fill_rejects_charged_history_above_cumulative_gross() {
    let config = GameConfig::proposed_defaults();
    let nominal_before = sell_fill(
        &config,
        Money::ZERO,
        FeeComponents::ZERO,
        FeeComponents::ZERO,
        100,
        0,
    )
    .nominal_after;

    let result = FillTransition::sell(SellFillInput {
        config: &config,
        fill_qty: 1,
        remaining_qty_after: 0,
        filled_value_before: Money::from_cents(100),
        gross_delta: Money::from_cents(100),
        nominal_before,
        charged_before: FeeComponents {
            commission: Money::from_cents(101),
            ..FeeComponents::ZERO
        },
    });

    assert!(result.is_err());
}

fn sell_fill(
    config: &GameConfig,
    filled_value_before: Money,
    nominal_before: FeeComponents,
    charged_before: FeeComponents,
    gross_delta: i64,
    remaining_qty_after: u32,
) -> FillTransition {
    FillTransition::sell(SellFillInput {
        config,
        fill_qty: 1,
        remaining_qty_after,
        filled_value_before,
        gross_delta: Money::from_cents(gross_delta),
        nominal_before,
        charged_before,
    })
    .unwrap()
}

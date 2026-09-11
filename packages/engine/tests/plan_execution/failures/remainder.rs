use super::*;
use engine::plans::quote_policy::QuoteAction;
use engine::session::{Event, PlanExecutionDisposition};

#[test]
fn buy_sub_lot_remainder_is_not_rounded_up_into_an_overbuy() {
    let mut session = session(setup(350, 8, 0, 0));
    let mut plans = PlanBook::default();
    let seller = create_plan(&mut plans, AccountId(2), Side::Sell, 350);
    let buyer = create_plan(&mut plans, AccountId(1), Side::Buy, 400);
    session
        .execute_plan_observation(
            &mut plans,
            request(
                seller,
                Side::Sell,
                QuoteAction::Submit {
                    price: Money::from_cents(1_000),
                    qty: 350,
                },
            ),
        )
        .expect("odd-lot full-position sell is legal");
    let bought = session
        .execute_plan_observation(
            &mut plans,
            request(
                buyer,
                Side::Buy,
                QuoteAction::Submit {
                    price: Money::from_cents(1_000),
                    qty: 400,
                },
            ),
        )
        .expect("board-lot buy partially fills");
    let active_id = submitted_id(&bought);
    let canceled = session
        .execute_plan_observation(
            &mut plans,
            request(
                buyer,
                Side::Buy,
                QuoteAction::Cancel {
                    order_id: active_id,
                },
            ),
        )
        .expect("real cancellation succeeds");

    let retry = session
        .execute_plan_observation(
            &mut plans,
            request(
                buyer,
                Side::Buy,
                QuoteAction::Submit {
                    price: Money::from_cents(1_001),
                    qty: 100,
                },
            ),
        )
        .expect("sub-lot remainder is explicit");

    assert!(canceled
        .events
        .iter()
        .any(|event| matches!(event, Event::OrderCanceled { id, .. } if *id == active_id)));
    assert!(matches!(
        retry.disposition,
        PlanExecutionDisposition::RemainingBelowBoardLot { remaining_qty: 50 }
    ));
    assert_eq!(plans.plan(buyer).expect("plan remains").filled_qty, 350);
    assert!(session.save().resting_orders[&code()].is_empty());
}

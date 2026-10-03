use super::*;

fn order(id: u64, side: Side, price: i64, qty: u32) -> Order {
    Order {
        id: OrderId(id),
        side,
        price: Money::from_cents(price),
        qty,
        original_qty: qty,
        filled_qty: 0,
        filled_value: Money::ZERO,
        owner: AccountId(id + 100),
        seq: 0,
    }
}

#[test]
fn partial_full_cancel_and_clone_keep_identity_and_fifo() {
    let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
    book.place(order(1, Side::Sell, 100, 3)).unwrap();
    book.place(order(2, Side::Sell, 100, 1)).unwrap();
    let snapshot = book.clone();
    let original_seq = book.resting_order_by_id(OrderId(1)).unwrap().seq;
    let result = book.place(order(3, Side::Buy, 100, 2)).unwrap();
    assert_eq!(result.trades[0].maker_order_id, OrderId(1));
    let remaining = book.resting_order_by_id(OrderId(1)).unwrap();
    assert_eq!(
        (remaining.qty, remaining.filled_qty, remaining.seq),
        (1, 2, original_seq)
    );
    assert_eq!(remaining.filled_value, Money::from_cents(200));
    assert_eq!(snapshot.resting_order_by_id(OrderId(1)).unwrap().qty, 3);
    assert_eq!(snapshot.filled_order_owner(OrderId(3)), None);

    let result = book.place(order(4, Side::Buy, 100, 1)).unwrap();
    assert_eq!(result.trades[0].maker_order_id, OrderId(1));
    assert!(book.resting_order_by_id(OrderId(1)).is_none());
    assert_eq!(book.filled_order_owner(OrderId(1)), Some(AccountId(101)));
    assert!(matches!(
        book.cancel(OrderId(1)),
        Err(OrderError::OrderAlreadyFilled(OrderId(1)))
    ));
    assert_eq!(book.cancel(OrderId(2)).unwrap().owner, AccountId(102));
    assert_eq!(book.resting_order_count(), 0);
    book.clear();
    assert_eq!(book.filled_order_owner(OrderId(1)), Some(AccountId(101)));
}

#[test]
fn maker_money_overflow_precedes_taker_money_overflow_without_book_write() {
    for side in [Side::Buy, Side::Sell] {
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        let maker_side = if side == Side::Buy {
            Side::Sell
        } else {
            Side::Buy
        };
        let mut maker = order(1, maker_side, 2, 2);
        maker.filled_value = Money::from_cents(i64::MAX - 1);
        book.place(maker).unwrap();
        let before = book.resting_orders();
        let mut taker = order(2, side, 2, 1);
        taker.filled_value = Money::from_cents(i64::MAX);
        let error = book.place(taker).unwrap_err();
        match error {
            OrderError::Money(MoneyError::Overflow { op, operand }) => {
                assert_eq!(op, "add");
                assert_eq!(operand, format!("{} + 2", i64::MAX - 1));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(book.resting_orders(), before);
        assert!(book.filled_orders().is_empty());
    }
}

#[test]
fn taker_money_overflow_leaves_book_status_and_cursor_unchanged() {
    for side in [Side::Buy, Side::Sell] {
        let maker_side = if side == Side::Buy {
            Side::Sell
        } else {
            Side::Buy
        };
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        book.place(order(1, maker_side, 2, 2)).unwrap();
        let before = book.resting_orders();
        let before_filled = book.filled_orders();
        let before_cursor = book.next_sequence();
        let mut taker = order(2, side, 2, 1);
        taker.filled_value = Money::from_cents(i64::MAX);
        match book.place(taker).unwrap_err() {
            OrderError::Money(MoneyError::Overflow { op, operand }) => {
                assert_eq!(op, "add");
                assert_eq!(operand, format!("{} + 2", i64::MAX));
            }
            other => panic!("unexpected error: {other:?}"),
        }
        assert_eq!(book.resting_orders(), before);
        assert_eq!(book.filled_orders(), before_filled);
        assert_eq!(book.next_sequence(), before_cursor);
    }
}

#[test]
fn projection_later_insert_error_preserves_earlier_write_and_old_cursor() {
    let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
    let first = order(1, Side::Sell, 100, 1);
    let second = order(2, Side::Sell, 100, 1);
    let delta = OrderBookDelta {
        expected_next_seq: 0,
        next_seq: 2,
        changes: vec![
            OrderBookChange {
                id: first.id,
                before: None,
                after: Some(first.clone()),
                filled_owner: None,
            },
            OrderBookChange {
                id: second.id,
                before: None,
                after: Some(second),
                filled_owner: None,
            },
        ],
    };
    assert!(matches!(
        book.apply_changes(delta),
        Err(OrderError::ProjectionMismatch { .. })
    ));
    assert_eq!(book.resting_orders(), vec![first]);
    assert_eq!(book.next_sequence(), 0);
}

#[test]
fn duplicate_qty_value_progress_price_keep_first_error_order() {
    let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
    book.place(order(1, Side::Sell, 100, 1)).unwrap();
    let mut invalid = order(1, Side::Sell, 0, 0);
    invalid.filled_value = Money::from_cents(-1);
    invalid.original_qty = 1;
    assert!(matches!(
        book.place(invalid.clone()),
        Err(OrderError::DuplicateOrderId(OrderId(1)))
    ));
    invalid.id = OrderId(2);
    assert!(matches!(
        book.place(invalid.clone()),
        Err(OrderError::InvalidQty(0))
    ));
    invalid.qty = 1;
    invalid.original_qty = 2;
    assert!(matches!(
        book.place(invalid.clone()),
        Err(OrderError::InvalidFilledValue(_))
    ));
    invalid.filled_value = Money::ZERO;
    assert!(matches!(
        book.place(invalid.clone()),
        Err(OrderError::InvalidQuantityProgress { .. })
    ));
    invalid.original_qty = 1;
    assert!(matches!(
        book.place(invalid),
        Err(OrderError::InvalidPrice { .. })
    ));
}

#[test]
fn valid_fill_progress_reaches_u32_boundary_on_both_sides() {
    for side in [Side::Buy, Side::Sell] {
        let maker_side = if side == Side::Buy {
            Side::Sell
        } else {
            Side::Buy
        };
        let mut book = OrderBook::new(Money::from_cents(1)).unwrap();
        let mut maker = order(1, maker_side, 1, 2);
        maker.original_qty = u32::MAX;
        maker.filled_qty = u32::MAX - 2;
        book.place(maker).unwrap();
        let mut taker = order(2, side, 1, 1);
        taker.original_qty = u32::MAX;
        taker.filled_qty = u32::MAX - 1;
        let result = book.place(taker).unwrap();
        assert!(result.resting.is_none());
        assert_eq!(book.filled_order_owner(OrderId(2)), Some(AccountId(102)));
        let remaining = book.resting_order_by_id(OrderId(1)).unwrap();
        assert_eq!((remaining.qty, remaining.filled_qty), (1, u32::MAX - 1));
        book.place(order(3, side, 1, 1)).unwrap();
        assert_eq!(book.filled_order_owner(OrderId(1)), Some(AccountId(101)));
        assert!(book.resting_order_by_id(OrderId(1)).is_none());
    }
}

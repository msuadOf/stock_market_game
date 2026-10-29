//! Trade 名册投影保留真实 AccountId，并对整批失败保持游标原子性。

use super::project_trade_events_for_roster;
use crate::runtime::{protocol::DeskTradeFact, Owner};
use crate::{AccountId, Event, Money, Side, StockId};

fn trade(maker: Owner, taker: Owner) -> DeskTradeFact {
    DeskTradeFact {
        stock: 1,
        price: 995,
        qty: 100,
        maker,
        taker,
        taker_side: Side::Sell,
        maker_fees_cents: 0,
        taker_fees_cents: 0,
        maker_fee_split: crate::fees::FeeComponents::ZERO,
        taker_fee_split: crate::fees::FeeComponents::ZERO,
        maker_order_serial: 1,
        taker_order_serial: 2,
    }
}

#[test]
fn sparse_account_ids_and_three_group_tail_preserve_exact_trade_fields() {
    let ids = [0, 99, 201, 400, 502, 699, 900].map(AccountId);
    let trades = [trade((0, 1), (1, 2)), trade((2, 0), (0, 0))];
    let mut seq = 41;
    let events = project_trade_events_for_roster(&trades, &ids, 3, &mut seq).unwrap();
    let fields = events
        .into_iter()
        .map(|event| match event {
            Event::Trade {
                seq,
                code,
                price,
                qty,
                maker,
                taker,
            } => (seq, code, price, qty, maker, taker),
            other => panic!("Trade 投影产生其他事件：{other:?}"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        fields,
        vec![
            (
                42,
                StockId(1),
                Money::from_cents(995),
                100,
                AccountId(99),
                AccountId(699)
            ),
            (
                43,
                StockId(1),
                Money::from_cents(995),
                100,
                AccountId(900),
                AccountId(0)
            ),
        ]
    );
    assert_eq!(seq, 43);
}

#[test]
fn invalid_owner_after_valid_trade_rejects_batch_without_advancing_seq() {
    let ids = [0, 99, 201, 400, 502, 699, 900].map(AccountId);
    // 非法 local 不能别名到下一组；未知 group 与尾组空位不能造账户。
    for invalid in [(0, 3), (3, 0), (2, 1)] {
        for bad_maker in [false, true] {
            let invalid_trade = if bad_maker {
                trade(invalid, (0, 0))
            } else {
                trade((0, 0), invalid)
            };
            let mut seq = 41;
            let result = project_trade_events_for_roster(
                &[trade((0, 1), (1, 2)), invalid_trade],
                &ids,
                3,
                &mut seq,
            );
            assert!(result.is_err(), "必须拒绝原始 Owner：{invalid:?}");
            assert_eq!(seq, 41, "后条失败不得消费前条事件序号");
        }
    }
}

#[test]
fn zero_group_width_and_sequence_overflow_are_explicit_and_atomic() {
    let ids = [AccountId(0), AccountId(99)];
    let mut seq = 41;
    assert!(project_trade_events_for_roster(&[trade((0, 1), (0, 0))], &ids, 0, &mut seq).is_err());
    assert_eq!(seq, 41);
    seq = u64::MAX - 1;
    assert!(project_trade_events_for_roster(
        &[trade((0, 1), (0, 0)), trade((0, 1), (0, 0))],
        &ids,
        2,
        &mut seq,
    )
    .is_err());
    assert_eq!(seq, u64::MAX - 1);
}

#[test]
fn empty_success_does_not_consume_sequence() {
    let mut seq = 41;
    assert!(
        project_trade_events_for_roster(&[], &[AccountId(0)], 1, &mut seq)
            .unwrap()
            .is_empty()
    );
    assert_eq!(seq, 41);
}

use super::*;
use InformationTradingAnchor::{DayBefore, NoPriorSession, TradingDay};

#[test]
fn information_anchor_wire_preserves_three_states_and_full_u64() {
    for (anchor, wire) in [
        (NoPriorSession, r#"{"kind":"NoPriorSession"}"#),
        (
            TradingDay { ordinal: 0 },
            r#"{"kind":"TradingDay","ordinal":"0"}"#,
        ),
        (
            DayBefore { ordinal: 0 },
            r#"{"kind":"DayBefore","ordinal":"0"}"#,
        ),
        (
            TradingDay { ordinal: u64::MAX },
            r#"{"kind":"TradingDay","ordinal":"18446744073709551615"}"#,
        ),
        (
            DayBefore { ordinal: u64::MAX },
            r#"{"kind":"DayBefore","ordinal":"18446744073709551615"}"#,
        ),
    ] {
        assert_eq!(serde_json::to_string(&anchor).unwrap(), wire);
        assert_eq!(
            serde_json::from_str::<InformationTradingAnchor>(wire).unwrap(),
            anchor
        );
    }
    assert_ne!(NoPriorSession, TradingDay { ordinal: 0 });
    assert_ne!(TradingDay { ordinal: 0 }, DayBefore { ordinal: 0 });
    assert_eq!(
        serde_json::from_str::<InformationTradingAnchor>(r#"{"ordinal":"7","kind":"DayBefore"}"#)
            .unwrap(),
        DayBefore { ordinal: 7 },
    );
}

#[test]
fn information_anchor_wire_rejects_wrong_shapes_and_fields() {
    for wire in [
        "0",
        "null",
        "false",
        r#""NoPriorSession""#,
        "[]",
        "{}",
        r#"{"kind":"Unknown"}"#,
        r#"{"kind":0}"#,
        r#"{"ordinal":"0"}"#,
        r#"{"kind":"TradingDay"}"#,
        r#"{"kind":"DayBefore"}"#,
        r#"{"kind":"NoPriorSession","ordinal":"0"}"#,
        r#"{"kind":"NoPriorSession","extra":0}"#,
        r#"{"kind":"TradingDay","ordinal":"0","extra":0}"#,
        r#"{"kind":"TradingDay","ordinal":0}"#,
        r#"{"kind":"DayBefore","ordinal":null}"#,
        r#"{"kind":"TradingDay","ordinal":true}"#,
        r#"{"kind":"TradingDay","kind":"TradingDay","ordinal":"0"}"#,
        r#"{"kind":"TradingDay","ordinal":"0","ordinal":"0"}"#,
    ] {
        assert!(
            serde_json::from_str::<InformationTradingAnchor>(wire).is_err(),
            "应拒绝 {wire}"
        );
    }
}

#[test]
fn information_anchor_wire_rejects_noncanonical_decimal_ordinals() {
    for ordinal in [
        "",
        " 0",
        "0 ",
        "+0",
        "+1",
        "-0",
        "-1",
        "00",
        "01",
        "1.0",
        "1e0",
        "１２",
        "١",
        "18446744073709551616",
    ] {
        for kind in ["TradingDay", "DayBefore"] {
            let wire = serde_json::json!({ "kind": kind, "ordinal": ordinal });
            assert!(
                serde_json::from_value::<InformationTradingAnchor>(wire).is_err(),
                "应拒绝 {kind} ordinal={ordinal:?}"
            );
        }
    }
}

#[test]
fn information_anchor_history_order_interleaves_variants_without_overflow() {
    let anchors = [
        NoPriorSession,
        TradingDay { ordinal: 0 },
        DayBefore { ordinal: 0 },
        TradingDay { ordinal: 1 },
        DayBefore { ordinal: 1 },
        TradingDay { ordinal: u64::MAX },
        DayBefore { ordinal: u64::MAX },
    ];
    for (left_index, left) in anchors.into_iter().enumerate() {
        for (right_index, right) in anchors.into_iter().enumerate() {
            assert_eq!(left.chronological_cmp(right), left_index.cmp(&right_index));
            assert_eq!(
                ensure_information_anchor_not_after(left, right).is_ok(),
                left_index <= right_index
            );
        }
    }
}

#[test]
fn information_anchor_history_reversal_retains_both_typed_identities() {
    let anchor = DayBefore { ordinal: 0 };
    let as_of = TradingDay { ordinal: 0 };
    assert_eq!(
        ensure_information_anchor_not_after(anchor, as_of),
        Err(InformationHorizonError::TimeReversed { anchor, as_of }),
    );
    assert_eq!(ensure_information_anchor_not_after(as_of, anchor), Ok(()));
    assert!(ensure_information_anchor_not_after(
        TradingDay { ordinal: u64::MAX },
        DayBefore { ordinal: 1 }
    )
    .is_err());
}

#[test]
fn information_horizon_position_and_history_order_are_distinct() {
    assert_eq!(information_session_position(NoPriorSession), 0);
    assert_eq!(information_session_position(TradingDay { ordinal: 0 }), 1);
    assert_eq!(information_session_position(DayBefore { ordinal: 0 }), 1);
    assert_eq!(
        information_session_position(TradingDay { ordinal: u64::MAX }),
        u128::from(u64::MAX) + 1
    );
    // 期限数学同位不认证历史顺序；业务还必须执行独立 nonfuture 守卫。
    assert_eq!(
        information_horizon_remaining(DayBefore { ordinal: 0 }, 2, TradingDay { ordinal: 0 }),
        Ok(2)
    );
    assert!(ensure_information_anchor_not_after(
        DayBefore { ordinal: 0 },
        TradingDay { ordinal: 0 }
    )
    .is_err());
}

#[test]
fn information_horizon_keeps_closed_position_and_expires_at_boundary() {
    assert_eq!(
        information_horizon_remaining(NoPriorSession, 2, TradingDay { ordinal: 0 }),
        Ok(1)
    );
    for as_of in [TradingDay { ordinal: 0 }, DayBefore { ordinal: 0 }] {
        assert_eq!(
            information_horizon_remaining(TradingDay { ordinal: 0 }, 2, as_of),
            Ok(2)
        );
    }
    assert_eq!(
        information_horizon_remaining(TradingDay { ordinal: 0 }, 2, TradingDay { ordinal: 1 }),
        Ok(1)
    );
    assert_eq!(
        information_horizon_remaining(TradingDay { ordinal: 0 }, 2, TradingDay { ordinal: 2 }),
        Ok(0)
    );
    assert_eq!(
        information_horizon_remaining(TradingDay { ordinal: 0 }, 2, DayBefore { ordinal: 8 }),
        Ok(0)
    );
}

#[test]
fn information_horizon_rejects_zero_and_position_reversal_without_wrapping() {
    assert_eq!(
        information_horizon_remaining(NoPriorSession, 0, NoPriorSession),
        Err(InformationHorizonError::ZeroHorizon)
    );
    let anchor = TradingDay { ordinal: 1 };
    let as_of = DayBefore { ordinal: 0 };
    assert_eq!(
        information_horizon_remaining(anchor, 1, as_of),
        Err(InformationHorizonError::TimeReversed { anchor, as_of })
    );
    for anchor in [
        TradingDay { ordinal: u64::MAX },
        DayBefore { ordinal: u64::MAX },
    ] {
        assert_eq!(
            information_horizon_remaining(anchor, u16::MAX, anchor),
            Ok(u16::MAX)
        );
    }
}

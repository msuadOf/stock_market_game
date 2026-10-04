//! engine money 模块集成测试（TDD 红绿循环）。
use engine::money::MoneyError;
use serde_json::json;

#[test]
fn audit_boundary_money_requires_at_least_one_decimal_digit() {
    for input in [".", "+.", "-.", " . ", " +. ", " -. "] {
        assert!(
            matches!(
                Money::from_yuan_str(input),
                Err(MoneyError::ParseFailed { .. })
            ),
            "{input}"
        );
    }
    for (input, cents) in [(".5", 50), ("12.", 1200), ("+.0", 0), ("-.1", -10)] {
        assert_eq!(Money::from_yuan_str(input).unwrap().cents(), cents);
    }
}

#[test]
fn money_error_variants_construct_and_display() {
    let precision_parse_error = MoneyError::ParseFailed {
        input: "12.345".to_string(),
        reason: "too many digits".to_string(),
    };
    assert!(precision_parse_error.to_string().contains("12.345"));

    let addition_overflow_error = MoneyError::Overflow {
        op: "add",
        operand: "i64 max".to_string(),
    };
    assert!(addition_overflow_error.to_string().contains("add"));

    let invalid_rate_error = MoneyError::InvalidRate { rate: f64::NAN };
    assert!(invalid_rate_error.to_string().contains("NaN"));
}

use engine::money::Money;

#[test]
fn money_from_cents_and_zero() {
    let m = Money::from_cents(1234);
    assert_eq!(m.cents(), 1234);
    assert_eq!(Money::ZERO.cents(), 0);
}

#[test]
fn money_equality_and_ordering() {
    assert_eq!(Money::from_cents(100), Money::from_cents(100));
    assert!(Money::from_cents(100) < Money::from_cents(200));
}

#[test]
fn money_is_copy() {
    let a = Money::from_cents(50);
    let b = a; // copy
    assert_eq!(a.cents(), 50); // a 仍可用
    assert_eq!(b.cents(), 50);
}

#[test]
fn money_supports_negative() {
    assert_eq!(Money::from_cents(-1).cents(), -1);
}

#[test]
fn money_add_sub_exact() -> Result<(), MoneyError> {
    let a = Money::from_cents(100);
    let b = Money::from_cents(25);
    assert_eq!(a.add(b)?.cents(), 125);
    assert_eq!(a.sub(b)?.cents(), 75);
    assert_eq!(
        Money::from_cents(10).sub(Money::from_cents(30))?.cents(),
        -20
    );
    Ok(())
}

#[test]
fn money_mul_shares_exact() -> Result<(), MoneyError> {
    // 单价 12.34 元 × 100 股 = 1234.00 元 = 123400 分
    let price = Money::from_cents(1234);
    assert_eq!(price.mul_shares(100)?.cents(), 123_400);
    // 0 股
    assert_eq!(price.mul_shares(0)?.cents(), 0);
    Ok(())
}

#[test]
fn money_add_overflow_returns_err() {
    let maxed = Money::from_cents(i64::MAX);
    let err = maxed.add(Money::from_cents(1)).unwrap_err();
    assert!(matches!(err, MoneyError::Overflow { op: "add", .. }));
}

#[test]
fn money_mul_shares_overflow_returns_err() {
    // i64::MAX 分 × 2 股必溢出
    let err = Money::from_cents(i64::MAX).mul_shares(2).unwrap_err();
    assert!(matches!(
        err,
        MoneyError::Overflow {
            op: "mul_shares",
            ..
        }
    ));
}

#[test]
fn money_from_yuan_str_valid() -> Result<(), MoneyError> {
    assert_eq!(Money::from_yuan_str("12.34")?.cents(), 1234);
    assert_eq!(Money::from_yuan_str("-0.01")?.cents(), -1);
    assert_eq!(Money::from_yuan_str("-1")?.cents(), -100);
    assert_eq!(Money::from_yuan_str("-1.50")?.cents(), -150);
    assert_eq!(Money::from_yuan_str("-12.34")?.cents(), -1234);
    assert_eq!(Money::from_yuan_str("0.1")?.cents(), 10);
    assert_eq!(Money::from_yuan_str("100")?.cents(), 10000);
    assert_eq!(Money::from_yuan_str("12.")?.cents(), 1200);
    assert_eq!(Money::from_yuan_str(".5")?.cents(), 50);
    assert_eq!(Money::from_yuan_str("+3.50")?.cents(), 350);
    Ok(())
}

#[test]
fn money_from_yuan_str_invalid() {
    assert!(matches!(
        Money::from_yuan_str("12.345"),
        Err(MoneyError::ParseFailed { .. })
    )); // 超过 2 位
    assert!(matches!(
        Money::from_yuan_str(""),
        Err(MoneyError::ParseFailed { .. })
    )); // 空
    assert!(matches!(
        Money::from_yuan_str("abc"),
        Err(MoneyError::ParseFailed { .. })
    )); // 非数字
    assert!(matches!(
        Money::from_yuan_str("1.2.3"),
        Err(MoneyError::ParseFailed { .. })
    )); // 多点
    assert!(matches!(
        Money::from_yuan_str("--1"),
        Err(MoneyError::ParseFailed { .. })
    )); // 多负号
    assert!(matches!(
        Money::from_yuan_str("12.3a"),
        Err(MoneyError::ParseFailed { .. })
    )); // 尾部非数字
}

#[test]
fn apply_rate_round_half_to_even_half_boundaries() -> Result<(), MoneyError> {
    // 0.5 边界 → 最近偶数（round-half-to-even）
    // 250 分 × 0.01 = 2.5 → 2 分（偶数）
    assert_eq!(Money::from_cents(250).apply_rate(0.01)?.cents(), 2);
    // 350 分 × 0.01 = 3.5 → 4 分（偶数）
    assert_eq!(Money::from_cents(350).apply_rate(0.01)?.cents(), 4);
    // 750 分 × 0.01 = 7.5 → 8 分（偶数）
    assert_eq!(Money::from_cents(750).apply_rate(0.01)?.cents(), 8);
    // 150 分 × 0.01 = 1.5 → 2 分（偶数）
    assert_eq!(Money::from_cents(150).apply_rate(0.01)?.cents(), 2);
    Ok(())
}

#[test]
fn apply_rate_non_half_normal_rounding() -> Result<(), MoneyError> {
    // 12.3 → 12（正常四舍，<0.5）
    assert_eq!(Money::from_cents(123).apply_rate(0.10)?.cents(), 12);
    // 12.8 → 13（>0.5 进位）
    assert_eq!(Money::from_cents(128).apply_rate(0.10)?.cents(), 13);
    Ok(())
}

#[test]
fn apply_rate_typical_commission() -> Result<(), MoneyError> {
    // 成交额 10000.00 元(=1_000_000 分) × 0.00025 = 2.50 元 = 250 分
    // 2.50 在「分」尺度即整数 250，无半边界争议
    assert_eq!(
        Money::from_cents(1_000_000).apply_rate(0.00025)?.cents(),
        250
    );
    Ok(())
}

#[test]
fn apply_rate_negative_toward_even() -> Result<(), MoneyError> {
    // -10 分 × 0.50 = -5.0 → -5 为奇数？-5.0 精确，取 -5。
    // 关键：-5 恰为整数，无半边界；验证负数方向不翻转
    assert_eq!(Money::from_cents(-10).apply_rate(0.50)?.cents(), -5);
    // -50 分 × 0.01 = -0.5 → 0（向偶数 0，非 -1）
    assert_eq!(Money::from_cents(-50).apply_rate(0.01)?.cents(), 0);
    Ok(())
}

#[test]
fn apply_rate_nan_inf_rejected() {
    assert!(matches!(
        Money::from_cents(100).apply_rate(f64::NAN),
        Err(MoneyError::InvalidRate { .. })
    ));
    assert!(matches!(
        Money::from_cents(100).apply_rate(f64::INFINITY),
        Err(MoneyError::InvalidRate { .. })
    ));
    assert!(matches!(
        Money::from_cents(100).apply_rate(f64::NEG_INFINITY),
        Err(MoneyError::InvalidRate { .. })
    ));
}

#[test]
fn apply_rate_rejects_result_outside_i64_range() {
    assert!(matches!(
        Money::from_cents(i64::MAX).apply_rate(2.0),
        Err(MoneyError::Overflow {
            op: "apply_rate",
            ..
        })
    ));
    assert!(matches!(
        Money::from_cents(i64::MIN).apply_rate(2.0),
        Err(MoneyError::Overflow {
            op: "apply_rate",
            ..
        })
    ));
}

#[test]
fn apply_rate_zero_rate() -> Result<(), MoneyError> {
    assert_eq!(Money::from_cents(12345).apply_rate(0.0)?.cents(), 0);
    Ok(())
}

#[test]
fn money_serde_roundtrip_preserves_cents() -> Result<(), Box<dyn std::error::Error>> {
    for cents in [
        0i64,
        1,
        -1,
        1234,
        9_999_999,
        -5555,
        1_000_000_000_000,
        9_007_199_254_740_993,
        i64::MIN,
        i64::MAX,
    ] {
        let m = Money::from_cents(cents);
        let j = serde_json::to_value(m)?;
        assert_eq!(j, json!(cents.to_string()), "serialize cents {cents}");
        let back: Money = serde_json::from_value(j)?;
        assert_eq!(back.cents(), cents, "deserialize cents {cents}");
    }
    Ok(())
}

#[test]
fn money_serde_is_decimal_cents_string() -> Result<(), Box<dyn std::error::Error>> {
    let j = serde_json::to_value(Money::from_cents(42))?;
    assert_eq!(j, json!("42"));
    assert_eq!(j.as_str(), Some("42"));
    Ok(())
}

#[test]
fn money_serde_rejects_noncanonical_strings_and_out_of_range_cents() {
    for input in [
        "",
        "+1",
        "01",
        "-01",
        "-0",
        "1.0",
        "1e3",
        " 1",
        "1 ",
        "-",
        "１２",
        "9223372036854775808",
        "-9223372036854775809",
    ] {
        assert!(
            serde_json::from_value::<Money>(json!(input)).is_err(),
            "{input:?}"
        );
    }
}

#[test]
fn money_serde_rejects_numeric_and_other_json_shapes() {
    for input in [
        json!(0),
        json!(42),
        json!(i64::MIN),
        json!(42.5),
        json!(null),
        json!(true),
        json!({"cents": "42"}),
        json!(["42"]),
    ] {
        assert!(
            serde_json::from_value::<Money>(input.clone()).is_err(),
            "{input}"
        );
    }
}

#[test]
fn money_typescript_contract_is_string() {
    assert_eq!(
        <Money as ts_rs::TS>::inline(&ts_rs::Config::default()),
        "string"
    );
}

#[test]
fn export_bindings_money_wire() {
    let config = ts_rs::Config::from_env();
    <Money as ts_rs::TS>::export(&config).unwrap();
    <engine::session::PositionSnap as ts_rs::TS>::export(&config).unwrap();
    <engine::experience::HoldingEpoch as ts_rs::TS>::export(&config).unwrap();
}

#[test]
fn institutional_fee_money_has_string_types_and_nonnull_exact_roundtrip() {
    use engine::experience::{ExperienceMoment, HoldingEpoch};
    let type_text = <HoldingEpoch as ts_rs::TS>::inline(&ts_rs::Config::default());
    assert!(
        type_text.contains("institutional_fees_paid: string | null"),
        "{type_text}"
    );
    let epoch = HoldingEpoch {
        entry_moment: ExperienceMoment {
            civil_date: engine::CivilDate::from_ymd(2030, 1, 2).unwrap(),
            market_minute: 0,
            trading_day: 0,
        },
        last_own_observation: None,
        institutional_fees_paid: Some(Money::from_cents(i64::MAX)),
    };
    let value = serde_json::to_value(&epoch).unwrap();
    assert_eq!(value["institutional_fees_paid"], i64::MAX.to_string());
    assert_eq!(
        serde_json::from_value::<HoldingEpoch>(value.clone()).unwrap(),
        epoch
    );
    let mut numeric = value;
    numeric["institutional_fees_paid"] = json!(42);
    assert!(serde_json::from_value::<HoldingEpoch>(numeric).is_err());
}

#[test]
fn position_cents_json_uses_the_same_money_wire_contract() {
    use engine::account::Position;
    use engine::session::PositionSnap;
    let snap = PositionSnap {
        qty: 100,
        t1_locked: 0,
        invested_cents: i64::MAX,
        recovered_cents: i64::MIN,
    };
    let value = serde_json::to_value(&snap).unwrap();
    assert_eq!(value["invested_cents"], json!(i64::MAX.to_string()));
    assert_eq!(value["recovered_cents"], json!(i64::MIN.to_string()));
    let restored: PositionSnap = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored.invested_cents, i64::MAX);
    assert_eq!(restored.recovered_cents, i64::MIN);
    let position = Position::from_restored_parts(100, 0, i64::MAX, i64::MIN);
    assert_eq!(serde_json::to_value(&position).unwrap(), value);
    let restored: Position = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(restored.invested_cents(), i64::MAX);
    assert_eq!(restored.recovered_cents(), i64::MIN);
    for field in ["invested_cents", "recovered_cents"] {
        let mut numeric = value.clone();
        numeric[field] = json!(42);
        assert!(serde_json::from_value::<PositionSnap>(numeric.clone()).is_err());
        assert!(serde_json::from_value::<Position>(numeric).is_err());
    }
}

#[test]
fn belief_debug_summary_preserves_full_i64_cents_as_strings() {
    let summary = engine::session::BeliefDebugSummary {
        method: None,
        per_share_pessimistic_cents: i64::MIN,
        per_share_optimistic_cents: i64::MAX,
        confidence_bp: 10000,
        unavailable_reason: None,
    };
    let value = serde_json::to_value(summary).unwrap();
    assert_eq!(
        value["per_share_pessimistic_cents"],
        json!(i64::MIN.to_string())
    );
    assert_eq!(
        value["per_share_optimistic_cents"],
        json!(i64::MAX.to_string())
    );
}

// 验证 lib.rs 的 re-export：调用方可直接 use engine::Money
#[test]
fn money_reexported_from_crate_root() {
    use engine::Money;
    assert_eq!(Money::ZERO.cents(), 0);
}

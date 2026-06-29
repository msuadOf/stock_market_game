//! engine money 模块集成测试（TDD 红绿循环）。
use engine::money::MoneyError;
use serde_json::json;

#[test]
fn money_error_variants_construct_and_display() {
    let e1 = MoneyError::ParseFailed { input: "12.345".to_string(), reason: "too many digits".to_string() };
    assert!(e1.to_string().contains("12.345"));

    let e2 = MoneyError::Overflow { op: "add", operand: "i64 max".to_string() };
    assert!(e2.to_string().contains("add"));

    let e3 = MoneyError::InvalidRate { rate: f64::NAN };
    assert!(e3.to_string().contains("NaN"));
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
    assert_eq!(Money::from_cents(10).sub(Money::from_cents(30))?.cents(), -20);
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
    assert!(matches!(err, MoneyError::Overflow { op: "mul_shares", .. }));
}

#[test]
fn money_from_yuan_str_valid() -> Result<(), MoneyError> {
    assert_eq!(Money::from_yuan_str("12.34")?.cents(), 1234);
    assert_eq!(Money::from_yuan_str("-0.01")?.cents(), -1);
    assert_eq!(Money::from_yuan_str("0.1")?.cents(), 10);
    assert_eq!(Money::from_yuan_str("100")?.cents(), 10000);
    assert_eq!(Money::from_yuan_str("12.")?.cents(), 1200);
    assert_eq!(Money::from_yuan_str(".5")?.cents(), 50);
    assert_eq!(Money::from_yuan_str("+3.50")?.cents(), 350);
    Ok(())
}

#[test]
fn money_from_yuan_str_invalid() {
    assert!(matches!(Money::from_yuan_str("12.345"), Err(MoneyError::ParseFailed { .. }))); // 超过 2 位
    assert!(matches!(Money::from_yuan_str(""), Err(MoneyError::ParseFailed { .. })));         // 空
    assert!(matches!(Money::from_yuan_str("abc"), Err(MoneyError::ParseFailed { .. })));      // 非数字
    assert!(matches!(Money::from_yuan_str("1.2.3"), Err(MoneyError::ParseFailed { .. })));    // 多点
    assert!(matches!(Money::from_yuan_str("--1"), Err(MoneyError::ParseFailed { .. })));      // 多负号
    assert!(matches!(Money::from_yuan_str("12.3a"), Err(MoneyError::ParseFailed { .. })));    // 尾部非数字
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
    assert_eq!(Money::from_cents(1_000_000).apply_rate(0.00025)?.cents(), 250);
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
fn apply_rate_zero_rate() -> Result<(), MoneyError> {
    assert_eq!(Money::from_cents(12345).apply_rate(0.0)?.cents(), 0);
    Ok(())
}

#[test]
fn money_serde_roundtrip_preserves_cents() -> Result<(), Box<dyn std::error::Error>> {
    for cents in [0i64, 1, -1, 1234, 9_999_999, -5555] {
        let m = Money::from_cents(cents);
        let j = serde_json::to_value(m)?;
        assert_eq!(j, json!(cents), "serialize cents {cents}");
        let back: Money = serde_json::from_value(j)?;
        assert_eq!(back.cents(), cents, "deserialize cents {cents}");
    }
    Ok(())
}

#[test]
fn money_serde_is_bare_integer() -> Result<(), Box<dyn std::error::Error>> {
    // 前端拿到的应是裸整数，不是对象 {"cents": ...}
    let j = serde_json::to_value(Money::from_cents(42))?;
    assert_eq!(j, json!(42));
    assert!(j.as_i64() == Some(42)); // 不是 object
    Ok(())
}

// 验证 lib.rs 的 re-export：调用方可直接 use engine::Money
#[test]
fn money_reexported_from_crate_root() {
    use engine::Money;
    assert_eq!(Money::ZERO.cents(), 0);
}

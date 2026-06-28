//! engine money 模块集成测试（TDD 红绿循环）。
use engine::money::MoneyError;

#[test]
fn money_error_variants_construct_and_display() {
    let e1 = MoneyError::ParseFailed { input: "12.345".to_string(), reason: "too many digits".to_string() };
    assert_eq!(e1.to_string().contains("12.345"), true);

    let e2 = MoneyError::Overflow { op: "add", operand: "i64 max".to_string() };
    assert_eq!(e2.to_string().contains("add"), true);

    let e3 = MoneyError::InvalidRate { rate: f64::NAN };
    assert_eq!(e3.to_string().contains("NaN"), true);
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

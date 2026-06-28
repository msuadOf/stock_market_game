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

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

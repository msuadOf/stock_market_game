//! engine config 模块集成测试（TDD 红绿循环）。
//!
//! 本文件先于实现编写，目的是驱动 `ConfigError` 错误类型的诞生（spec §6 / plan T1）。
//! 当前阶段只覆盖四变体构造 + Display；GameConfig 校验/方法在后续 T2-T6 补齐。
use engine::config::ConfigError;
use engine::money::Money;

#[test]
fn config_error_invalid_rate_constructs_and_displays() {
    let e = ConfigError::InvalidRate {
        field: "commission_rate",
        rate: -0.1,
        reason: "negative".to_string(),
    };
    let s = e.to_string();
    assert!(s.contains("commission_rate"), "missing field: {s}");
    assert!(s.contains("-0.1"), "missing rate value: {s}");
    assert!(s.contains("negative"), "missing reason: {s}");
}

#[test]
fn config_error_invalid_limit_constructs_and_displays() {
    let e = ConfigError::InvalidLimit {
        field: "default_limit",
        limit: 0.0,
    };
    let s = e.to_string();
    assert!(s.contains("default_limit"), "missing field: {s}");
    assert!(s.contains("0"), "missing limit value: {s}");
}

#[test]
fn config_error_invalid_lot_size_constructs_and_displays() {
    let e = ConfigError::InvalidLotSize(0);
    let s = e.to_string();
    assert!(s.contains("0"), "missing lot_size value: {s}");
}

#[test]
fn config_error_invalid_cash_constructs_and_displays() {
    let e = ConfigError::InvalidCash(Money::from_cents(-1));
    let s = e.to_string();
    assert!(s.contains("-1"), "missing cash cents value: {s}");
}

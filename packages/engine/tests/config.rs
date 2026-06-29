//! engine config 模块集成测试（TDD 红绿循环）。
//!
//! 本文件先于实现编写，目的是驱动 `ConfigError` 错误类型的诞生（spec §6 / plan T1）。
//! 当前阶段只覆盖四变体构造 + Display；GameConfig 校验/方法在后续 T2-T6 补齐。
use engine::config::ConfigError;
use engine::money::Money;
use engine::GameConfig;

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

// T2：GameConfig serde 往返保真（spec §6 测试矩阵第 2 条）。
// 刻意用字面量直接构造（字段须 pub），不依赖 T3 才实现的 new()。

/// 构造一份合法的 GameConfig 实例，供 round-trip 测试复用。
fn sample_config() -> GameConfig {
    GameConfig {
        commission_rate: 0.00025,
        commission_min: Money::from_cents(500),
        stamp_tax_rate: 0.0005,
        default_limit: 0.10,
        st_limit: 0.05,
        lot_size: 100,
        starting_cash: Money::from_cents(10_000_000),
    }
}

#[test]
fn gameconfig_serde_roundtrip_preserves_all_fields() {
    let original = sample_config();
    let json = serde_json::to_string(&original).expect("serialize must succeed");
    let back: GameConfig = serde_json::from_str(&json).expect("deserialize must succeed");

    // f64 比率字段：bit-for-bit 相等（这些值均可精确表示）。
    assert!(
        (back.commission_rate - original.commission_rate).abs() == 0.0,
        "commission_rate changed: {} vs {}",
        back.commission_rate,
        original.commission_rate
    );
    assert!(
        (back.stamp_tax_rate - original.stamp_tax_rate).abs() == 0.0,
        "stamp_tax_rate changed"
    );
    assert!((back.default_limit - original.default_limit).abs() == 0.0, "default_limit changed");
    assert!((back.st_limit - original.st_limit).abs() == 0.0, "st_limit changed");

    // u32 字段：精确相等。
    assert_eq!(back.lot_size, original.lot_size, "lot_size changed");

    // Money 字段：定点，精确相等。
    assert_eq!(back.commission_min, original.commission_min, "commission_min changed");
    assert_eq!(back.starting_cash, original.starting_cash, "starting_cash changed");
}

#[test]
fn gameconfig_money_fields_serialize_as_bare_i64() {
    let cfg = sample_config();
    let json = serde_json::to_string(&cfg).expect("serialize must succeed");
    let value: serde_json::Value = serde_json::from_str(&json).expect("json must parse");

    // commission_min == 500 分 → 裸整数 500（而非 {"cents":500} 之类对象）。
    let min = value
        .get("commission_min")
        .expect("commission_min key present");
    assert_eq!(min.as_i64(), Some(500), "commission_min must be bare i64: {min}");

    // starting_cash == 10_000_000 分 → 裸整数。
    let cash = value
        .get("starting_cash")
        .expect("starting_cash key present");
    assert_eq!(
        cash.as_i64(),
        Some(10_000_000),
        "starting_cash must be bare i64: {cash}"
    );
}

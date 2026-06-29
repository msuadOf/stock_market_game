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

// T3：GameConfig::new(...) 构造即校验（spec §6 测试矩阵第 3 条 / plan T3）。
//
// 全合法字段 → Ok；任一非法字段 → 对应变体 Err。错误必须 fail loud，绝不静默 fallback。

#[test]
fn new_all_valid_returns_ok() {
    let cfg = GameConfig::new(
        0.00025,
        Money::from_cents(500),
        0.0005,
        0.10,
        0.05,
        100,
        Money::from_cents(10_000_000),
    )
    .expect("all-valid config must construct");
    assert_eq!(cfg.commission_rate, 0.00025);
    assert_eq!(cfg.lot_size, 100);
}

#[test]
fn new_commission_rate_negative_rejected() {
    let err = GameConfig::new(
        -0.1,
        Money::from_cents(500),
        0.0005,
        0.10,
        0.05,
        100,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidRate { field: "commission_rate", .. }),
        "expected InvalidRate commission_rate, got {err:?}"
    );
}

#[test]
fn new_commission_rate_nan_rejected() {
    let err = GameConfig::new(
        f64::NAN,
        Money::from_cents(500),
        0.0005,
        0.10,
        0.05,
        100,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidRate { field: "commission_rate", .. }),
        "expected InvalidRate commission_rate, got {err:?}"
    );
}

#[test]
fn new_commission_rate_positive_inf_rejected() {
    let err = GameConfig::new(
        f64::INFINITY,
        Money::from_cents(500),
        0.0005,
        0.10,
        0.05,
        100,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidRate { field: "commission_rate", .. }),
        "expected InvalidRate commission_rate, got {err:?}"
    );
}

#[test]
fn new_stamp_tax_rate_negative_rejected() {
    let err = GameConfig::new(
        0.00025,
        Money::from_cents(500),
        -0.001,
        0.10,
        0.05,
        100,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidRate { field: "stamp_tax_rate", .. }),
        "expected InvalidRate stamp_tax_rate, got {err:?}"
    );
}

#[test]
fn new_stamp_tax_rate_nan_rejected() {
    let err = GameConfig::new(
        0.00025,
        Money::from_cents(500),
        f64::NAN,
        0.10,
        0.05,
        100,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidRate { field: "stamp_tax_rate", .. }),
        "expected InvalidRate stamp_tax_rate, got {err:?}"
    );
}

#[test]
fn new_default_limit_zero_rejected() {
    let err = GameConfig::new(
        0.00025,
        Money::from_cents(500),
        0.0005,
        0.0,
        0.05,
        100,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidLimit { field: "default_limit", .. }),
        "expected InvalidLimit default_limit, got {err:?}"
    );
}

#[test]
fn new_default_limit_one_rejected() {
    let err = GameConfig::new(
        0.00025,
        Money::from_cents(500),
        0.0005,
        1.0,
        0.05,
        100,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidLimit { field: "default_limit", .. }),
        "expected InvalidLimit default_limit, got {err:?}"
    );
}

#[test]
fn new_default_limit_negative_rejected() {
    let err = GameConfig::new(
        0.00025,
        Money::from_cents(500),
        0.0005,
        -0.1,
        0.05,
        100,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidLimit { field: "default_limit", .. }),
        "expected InvalidLimit default_limit, got {err:?}"
    );
}

#[test]
fn new_default_limit_above_one_rejected() {
    let err = GameConfig::new(
        0.00025,
        Money::from_cents(500),
        0.0005,
        1.5,
        0.05,
        100,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidLimit { field: "default_limit", .. }),
        "expected InvalidLimit default_limit, got {err:?}"
    );
}

#[test]
fn new_st_limit_zero_rejected() {
    let err = GameConfig::new(
        0.00025,
        Money::from_cents(500),
        0.0005,
        0.10,
        0.0,
        100,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidLimit { field: "st_limit", .. }),
        "expected InvalidLimit st_limit, got {err:?}"
    );
}

#[test]
fn new_st_limit_one_rejected() {
    let err = GameConfig::new(
        0.00025,
        Money::from_cents(500),
        0.0005,
        0.10,
        1.0,
        100,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidLimit { field: "st_limit", .. }),
        "expected InvalidLimit st_limit, got {err:?}"
    );
}

#[test]
fn new_lot_size_zero_rejected() {
    let err = GameConfig::new(
        0.00025,
        Money::from_cents(500),
        0.0005,
        0.10,
        0.05,
        0,
        Money::from_cents(10_000_000),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidLotSize(0)),
        "expected InvalidLotSize(0), got {err:?}"
    );
}

#[test]
fn new_negative_starting_cash_rejected() {
    let err = GameConfig::new(
        0.00025,
        Money::from_cents(500),
        0.0005,
        0.10,
        0.05,
        100,
        Money::from_cents(-1),
    )
    .unwrap_err();
    assert!(
        matches!(err, ConfigError::InvalidCash(_)),
        "expected InvalidCash, got {err:?}"
    );
}

// T4：GameConfig::proposed_defaults()（spec §6 测试矩阵第 4 条）。
//
// 返回 ref 提议默认值，逐字段断言；两次调用必须相等。这些数值来自参考游戏，
// 标注「待 msuad 确认」，尚未作为硬编码常量散落代码。

#[test]
fn proposed_defaults_returns_ref_proposed_values() {
    let cfg = GameConfig::proposed_defaults();

    // 比率字段（f64）：与 ref 提议值 bit-for-bit 相等（这些值均可精确表示）。
    assert_eq!(cfg.commission_rate, 0.00025, "commission_rate mismatch");
    assert_eq!(cfg.stamp_tax_rate, 0.0005, "stamp_tax_rate mismatch");

    // 涨跌幅（f64）。
    assert_eq!(cfg.default_limit, 0.10, "default_limit mismatch");
    assert_eq!(cfg.st_limit, 0.05, "st_limit mismatch");

    // 一手股数（u32）。
    assert_eq!(cfg.lot_size, 100, "lot_size mismatch");

    // 金额字段（Money，定点 i64 分）：精确相等。
    assert_eq!(
        cfg.commission_min,
        Money::from_cents(500),
        "commission_min mismatch (期望 5.00 元 = 500 分)"
    );
    assert_eq!(
        cfg.starting_cash,
        Money::from_cents(10_000_000),
        "starting_cash mismatch (期望 100000.00 元 = 10_000_000 分)"
    );
}

#[test]
fn proposed_defaults_is_stable_across_calls() {
    // 两次调用应返回相等配置（纯函数，无随机/全局状态）。
    let a = GameConfig::proposed_defaults();
    let b = GameConfig::proposed_defaults();
    assert_eq!(a.commission_rate, b.commission_rate);
    assert_eq!(a.stamp_tax_rate, b.stamp_tax_rate);
    assert_eq!(a.default_limit, b.default_limit);
    assert_eq!(a.st_limit, b.st_limit);
    assert_eq!(a.lot_size, b.lot_size);
    assert_eq!(a.commission_min, b.commission_min);
    assert_eq!(a.starting_cash, b.starting_cash);
}

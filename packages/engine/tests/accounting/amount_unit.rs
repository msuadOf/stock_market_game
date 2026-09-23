//! AccountingAmount 单元金样：checked 算术、整数基点、合同累计余数、半偶舍入、
//! 十进制字符串 serde（含 JS 安全整数以上精确往返）、Money 范围转换、严格解析。

use engine::accounting::{AccountingAmount, AccountingError, FractionUnits};
use engine::money::Money;

#[test]
fn amount_checked_arithmetic_and_i128_overflow_typed() {
    let a = AccountingAmount::from_cents(1_000);
    assert_eq!(
        a.add(AccountingAmount::from_cents(233)).expect("add"),
        AccountingAmount::from_cents(1_233)
    );
    assert_eq!(
        a.sub(AccountingAmount::from_cents(400)).expect("sub"),
        AccountingAmount::from_cents(600)
    );
    assert_eq!(
        a.mul_i128(3).expect("mul"),
        AccountingAmount::from_cents(3_000)
    );
    assert_eq!(a.neg().expect("neg"), AccountingAmount::from_cents(-1_000));

    // i128 溢出必须是类型化错误，绝不下溢环绕。
    let max = AccountingAmount::MAX;
    assert!(matches!(
        max.add(AccountingAmount::from_cents(1)),
        Err(AccountingError::AmountOverflow { .. })
    ));
    assert!(matches!(
        max.mul_i128(2),
        Err(AccountingError::AmountOverflow { .. })
    ));
    assert!(matches!(
        max.apply_basis_points(2),
        Err(AccountingError::AmountOverflow { .. })
    ));
    assert!(matches!(
        AccountingAmount::from_cents(i128::MIN).sub(AccountingAmount::from_cents(1)),
        Err(AccountingError::AmountOverflow { .. })
    ));
}

#[test]
fn amount_round_half_even_ties_pin_behavior() {
    // 15 分 × 1000bp = 1.5 分 → 半偶舍入到 2（1 为奇，进位到偶）
    assert_eq!(
        AccountingAmount::from_cents(15)
            .apply_basis_points(1000)
            .expect("bp"),
        AccountingAmount::from_cents(2)
    );
    // 25 分 × 1000bp = 2.5 分 → 半偶舍入到 2（2 已为偶，不进位）
    assert_eq!(
        AccountingAmount::from_cents(25)
            .apply_basis_points(1000)
            .expect("bp"),
        AccountingAmount::from_cents(2)
    );
    // 负数对称：-25 → -2.5 → -2
    assert_eq!(
        AccountingAmount::from_cents(-25)
            .apply_basis_points(1000)
            .expect("bp"),
        AccountingAmount::from_cents(-2)
    );
    // 常规舍入：1000 分 × 337bp = 33.7 → 34
    assert_eq!(
        AccountingAmount::from_cents(1000)
            .apply_basis_points(337)
            .expect("bp"),
        AccountingAmount::from_cents(34)
    );
}

/// K2：利息小数保留合同累计余数——1000 分 @337bp 跨 3 期，逐期半偶落分，
/// 尾差进余数账户，总分毫未丢（守恒：3×1000×337 = 已付×10000 + 余数）。
#[test]
fn amount_basis_point_accumulation_conserves_exact_total() {
    let principal = AccountingAmount::from_cents(1000);
    let mut carried = FractionUnits::ZERO;
    let mut paid_total = AccountingAmount::ZERO;
    let mut paid_each = Vec::new();
    for _ in 0..3 {
        let (paid, next) = principal
            .apply_basis_points_accum(337, carried)
            .expect("bp accum");
        paid_each.push(paid.cents());
        paid_total = paid_total.add(paid).expect("paid sum");
        carried = next;
    }
    // 逐期：33.7→34（余 -0.3）、33.4→33（余 +0.4）、34.1→34（余 +0.1）
    assert_eq!(paid_each, vec![34, 33, 34]);
    assert_eq!(carried.units(), 1000); // 0.1 分
                                       // 精确守恒：总应计 3 × 1000 × 337 = 1_011_000（单位 1/10000 分）
    let accrued_units = 3_i128 * 1000 * 337;
    let conserved_units = paid_total.cents() * 10_000 + carried.units();
    assert_eq!(accrued_units, conserved_units);
    assert_eq!(conserved_units, 1_011_000);
}

#[test]
fn amount_serde_decimal_string_round_trip_above_js_safe_integer() {
    // JS Number.MAX_SAFE_INTEGER = 2^53-1 = 9_007_199_254_740_991（以「分」计）。
    // 取其 +3（偶数尾数便于两位小数精确表示）：跨 JSON 必须仍是十进制字符串。
    let above_safe = AccountingAmount::from_cents(9_007_199_254_740_993 + 1);
    let json = serde_json::to_value(above_safe).expect("ser");
    assert_eq!(
        json,
        serde_json::Value::String("90071992547409.94".to_string())
    );
    let back: AccountingAmount = serde_json::from_value(json).expect("de");
    assert_eq!(back, above_safe);

    // 负数与小数形态
    for (cents, text) in [
        (-1_i128, "-0.01"),
        (0_i128, "0.00"),
        (123_456_i128, "1234.56"),
        (i128::MAX, "1701411834604692317316873037158841057.27"),
    ] {
        let amt = AccountingAmount::from_cents(cents);
        assert_eq!(
            serde_json::to_value(amt).expect("ser"),
            serde_json::Value::String(text.to_string())
        );
        let back: AccountingAmount =
            serde_json::from_value(serde_json::to_value(amt).expect("ser")).expect("de");
        assert_eq!(back, amt);
    }
}

#[test]
fn amount_money_conversion_range_checked() {
    // i64「分」⊂ i128「分」：from_money 全域无损；to_money 超界必须类型化拒绝。
    let m = Money::from_cents(-12_345);
    let amt = AccountingAmount::from_money(m);
    assert_eq!(amt.cents(), -12_345);
    assert_eq!(amt.to_money().expect("to_money"), m);

    let beyond_i64 = AccountingAmount::from_cents(i64::MAX as i128 + 1);
    assert!(matches!(
        beyond_i64.to_money(),
        Err(AccountingError::MoneyRangeExceeded { .. })
    ));
}

#[test]
fn amount_yuan_string_parse_strict() {
    let ok = |s: &str, cents: i128| {
        assert_eq!(
            AccountingAmount::from_yuan_str(s)
                .expect("parse ok")
                .cents(),
            cents,
            "input {s}"
        );
    };
    ok("12.34", 1234);
    ok("12.3", 1230);
    ok("12", 1200);
    ok("-0.01", -1);
    ok("+7.00", 700);
    ok("0.00", 0);

    for bad in [
        "", "  ", "abc", "1.2.3", "12.345", "1e3", "--5", "5.", ".5.5", "０.１",
    ] {
        assert!(
            AccountingAmount::from_yuan_str(bad).is_err(),
            "input {bad:?} must be rejected"
        );
    }
    // 超出 i128 的天文数字：解析层显式失败而不是环绕
    let huge = "9".repeat(45);
    assert!(matches!(
        AccountingAmount::from_yuan_str(&huge),
        Err(AccountingError::DecimalParse { .. })
    ));
    // Display 与 serde 字符串一致（人读「元」）
    assert_eq!(
        AccountingAmount::from_cents(-123_456).to_string(),
        "-1234.56"
    );
    assert_eq!(
        format!("{:?}", AccountingAmount::from_cents(-123_456)),
        "AccountingAmount(-1234.56)"
    );
}

#[test]
fn fraction_units_serde_integer_string() {
    let f = FractionUnits::from_units(-3000);
    let json = serde_json::to_value(f).expect("ser");
    assert_eq!(json, serde_json::Value::String("-3000".to_string()));
    let back: FractionUnits = serde_json::from_value(json).expect("de");
    assert_eq!(back, f);
    assert!(
        serde_json::from_value::<FractionUnits>(serde_json::Value::String("12.5".to_string()))
            .is_err()
    );
}

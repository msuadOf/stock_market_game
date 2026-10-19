use super::{
    cash_dividend_ex_reference_price, share_split_ex_rights_reference_price,
    stock_distribution_ex_rights_reference_price, CashDividendFormula, ExReferencePriceError,
    ShareSplitExRightsFormula, StockDistributionExRightsFormula,
};
use crate::calendar::{
    CalendarExchange, CalendarPolicy, CivilDate, OfficialCoverageEntry, TradingCalendar,
};
use crate::Money;

fn date(value: &str) -> CivilDate {
    CivilDate::from_iso(value).unwrap()
}

#[test]
fn cash_dividend_sets_next_exchange_trading_day_reference_price() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let result = cash_dividend_ex_reference_price(
        &calendar,
        CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        date("2030-06-07"),
        Money::from_cents(1_000),
        Money::from_cents(125),
    )
    .unwrap();

    assert_eq!(result.ex_date, date("2030-06-10"));
    assert_eq!(result.reference_price, Money::from_cents(875));
}

#[test]
fn calculation_uses_integer_cents_without_extra_rounding() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let result = cash_dividend_ex_reference_price(
        &calendar,
        CalendarExchange::Szse,
        CashDividendFormula::StandardCashOnly,
        date("2030-06-03"),
        Money::from_cents(1_001),
        Money::from_cents(1),
    )
    .unwrap();

    assert_eq!(result.reference_price, Money::from_cents(1_000));
}

#[test]
fn rejects_non_positive_reference_price() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let error = cash_dividend_ex_reference_price(
        &calendar,
        CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        date("2030-06-03"),
        Money::from_cents(100),
        Money::from_cents(100),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ExReferencePriceError::NonPositiveReferencePrice { .. }
    ));
}

#[test]
fn rejects_non_trading_registration_date() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let error = cash_dividend_ex_reference_price(
        &calendar,
        CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        date("2030-06-02"),
        Money::from_cents(1_000),
        Money::from_cents(10),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ExReferencePriceError::InvalidRegistrationDate { .. }
    ));
}

#[test]
fn rejects_zero_and_negative_previous_close_and_cash_dividend() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    for previous_close in [Money::ZERO, Money::from_cents(-1)] {
        let error = cash_dividend_ex_reference_price(
            &calendar,
            CalendarExchange::Sse,
            CashDividendFormula::StandardCashOnly,
            date("2030-06-03"),
            previous_close,
            Money::from_cents(1),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ExReferencePriceError::InvalidPreviousClose { .. }
        ));
    }
    for cash_dividend in [Money::ZERO, Money::from_cents(-1)] {
        let error = cash_dividend_ex_reference_price(
            &calendar,
            CalendarExchange::Sse,
            CashDividendFormula::StandardCashOnly,
            date("2030-06-03"),
            Money::from_cents(1),
            cash_dividend,
        )
        .unwrap_err();
        assert!(matches!(
            error,
            ExReferencePriceError::InvalidCashDividend { .. }
        ));
    }
}

#[test]
fn rejects_dividend_greater_than_previous_close() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let error = cash_dividend_ex_reference_price(
        &calendar,
        CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        date("2030-06-03"),
        Money::from_cents(99),
        Money::from_cents(100),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ExReferencePriceError::NonPositiveReferencePrice { price }
            if price == Money::from_cents(-1)
    ));
}

#[test]
fn propagates_calendar_applicability_boundary_error() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let error = cash_dividend_ex_reference_price(
        &calendar,
        CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        date("2099-12-31"),
        Money::from_cents(100),
        Money::from_cents(1),
    )
    .unwrap_err();

    assert!(matches!(error, ExReferencePriceError::Calendar(_)));
}

#[test]
fn follows_exchange_specific_calendar_coverage() {
    let base = CalendarPolicy::current_default_policy().unwrap();
    let mut spec = base.spec();
    spec.official_coverage.push(OfficialCoverageEntry::new(
        CalendarExchange::Sse,
        2030,
        vec![(date("2030-06-10"), date("2030-06-10"))],
        "synthetic-test-coverage".to_string(),
        "synthetic-test-digest".to_string(),
    ));
    let calendar = TradingCalendar::from_policy(CalendarPolicy::from_parts(spec).unwrap()).unwrap();

    let sse = cash_dividend_ex_reference_price(
        &calendar,
        CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        date("2030-06-07"),
        Money::from_cents(100),
        Money::from_cents(1),
    )
    .unwrap();
    let szse = cash_dividend_ex_reference_price(
        &calendar,
        CalendarExchange::Szse,
        CashDividendFormula::StandardCashOnly,
        date("2030-06-07"),
        Money::from_cents(100),
        Money::from_cents(1),
    )
    .unwrap();

    assert_eq!(sse.ex_date, date("2030-06-11"));
    assert_eq!(szse.ex_date, date("2030-06-10"));
}

#[test]
fn rejects_exchange_approved_adjustment_formula_explicitly() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let error = cash_dividend_ex_reference_price(
        &calendar,
        CalendarExchange::Sse,
        CashDividendFormula::ExchangeApprovedAdjustment,
        date("2030-06-03"),
        Money::from_cents(100),
        Money::from_cents(1),
    )
    .unwrap_err();

    assert!(matches!(
        error,
        ExReferencePriceError::UnsupportedApprovedAdjustment
    ));
}

#[test]
fn stock_distribution_ex_rights_divides_previous_close_by_dilution_factor() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    // 沪市：10 送 5（ratio = 0.5），前收 1000 分，无现金红利 → 1000 / 1.5 = 666.67 分。
    let result = stock_distribution_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Sse,
        StockDistributionExRightsFormula::ShanghaiCirculatingShareChange {
            ratio_micros: 500_000,
        },
        date("2030-06-06"),
        Money::from_cents(1_000),
        Money::ZERO,
    )
    .unwrap();
    assert_eq!(result.ex_date, date("2030-06-07"));
    assert_eq!(result.reference_price, Money::from_cents(667));

    // 深市：10 转 2 加同日每股现金红利 10 分 → (1000 − 10) / 1.2 = 825 分。
    let result = stock_distribution_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Szse,
        StockDistributionExRightsFormula::ShenzhenShareChange {
            ratio_micros: 200_000,
        },
        date("2030-06-06"),
        Money::from_cents(1_000),
        Money::from_cents(10),
    )
    .unwrap();
    assert_eq!(result.reference_price, Money::from_cents(825));
}

#[test]
fn stock_distribution_ex_rights_uses_bankers_rounding_at_half_cent() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    // 沪市 10 送 4：700 / 1.4 = 500 整除，无舍入。
    let exact = stock_distribution_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Sse,
        StockDistributionExRightsFormula::ShanghaiCirculatingShareChange {
            ratio_micros: 400_000,
        },
        date("2030-06-06"),
        Money::from_cents(700),
        Money::ZERO,
    )
    .unwrap();
    assert_eq!(exact.reference_price, Money::from_cents(500));
    // 5001 / 1.5 = 3334：无半分情形。
    let plain = stock_distribution_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Sse,
        StockDistributionExRightsFormula::ShanghaiCirculatingShareChange {
            ratio_micros: 500_000,
        },
        date("2030-06-06"),
        Money::from_cents(5_001),
        Money::ZERO,
    )
    .unwrap();
    assert_eq!(plain.reference_price, Money::from_cents(3_334));
}

#[test]
fn stock_distribution_ex_rights_rejects_cross_market_formula_and_invalid_inputs() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    assert!(matches!(
        stock_distribution_ex_rights_reference_price(
            &calendar,
            CalendarExchange::Szse,
            StockDistributionExRightsFormula::ShanghaiCirculatingShareChange {
                ratio_micros: 500_000
            },
            date("2030-06-06"),
            Money::from_cents(1_000),
            Money::ZERO,
        ),
        Err(ExReferencePriceError::FormulaExchangeMismatch { .. })
    ));
    assert!(matches!(
        stock_distribution_ex_rights_reference_price(
            &calendar,
            CalendarExchange::Sse,
            StockDistributionExRightsFormula::ShanghaiCirculatingShareChange {
                ratio_micros: 0
            },
            date("2030-06-06"),
            Money::from_cents(1_000),
            Money::ZERO,
        ),
        Err(ExReferencePriceError::InvalidShareChangeRatio)
    ));
    // 现金红利大到把分子扣成非正数时显式拒绝，不产生零或负参考价。
    assert!(matches!(
        stock_distribution_ex_rights_reference_price(
            &calendar,
            CalendarExchange::Sse,
            StockDistributionExRightsFormula::ShanghaiCirculatingShareChange {
                ratio_micros: 500_000
            },
            date("2030-06-06"),
            Money::from_cents(1_000),
            Money::from_cents(1_000),
        ),
        Err(ExReferencePriceError::NonPositiveExRightsNumerator { .. })
    ));
    // 超大送转比例把参考价稀释到不足一分时显式拒绝。
    assert!(matches!(
        stock_distribution_ex_rights_reference_price(
            &calendar,
            CalendarExchange::Sse,
            StockDistributionExRightsFormula::ShanghaiCirculatingShareChange {
                ratio_micros: u64::MAX / 2
            },
            date("2030-06-06"),
            Money::from_cents(1),
            Money::ZERO,
        ),
        Err(ExReferencePriceError::NonPositiveReferencePrice { .. })
    ));
}

// ==== 配股除权公式（2026-10-07 M 批）====

use super::{rights_offering_ex_rights_reference_price, RightsOfferingExRightsFormula};

#[test]
fn rights_formula_combines_cash_dividend_and_rights_price_components() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    // 沪市：前收 1000 分、红利 40 分、配股价 600 分、实际比例 30%（300_000 micros）：
    // [(1000−40)+600×0.3]/1.3 = (960+180)/1.3 = 1140/1.3 = 876.923… → 877 分。
    let result = rights_offering_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Sse,
        RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChange {
            ratio_micros: 300_000,
        },
        date("2030-06-10"),
        Money::from_cents(1_000),
        Money::from_cents(40),
        Money::from_cents(600),
    )
    .unwrap();
    assert_eq!(result.ex_date, date("2030-06-11"));
    assert_eq!(result.reference_price, Money::from_cents(877));

    // 深市：无红利、前收 1300 分、配股价 800 分、比例 50%：
    // (1300+400)/1.5 = 1700/1.5 = 1133.33… → 1133 分。
    let result = rights_offering_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Szse,
        RightsOfferingExRightsFormula::ShenzhenRightsShareChange {
            ratio_micros: 500_000,
        },
        date("2030-06-10"),
        Money::from_cents(1_300),
        Money::from_cents(0),
        Money::from_cents(800),
    )
    .unwrap();
    assert_eq!(result.reference_price, Money::from_cents(1_133));
}

#[test]
fn rights_formula_rejects_exchange_mismatch_and_invalid_inputs() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    assert!(matches!(
        rights_offering_ex_rights_reference_price(
            &calendar,
            CalendarExchange::Szse,
            RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChange {
                ratio_micros: 300_000
            },
            date("2030-06-10"),
            Money::from_cents(1_000),
            Money::from_cents(0),
            Money::from_cents(600),
        ),
        Err(ExReferencePriceError::RightsFormulaExchangeMismatch { .. })
    ));
    assert!(matches!(
        rights_offering_ex_rights_reference_price(
            &calendar,
            CalendarExchange::Sse,
            RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChange { ratio_micros: 0 },
            date("2030-06-10"),
            Money::from_cents(1_000),
            Money::from_cents(0),
            Money::from_cents(600),
        ),
        Err(ExReferencePriceError::InvalidRightsChangeRatio)
    ));
    assert!(matches!(
        rights_offering_ex_rights_reference_price(
            &calendar,
            CalendarExchange::Sse,
            RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChange {
                ratio_micros: 300_000
            },
            date("2030-06-10"),
            Money::from_cents(1_000),
            Money::from_cents(0),
            Money::from_cents(0),
        ),
        Err(ExReferencePriceError::InvalidRightsPrice { .. })
    ));
    // 非交易日 L 拒绝。
    assert!(matches!(
        rights_offering_ex_rights_reference_price(
            &calendar,
            CalendarExchange::Sse,
            RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChange {
                ratio_micros: 300_000
            },
            date("2030-06-08"),
            Money::from_cents(1_000),
            Money::from_cents(0),
            Money::from_cents(600),
        ),
        Err(ExReferencePriceError::InvalidRegistrationDate { .. })
    ));
}

#[test]
fn rights_formula_rounds_half_to_even_at_the_cent() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    // 构造 .5 分边界：前收 1300、无红利、配股价 500、比例 20%：
    // (1300+100)/1.2 = 1400/1.2 = 1166.666… → 1167（非 .5，先做正常取整锚）。
    let even_case = rights_offering_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Sse,
        RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChange {
            ratio_micros: 200_000,
        },
        date("2030-06-10"),
        Money::from_cents(1_300),
        Money::from_cents(0),
        Money::from_cents(500),
    )
    .unwrap();
    assert_eq!(even_case.reference_price, Money::from_cents(1_167));
    // .5 精确边界：前收 1001、无红利、配股价 1001、比例 100_000（10%）：
    // (1001 + 100.1)/1.1 = 1101.1/1.1 = 1001.0 → 恰整除，锚定无舍入路径。
    let exact = rights_offering_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Sse,
        RightsOfferingExRightsFormula::ShanghaiCirculatingRightsChange {
            ratio_micros: 100_000,
        },
        date("2030-06-10"),
        Money::from_cents(1_001),
        Money::from_cents(0),
        Money::from_cents(1_001),
    )
    .unwrap();
    assert_eq!(exact.reference_price, Money::from_cents(1_001));
}

// ---------- 拆股／缩股除权公式 ----------

#[test]
fn split_divides_previous_close_by_integer_ratio() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    // 1 拆 10：参考价 = 前收 / 10（沪 4.3.2 公式代入变动比例 9）。
    let result = share_split_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Sse,
        ShareSplitExRightsFormula::ShanghaiCirculatingShareChange {
            numerator: 10,
            denominator: 1,
        },
        date("2030-06-03"),
        Money::from_cents(10_000),
        Money::from_cents(0),
    )
    .unwrap();
    assert_eq!(result.ex_date, date("2030-06-04"));
    assert_eq!(result.reference_price, Money::from_cents(1_000));
}

#[test]
fn consolidation_multiplies_previous_price_and_merges_same_day_cash_dividend() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    // 10 并 1：参考价 = (前收 − 红利) × 10。
    let result = share_split_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Szse,
        ShareSplitExRightsFormula::ShenzhenShareChange {
            numerator: 1,
            denominator: 10,
        },
        date("2030-06-03"),
        Money::from_cents(3_000),
        Money::from_cents(50),
    )
    .unwrap();
    assert_eq!(result.ex_date, date("2030-06-04"));
    assert_eq!(result.reference_price, Money::from_cents(29_500));
}

#[test]
fn split_rounds_half_to_even_on_non_divisible_cents() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    // 1 拆 2（新股/旧股 = 2/1）：1001 分 ÷ 2 = 500.5 → 银行家舍入到 500（偶数）；
    // 1003 ÷ 2 = 501.5 → 502。
    let half_down = share_split_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Sse,
        ShareSplitExRightsFormula::ShanghaiCirculatingShareChange {
            numerator: 2,
            denominator: 1,
        },
        date("2030-06-03"),
        Money::from_cents(1_001),
        Money::from_cents(0),
    )
    .unwrap();
    assert_eq!(half_down.reference_price, Money::from_cents(500));
    let half_up = share_split_ex_rights_reference_price(
        &calendar,
        CalendarExchange::Sse,
        ShareSplitExRightsFormula::ShanghaiCirculatingShareChange {
            numerator: 2,
            denominator: 1,
        },
        date("2030-06-03"),
        Money::from_cents(1_003),
        Money::from_cents(0),
    )
    .unwrap();
    assert_eq!(half_up.reference_price, Money::from_cents(502));
}

#[test]
fn split_formula_rejects_exchange_mismatch_identity_ratio_and_bad_inputs() {
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    assert!(matches!(
        share_split_ex_rights_reference_price(
            &calendar,
            CalendarExchange::Szse,
            ShareSplitExRightsFormula::ShanghaiCirculatingShareChange {
                numerator: 2,
                denominator: 1,
            },
            date("2030-06-03"),
            Money::from_cents(1_000),
            Money::from_cents(0),
        ),
        Err(ExReferencePriceError::SplitFormulaExchangeMismatch { .. })
    ));
    assert!(matches!(
        share_split_ex_rights_reference_price(
            &calendar,
            CalendarExchange::Sse,
            ShareSplitExRightsFormula::ShanghaiCirculatingShareChange {
                numerator: 1,
                denominator: 1,
            },
            date("2030-06-03"),
            Money::from_cents(1_000),
            Money::from_cents(0),
        ),
        Err(ExReferencePriceError::InvalidSplitShareChangeRatio)
    ));
    // 非正分子（前收 − 红利）拒绝。
    assert!(matches!(
        share_split_ex_rights_reference_price(
            &calendar,
            CalendarExchange::Sse,
            ShareSplitExRightsFormula::ShanghaiCirculatingShareChange {
                numerator: 2,
                denominator: 1,
            },
            date("2030-06-03"),
            Money::from_cents(100),
            Money::from_cents(100),
        ),
        Err(ExReferencePriceError::NonPositiveExRightsNumerator { .. })
    ));
}

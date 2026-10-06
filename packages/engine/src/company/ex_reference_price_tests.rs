use super::{
    cash_dividend_ex_reference_price, stock_distribution_ex_rights_reference_price,
    CashDividendFormula, ExReferencePriceError, StockDistributionExRightsFormula,
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

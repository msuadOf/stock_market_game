use crate::calendar::{CalendarError, CalendarExchange, CivilDate, TradingCalendar};
use crate::{Money, MoneyError};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ExReferencePrice {
    pub ex_date: CivilDate,
    pub reference_price: Money,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum CashDividendFormula {
    StandardCashOnly,
    ExchangeApprovedAdjustment,
}

#[derive(Debug, Error)]
pub enum ExReferencePriceError {
    #[error("查询现金分红除息日历失败：{0}")]
    Calendar(#[from] CalendarError),
    #[error("权益登记日 {date} 不是 {exchange:?} 交易日")]
    InvalidRegistrationDate {
        exchange: CalendarExchange,
        date: CivilDate,
    },
    #[error("除息计算的前收盘价必须为正数，实际为 {price:?}")]
    InvalidPreviousClose { price: Money },
    #[error("税前每股现金红利必须为正数，实际为 {amount:?}")]
    InvalidCashDividend { amount: Money },
    #[error("前收盘价减税前每股现金红利后参考价必须为正数，实际为 {price:?}")]
    NonPositiveReferencePrice { price: Money },
    #[error("除息参考价计算溢出：{0}")]
    Arithmetic(#[from] MoneyError),
    #[error("已获交易所批准的调整公式不受纯现金基础公式模块支持")]
    UnsupportedApprovedAdjustment,
}

/// 计算未发生交易所批准特殊调整时的纯现金除息参考价。
///
/// 此基础 API 不接收、也不代表已获交易所批准的调整公式；调用方必须先排除该类事件，
/// 不得将结果直接用作调整事件的市场参考价。
pub fn cash_dividend_ex_reference_price(
    calendar: &TradingCalendar,
    exchange: CalendarExchange,
    formula: CashDividendFormula,
    registration_date: CivilDate,
    previous_close: Money,
    pre_tax_cash_per_share: Money,
) -> Result<ExReferencePrice, ExReferencePriceError> {
    if formula == CashDividendFormula::ExchangeApprovedAdjustment {
        return Err(ExReferencePriceError::UnsupportedApprovedAdjustment);
    }
    if previous_close <= Money::ZERO {
        return Err(ExReferencePriceError::InvalidPreviousClose {
            price: previous_close,
        });
    }
    if pre_tax_cash_per_share <= Money::ZERO {
        return Err(ExReferencePriceError::InvalidCashDividend {
            amount: pre_tax_cash_per_share,
        });
    }
    if !calendar.is_trading_day(exchange, registration_date)? {
        return Err(ExReferencePriceError::InvalidRegistrationDate {
            exchange,
            date: registration_date,
        });
    }

    let ex_date = calendar.next_trading_day(exchange, registration_date)?;
    let reference_price = previous_close.sub(pre_tax_cash_per_share)?;
    if reference_price <= Money::ZERO {
        return Err(ExReferencePriceError::NonPositiveReferencePrice {
            price: reference_price,
        });
    }

    Ok(ExReferencePrice {
        ex_date,
        reference_price,
    })
}

#[cfg(test)]
#[path = "ex_reference_price_tests.rs"]
mod tests;

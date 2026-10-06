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

/// 送转除权公式按上市地交易所分列参数化，两市措辞口径不同，禁止合并成一个
/// 不分市场的公式。
///
/// - 沪市《交易规则（2026 年修订）》4.3.1：`[(前收盘价格−现金红利)+配股价格×流通股份变动比例]/(1+流通股份变动比例)`。
/// - 深市《交易规则（2026 年修订）》4.4.1：`[(前收盘价−现金红利)+配股价格×股份变动比例]/(1+股份变动比例)`。
///
/// 本批只实现送转分量（配股未实现，配股价格分量恒为零）：参考价 =
/// (前收盘价 − 同日合计税前每股现金红利) / (1 + 变动比例)。变动比例使用
/// 百万分之一股的整数单位，与 `company::stock_distribution` 的比例口径一致。
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum StockDistributionExRightsFormula {
    /// 沪市口径：流通股份变动比例；只能用于 `CalendarExchange::Sse` 证券。
    ShanghaiCirculatingShareChange {
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        #[ts(type = "string")]
        ratio_micros: u64,
    },
    /// 深市口径：股份变动比例；只能用于 `CalendarExchange::Szse` 证券。
    ShenzhenShareChange {
        #[serde(with = "crate::orderbook::canonical_u64_decimal")]
        #[ts(type = "string")]
        ratio_micros: u64,
    },
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
    #[error("送转除权前收盘价减同日现金红利后必须为正数，实际为 {cents} 分")]
    NonPositiveExRightsNumerator { cents: i128 },
    #[error("送转除权公式与交易所不匹配：{formula:?} 不能用于 {exchange:?} 证券")]
    FormulaExchangeMismatch {
        formula: StockDistributionExRightsFormula,
        exchange: CalendarExchange,
    },
    #[error("送转除权比例必须为正数（百万分之一股整数单位）")]
    InvalidShareChangeRatio,
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

/// 计算送转（股票股利／资本公积转增）与可选同日现金红利合并的标准除权参考价。
///
/// 公式按上市地交易所分列（沪市"流通股份变动比例"、深市"股份变动比例"），公式变体
/// 与 `exchange` 不匹配时显式拒绝。除权日为权益登记日次一交易日，与新股入账（R+1）
/// 为同一交易日。除权参考价是行情前收盘与涨跌幅基准，不覆写历史成交。
///
/// 除法结果按最小变动价位取整数分：本实现使用与 `Money` 既有一致的银行家舍入
/// （half-to-even）。沪深规则条文只给出公式，未在已核验摘录中写明分位舍入方向；
/// 该舍入选择登记为游戏简化，取得官方分位舍入口径后再行校正。
pub fn stock_distribution_ex_rights_reference_price(
    calendar: &TradingCalendar,
    exchange: CalendarExchange,
    formula: StockDistributionExRightsFormula,
    registration_date: CivilDate,
    previous_close: Money,
    cash_dividend_per_share: Money,
) -> Result<ExReferencePrice, ExReferencePriceError> {
    let ratio_micros = match formula {
        StockDistributionExRightsFormula::ShanghaiCirculatingShareChange { ratio_micros } => {
            if exchange != CalendarExchange::Sse {
                return Err(ExReferencePriceError::FormulaExchangeMismatch { formula, exchange });
            }
            ratio_micros
        }
        StockDistributionExRightsFormula::ShenzhenShareChange { ratio_micros } => {
            if exchange != CalendarExchange::Szse {
                return Err(ExReferencePriceError::FormulaExchangeMismatch { formula, exchange });
            }
            ratio_micros
        }
    };
    if ratio_micros == 0 {
        return Err(ExReferencePriceError::InvalidShareChangeRatio);
    }
    if previous_close <= Money::ZERO {
        return Err(ExReferencePriceError::InvalidPreviousClose {
            price: previous_close,
        });
    }
    if cash_dividend_per_share < Money::ZERO {
        return Err(ExReferencePriceError::InvalidCashDividend {
            amount: cash_dividend_per_share,
        });
    }
    if !calendar.is_trading_day(exchange, registration_date)? {
        return Err(ExReferencePriceError::InvalidRegistrationDate {
            exchange,
            date: registration_date,
        });
    }

    let ex_date = calendar.next_trading_day(exchange, registration_date)?;
    let numerator_cents =
        i128::from(previous_close.cents()) - i128::from(cash_dividend_per_share.cents());
    if numerator_cents <= 0 {
        return Err(ExReferencePriceError::NonPositiveExRightsNumerator {
            cents: numerator_cents,
        });
    }
    // 参考价 = numerator / (1 + ratio_micros / 1_000_000)
    //        = numerator × 1_000_000 / (1_000_000 + ratio_micros)。
    let scaled = numerator_cents.checked_mul(1_000_000).ok_or(
        ExReferencePriceError::NonPositiveExRightsNumerator {
            cents: numerator_cents,
        },
    )?;
    let denominator = 1_000_000_i128 + i128::from(ratio_micros);
    let quotient = scaled / denominator;
    let remainder = scaled % denominator;
    let doubled_remainder = remainder
        .checked_mul(2)
        .ok_or(ExReferencePriceError::NonPositiveExRightsNumerator {
            cents: numerator_cents,
        })?;
    let reference_cents = match doubled_remainder.cmp(&denominator) {
        std::cmp::Ordering::Less => quotient,
        std::cmp::Ordering::Greater => quotient + 1,
        std::cmp::Ordering::Equal if quotient % 2 == 0 => quotient,
        std::cmp::Ordering::Equal => quotient + 1,
    };
    let reference_cents = i64::try_from(reference_cents).map_err(|_| MoneyError::Overflow {
        op: "ex_rights_reference_cents",
        operand: reference_cents.to_string(),
    })?;
    let reference_price = Money::from_cents(reference_cents);
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

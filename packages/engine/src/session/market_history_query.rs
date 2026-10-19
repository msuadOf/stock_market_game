//! B2 件 5：市场历史查询面（保留历史分页/当日分钟K/日内均价）。
//!
//! 对照 18725d07 `market_history_page`/`current_minute_history`/
//! `query_intraday_average` 语义基线重立：
//! - **availability 五态**：Traded/NoTrades/Closed/BeforeStart/NotEnded
//!   （零成交开市=NoTrades；休市=Closed；查询早于开局=BeforeStart；
//!   自然日未结束（超出 `settled_through`）=NotEnded）；
//! - **分页游标**：`after` 排他下界（读己之写快照）；`next_cursor`=
//!   末条日期（还有剩余时）；`settled_through`=civil clock 权威；
//! - **副作用**：`query_market_history_for`/`query_current_minute_history_for`
//!   记 `history_reads`（账户+股+市场分钟）；纯查询 `market_history_page`/
//!   `current_minute_history` 不记；个人域查询不记（旧契约）；
//! - 数据源=session 镜像（retained 链+当日分钟桶——驱动器每拍/日结
//!   安装的发布投影，与发布版本同源同拍一致）。

use super::{GameSession, SessionError};
use crate::calendar::CivilDate;
use crate::runtime::MinuteBarFact;
use crate::stock::StockId;
use serde::{Deserialize, Serialize};

/// 保留历史分页请求（对照旧 `MarketHistoryRequest`——`after` 必须显式
/// null 或日期，缺失=拒绝）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct MarketHistoryRequest {
    pub stock: StockId,
    pub date_from: CivilDate,
    pub date_to: CivilDate,
    #[serde(deserialize_with = "required_nullable")]
    pub after: Option<CivilDate>,
    pub page_size: u32,
}

/// 一日条目的可提供性（五态——对照旧 `MarketHistoryAvailability`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "PascalCase")]
#[ts(export)]
pub enum MarketHistoryAvailability {
    /// 开市且有真实成交。
    Traded,
    /// 开市但零成交（不伪造 bar）。
    NoTrades,
    /// 休市日。
    Closed,
    /// 早于本局开始日期。
    BeforeStart,
    /// 自然日尚未结束（超出 settled_through）。
    NotEnded,
}

/// 一日条目。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct MarketHistoryEntry {
    pub date: CivilDate,
    pub availability: MarketHistoryAvailability,
    pub bars: Vec<MinuteBarFact>,
    #[serde(deserialize_with = "required_nullable")]
    pub daily_candle: Option<crate::session::DailyCandle>,
}

/// 保留历史分页结果。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct MarketHistoryPage {
    pub stock: StockId,
    pub entries: Vec<MarketHistoryEntry>,
    #[serde(deserialize_with = "required_nullable")]
    pub next_cursor: Option<CivilDate>,
    #[serde(deserialize_with = "required_nullable")]
    pub settled_through: Option<CivilDate>,
}

/// 当日分钟K请求（对照旧 `CurrentMinuteHistoryRequest`）。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct CurrentMinuteHistoryRequest {
    pub stock: StockId,
}

/// 当日观察相位（对照旧 `CurrentMinuteHistoryPhase`）。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[serde(rename_all = "PascalCase")]
#[ts(export)]
pub enum CurrentMinuteHistoryPhase {
    CallAuction,
    PreOpen,
    Continuous,
    ClosingAuction,
    AfterClose,
    Closed,
}

/// 当日分钟K响应。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct CurrentMinuteHistoryResponse {
    pub stock: StockId,
    pub date: CivilDate,
    #[ts(type = "true")]
    pub live: bool,
    pub trading: bool,
    pub phase: CurrentMinuteHistoryPhase,
    pub bars: Vec<MinuteBarFact>,
}

fn required_nullable<'de, Value: Deserialize<'de>, Deserializer: serde::Deserializer<'de>>(
    deserializer: Deserializer,
) -> Result<Option<Value>, Deserializer::Error> {
    Option::<Value>::deserialize(deserializer)
}

/// 1261=市场历史查询参数不合法（数值错误纪律：文案不入错误体——
/// ADR-0041 词表；诊断语义由调用点注释锚定）。
fn invalid(_detail: impl Into<String>) -> SessionError {
    SessionError::InvalidSave(crate::error::ValidationFault::bare(1261))
}

impl GameSession {
    /// 保留历史分页（纯查询——无副作用；对照旧 `market_history_page`）。
    pub fn market_history_page(
        &self,
        request: &MarketHistoryRequest,
    ) -> Result<MarketHistoryPage, SessionError> {
        self.require_healthy()?;
        if !self.state.markets.contains_key(&request.stock) {
            return Err(SessionError::UnknownHistoryStock(request.stock));
        }
        if request.date_from > request.date_to
            || request.page_size == 0
            || request
                .after
                .is_some_and(|date| date < request.date_from || date > request.date_to)
        {
            return Err(invalid("历史日期范围、排他游标或 page_size 不合法"));
        }
        let mut cursor = match request.after {
            Some(date) if date == request.date_to => None,
            Some(date) => Some(
                date.next()
                    .map_err(|e| invalid(format!("日期推进失败：{e}")))?,
            ),
            None => Some(request.date_from),
        };
        let settled = self.state.civil_clock.settled_through();
        let mut entries = Vec::new();
        while let Some(date) = cursor {
            if date > request.date_to || entries.len() >= request.page_size as usize {
                break;
            }
            let candle_time = crate::session::candles::candle_date_time(date);
            let daily_candle = self
                .state
                .candle_book
                .histories()
                .get(&request.stock)
                .and_then(|history| {
                    history
                        .iter_rev()
                        .find(|candle| candle.time == candle_time)
                        .cloned()
                });
            let (availability, bars) = if date < self.state.setup.start_date {
                (MarketHistoryAvailability::BeforeStart, Vec::new())
            } else if settled.is_none_or(|settled| date > settled) {
                (MarketHistoryAvailability::NotEnded, Vec::new())
            } else {
                let index = usize::try_from(date.days_since(self.state.setup.start_date))
                    .map_err(|_| invalid("日期索引溢出"))?;
                let day = self
                    .state
                    .retained_market_days
                    .get(index)
                    .filter(|day| day.0.date == date)
                    .ok_or_else(|| invalid("已结束自然日缺失保留历史"))?;
                let session = day
                    .0
                    .securities
                    .get(&request.stock.0)
                    .ok_or_else(|| invalid("已结束自然日缺证券条目"))?;
                (
                    if !session.trading {
                        MarketHistoryAvailability::Closed
                    } else if session.bars.is_empty() {
                        MarketHistoryAvailability::NoTrades
                    } else {
                        MarketHistoryAvailability::Traded
                    },
                    session.bars.as_ref().clone(),
                )
            };
            entries.push(MarketHistoryEntry {
                date,
                availability,
                bars,
                daily_candle: if availability == MarketHistoryAvailability::NotEnded {
                    None
                } else {
                    daily_candle
                },
            });
            cursor = if date == request.date_to {
                None
            } else {
                Some(
                    date.next()
                        .map_err(|e| invalid(format!("日期推进失败：{e}")))?,
                )
            };
        }
        Ok(MarketHistoryPage {
            stock: request.stock,
            next_cursor: cursor.and_then(|_| entries.last().map(|entry| entry.date)),
            entries,
            settled_through: settled,
        })
    }

    /// 带读记录的市场历史查询（对照旧 `query_market_history_for`——
    /// 副作用=history_reads 恰记请求账户一股）。
    pub fn query_market_history_for(
        &mut self,
        account: crate::AccountId,
        request: &MarketHistoryRequest,
    ) -> Result<MarketHistoryPage, SessionError> {
        if !self.state.accounts.contains_key(&account) {
            return Err(SessionError::UnknownHistoryAccount(account));
        }
        let page = self.market_history_page(request)?;
        self.record_market_history_read(account, &request.stock)?;
        Ok(page)
    }

    /// 当日分钟K（纯查询——对照旧 `current_minute_history`）。
    pub fn current_minute_history(
        &self,
        request: &CurrentMinuteHistoryRequest,
    ) -> Result<CurrentMinuteHistoryResponse, SessionError> {
        self.require_healthy()?;
        if !self.state.markets.contains_key(&request.stock) {
            return Err(SessionError::UnknownHistoryStock(request.stock));
        }
        let date = self.civil_date();
        let trading = matches!(
            self.runtime_stock_trading_on(request.stock.0, date)
                .map_err(|e| invalid(format!("开市判定失败：{e:?}")))?,
            true
        );
        // 收盘后：当日步数已满（tick 为 tpd 倍数）且日K已入历史。
        let after_close = trading
            && self.state.tick.is_multiple_of(self.state.setup.ticks_per_day)
            && self
                .state
                .candle_book
                .histories()
                .get(&request.stock)
                .is_some_and(|history| {
                    history
                        .iter_rev()
                        .next()
                        .is_some_and(|candle| {
                            candle.time
                                == crate::session::candles::candle_date_time(date)
                        })
                });
        let phase = if !trading {
            CurrentMinuteHistoryPhase::Closed
        } else if after_close {
            CurrentMinuteHistoryPhase::AfterClose
        } else {
            match self.phase() {
                crate::TradingPhase::CallAuction => CurrentMinuteHistoryPhase::CallAuction,
                crate::TradingPhase::PreOpen => CurrentMinuteHistoryPhase::PreOpen,
                crate::TradingPhase::Continuous => CurrentMinuteHistoryPhase::Continuous,
                crate::TradingPhase::ClosingAuction => {
                    CurrentMinuteHistoryPhase::ClosingAuction
                }
            }
        };
        let bars = if trading {
            self.state
                .market_minute_bars
                .get(&request.stock)
                .cloned()
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        Ok(CurrentMinuteHistoryResponse {
            stock: request.stock,
            date,
            live: true,
            trading,
            phase,
            bars,
        })
    }

    /// 带读记录的当日分钟K查询（对照旧 `query_current_minute_history_for`）。
    pub fn query_current_minute_history_for(
        &mut self,
        account: crate::AccountId,
        request: &CurrentMinuteHistoryRequest,
    ) -> Result<CurrentMinuteHistoryResponse, SessionError> {
        if !self.state.accounts.contains_key(&account) {
            return Err(SessionError::UnknownHistoryAccount(account));
        }
        let response = self.current_minute_history(request)?;
        self.record_market_history_read(account, &request.stock)?;
        Ok(response)
    }

    /// 日内均价（对照旧 `query_intraday_average`——活动 K 的量价统计）。
    pub fn query_intraday_average(
        &self,
        stock: &StockId,
    ) -> Result<Option<crate::intraday_average::IntradayAverage>, SessionError> {
        if !self.state.markets.contains_key(stock) {
            return Err(SessionError::UnknownHistoryStock(*stock));
        }
        let active = self.state.candle_book.active().get(stock);
        crate::intraday_average::calculate_intraday_average(
            active.and_then(|value| value.trade_stats.as_ref()),
            active.map_or(0, |value| value.volume),
        )
        .map_err(|e| invalid(format!("日内均价计算失败：{e:?}")))
    }

    /// history_reads 记录（两 query_for 面共用——失败不静默）。
    fn record_market_history_read(
        &mut self,
        account: crate::AccountId,
        stock: &StockId,
    ) -> Result<(), SessionError> {
        let minute = self.current_market_minute();
        self.state
            .history_reads
            .get_mut(&account)
            .ok_or(SessionError::UnknownHistoryAccount(account))?
            .record(stock, minute)
            // 1260：个人历史读记录失败（与 query_stock_history 同码同义）。
            .map_err(|_| {
                SessionError::InvalidSave(crate::error::ValidationFault::bare(1260))
            })?;
        Ok(())
    }
}

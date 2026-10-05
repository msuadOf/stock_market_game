use super::{CivilClock, DailyCandle, Event, GameSession, SessionError, StockSpec, TradingPhase};
use crate::experience::AppendOnlyHistory;
use crate::{CivilDate, CivilInstant, DayStatus, Money, StockCode};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::Arc;
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum HistoryTradingPhase {
    OpenAuction,
    Continuous,
    ClosingAuction,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct MinuteBar {
    pub minute_of_day: u16,
    pub phase: HistoryTradingPhase,
    pub open: Money,
    pub high: Money,
    pub low: Money,
    pub close: Money,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    #[ts(type = "string")]
    pub volume_shares: u64,
    #[serde(with = "crate::orderbook::canonical_u128_decimal")]
    #[ts(type = "string")]
    pub turnover_cents: u128,
    #[serde(with = "crate::orderbook::canonical_u64_decimal")]
    #[ts(type = "string")]
    pub trade_count: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum HistorySessionStatus {
    Trading,
    Closed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct MinuteHistorySession {
    pub status: HistorySessionStatus,
    pub bars: Vec<MinuteBar>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct RetainedHistoryDay {
    pub date: CivilDate,
    pub securities: BTreeMap<StockCode, MinuteHistorySession>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct MarketHistoryRequest {
    pub code: StockCode,
    pub date_from: CivilDate,
    pub date_to: CivilDate,
    #[serde(deserialize_with = "required_nullable")]
    pub after: Option<CivilDate>,
    pub page_size: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[ts(export)]
pub enum MarketHistoryAvailability {
    Traded,
    NoTrades,
    Closed,
    BeforeStart,
    NotEnded,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct MarketHistoryEntry {
    pub date: CivilDate,
    pub availability: MarketHistoryAvailability,
    pub bars: Vec<MinuteBar>,
    #[serde(deserialize_with = "required_nullable")]
    pub daily_candle: Option<DailyCandle>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct MarketHistoryPage {
    pub code: StockCode,
    pub entries: Vec<MarketHistoryEntry>,
    #[serde(deserialize_with = "required_nullable")]
    pub next_cursor: Option<CivilDate>,
    #[serde(deserialize_with = "required_nullable")]
    pub settled_through: Option<CivilDate>,
}

fn required_nullable<'de, Value: Deserialize<'de>, Deserializer: serde::Deserializer<'de>>(
    deserializer: Deserializer,
) -> Result<Option<Value>, Deserializer::Error> {
    Option::<Value>::deserialize(deserializer)
}

#[derive(Clone, Debug, Default)]
pub(super) struct RetainedMarketHistory {
    days: AppendOnlyHistory<SharedHistoryDay>,
    active: BTreeMap<StockCode, BTreeMap<(u16, HistoryTradingPhase), MinuteBar>>,
}

impl Serialize for RetainedMarketHistory {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("RetainedMarketHistory", 2)?;
        state.serialize_field("days", &self.days)?;
        state.serialize_field("active", &self.saved_active())?;
        state.end()
    }
}

#[derive(Clone, Debug)]
struct SharedHistoryDay(Arc<RetainedHistoryDay>);

impl Serialize for SharedHistoryDay {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.as_ref().serialize(serializer)
    }
}

fn invalid(message: impl Into<String>) -> SessionError {
    SessionError::InvalidSave(format!("retained_market_history: {}", message.into()))
}

impl RetainedMarketHistory {
    fn record(
        &mut self,
        instant: CivilInstant,
        phase: TradingPhase,
        events: &[Event],
    ) -> Result<(), SessionError> {
        let completions = events
            .iter()
            .filter_map(|event| match event {
                Event::AuctionCompleted { code, phase, .. } => Some((code.clone(), *phase)),
                _ => None,
            })
            .collect::<BTreeMap<_, _>>();
        let mut changed = BTreeMap::new();
        for event in events {
            let Event::Trade {
                code, price, qty, ..
            } = event
            else {
                continue;
            };
            if price.cents() <= 0 || *qty == 0 {
                return Err(invalid("真实成交的价格与股数必须为正"));
            }
            let (minute, history_phase) = match completions.get(code) {
                Some(TradingPhase::CallAuction) => (565, HistoryTradingPhase::OpenAuction),
                Some(TradingPhase::ClosingAuction) => (900, HistoryTradingPhase::ClosingAuction),
                Some(_) => return Err(invalid("竞价完成事实包含非竞价阶段")),
                None if phase == TradingPhase::Continuous => (
                    u16::try_from(instant.second_of_day() / 60)
                        .map_err(|error| invalid(error.to_string()))?,
                    HistoryTradingPhase::Continuous,
                ),
                None => return Err(invalid("竞价成交缺少真实完成事实，或盘前产生了成交")),
            };
            let bars = changed
                .entry(code.clone())
                .or_insert_with(|| self.active.get(code).cloned().unwrap_or_default());
            let turnover = u128::try_from(price.cents())
                .map_err(|error| invalid(error.to_string()))?
                .checked_mul(u128::from(*qty))
                .ok_or_else(|| invalid("单笔成交额溢出u128"))?;
            let bar = bars.entry((minute, history_phase)).or_insert(MinuteBar {
                minute_of_day: minute,
                phase: history_phase,
                open: *price,
                high: *price,
                low: *price,
                close: *price,
                volume_shares: 0,
                turnover_cents: 0,
                trade_count: 0,
            });
            bar.high = bar.high.max(*price);
            bar.low = bar.low.min(*price);
            bar.close = *price;
            bar.volume_shares = bar
                .volume_shares
                .checked_add(u64::from(*qty))
                .ok_or_else(|| invalid("分钟成交股数溢出u64"))?;
            bar.turnover_cents = bar
                .turnover_cents
                .checked_add(turnover)
                .ok_or_else(|| invalid("分钟成交额溢出u128"))?;
            bar.trade_count = bar
                .trade_count
                .checked_add(1)
                .ok_or_else(|| invalid("分钟成交笔数溢出u64"))?;
            validate_bar(bar)?;
        }
        self.active.extend(changed);
        Ok(())
    }

    pub(super) fn saved_days(&self) -> Vec<RetainedHistoryDay> {
        self.days.iter().map(|day| day.0.as_ref().clone()).collect()
    }

    pub(super) fn saved_active(&self) -> BTreeMap<StockCode, Vec<MinuteBar>> {
        self.active
            .iter()
            .map(|(code, bars)| (code.clone(), bars.values().cloned().collect()))
            .collect()
    }

    pub(super) fn current_bars(&self, code: &StockCode) -> Vec<MinuteBar> {
        self.active.get(code).map(|bars| bars.values().cloned().collect()).unwrap_or_default()
    }

    pub(super) fn restore_active(&mut self, active: &BTreeMap<StockCode, Vec<MinuteBar>>) {
        self.active = active
            .iter()
            .map(|(code, bars)| {
                (
                    code.clone(),
                    bars.iter()
                        .map(|bar| ((bar.minute_of_day, bar.phase), bar.clone()))
                        .collect(),
                )
            })
            .collect();
    }

    pub(super) fn restore_days(days: &[RetainedHistoryDay]) -> Self {
        let mut result = Self::default();
        for day in days {
            result.days.push(SharedHistoryDay(Arc::new(day.clone())));
        }
        result
    }

    fn finish(
        &mut self,
        date: CivilDate,
        clock: &CivilClock,
        setup: &super::SessionSetup,
        candles: &BTreeMap<StockCode, DailyCandle>,
    ) -> Result<(), SessionError> {
        if self
            .days
            .last()
            .is_some_and(|last| last.0.date.next().ok() != Some(date))
        {
            return Err(invalid("日终历史重复或跳日"));
        }
        let mut securities = BTreeMap::new();
        for stock in &setup.stocks {
            let is_open = matches!(
                clock
                    .exchange_day_status(super::session_calendar_exchange(stock.exchange), date)?,
                DayStatus::Trading
            );
            let bars = self
                .active
                .get(&stock.code)
                .map(|bars| bars.values().cloned().collect::<Vec<_>>())
                .unwrap_or_default();
            validate_stock_bars(stock, setup, &bars)?;
            if is_open {
                let candle = candles
                    .get(&stock.code)
                    .ok_or_else(|| invalid(format!("开市日{}缺少真实日K", stock.code.0)))?;
                validate_bars_against_candle(&bars, candle)?;
            } else if !bars.is_empty() || candles.contains_key(&stock.code) {
                return Err(invalid("休市证券出现分钟或当日日K"));
            }
            securities.insert(
                stock.code.clone(),
                MinuteHistorySession {
                    status: if is_open {
                        HistorySessionStatus::Trading
                    } else {
                        HistorySessionStatus::Closed
                    },
                    bars,
                },
            );
        }
        if self
            .active
            .keys()
            .any(|code| !securities.contains_key(code))
        {
            return Err(invalid("分钟事实含未知证券"));
        }
        self.days
            .push(SharedHistoryDay(Arc::new(RetainedHistoryDay {
                date,
                securities,
            })));
        self.active.clear();
        Ok(())
    }
}

fn validate_bar(bar: &MinuteBar) -> Result<(), SessionError> {
    let valid_time = match bar.phase {
        HistoryTradingPhase::OpenAuction => bar.minute_of_day == 565,
        HistoryTradingPhase::ClosingAuction => bar.minute_of_day == 900,
        HistoryTradingPhase::Continuous => {
            (570..690).contains(&bar.minute_of_day) || (780..900).contains(&bar.minute_of_day)
        }
    };
    if !valid_time
        || bar.low.cents() <= 0
        || bar.low > bar.open
        || bar.low > bar.close
        || bar.high < bar.open
        || bar.high < bar.close
        || bar.low > bar.high
        || bar.volume_shares == 0
        || bar.trade_count == 0
        || bar.trade_count > bar.volume_shares
    {
        return Err(invalid("分钟OHLC、真实成交股数、笔数或交易时段不合法"));
    }
    let minimum = u128::try_from(bar.low.cents()).map_err(|error| invalid(error.to_string()))?
        * u128::from(bar.volume_shares);
    let maximum = u128::try_from(bar.high.cents()).map_err(|error| invalid(error.to_string()))?
        * u128::from(bar.volume_shares);
    if bar.turnover_cents < minimum || bar.turnover_cents > maximum {
        return Err(invalid("分钟成交额不在真实价格与股数约束范围内"));
    }
    Ok(())
}

fn validate_bars(bars: &[MinuteBar]) -> Result<(), SessionError> {
    for bar in bars {
        validate_bar(bar)?;
    }
    if bars.windows(2).any(|pair| {
        (pair[0].minute_of_day, pair[0].phase) >= (pair[1].minute_of_day, pair[1].phase)
    }) {
        return Err(invalid("分钟事实乱序或重复"));
    }
    Ok(())
}

fn validate_stock_bars(
    stock: &StockSpec,
    setup: &super::SessionSetup,
    bars: &[MinuteBar],
) -> Result<(), SessionError> {
    validate_bars(bars)?;
    if stock.tick.cents() <= 0 {
        return Err(invalid(format!(
            "证券{}的价格最小变动单位必须为正",
            stock.code.0
        )));
    }
    for bar in bars {
        if [bar.open, bar.high, bar.low, bar.close]
            .iter()
            .any(|price| price.cents() % stock.tick.cents() != 0)
        {
            return Err(invalid(format!(
                "证券{}的分钟价格不符合本股最小变动单位",
                stock.code.0
            )));
        }
        let enabled = match bar.phase {
            HistoryTradingPhase::OpenAuction => setup.auction_ticks > 0,
            HistoryTradingPhase::ClosingAuction => setup.closing_auction_ticks > 0,
            HistoryTradingPhase::Continuous => {
                setup.closing_auction_ticks == 0 || bar.minute_of_day < 897
            }
        };
        if !enabled {
            return Err(invalid(format!(
                "证券{}的分钟阶段或时刻与本局竞价配置不一致",
                stock.code.0
            )));
        }
    }
    Ok(())
}

fn validate_bars_against_candle(
    bars: &[MinuteBar],
    candle: &DailyCandle,
) -> Result<(), SessionError> {
    validate_bars(bars)?;
    let stats = candle
        .trade_stats
        .as_ref()
        .ok_or_else(|| invalid("实时历史缺少真实成交统计"))?;
    let (volume, turnover, count) =
        bars.iter()
            .try_fold((0_u64, 0_u128, 0_u64), |(volume, turnover, count), bar| {
                Ok::<_, SessionError>((
                    volume
                        .checked_add(bar.volume_shares)
                        .ok_or_else(|| invalid("日累计股数溢出"))?,
                    turnover
                        .checked_add(bar.turnover_cents)
                        .ok_or_else(|| invalid("日累计金额溢出"))?,
                    count
                        .checked_add(bar.trade_count)
                        .ok_or_else(|| invalid("日累计笔数溢出"))?,
                ))
            })?;
    if volume != candle.volume
        || turnover != u128::from(stats.turnover_cents)
        || count != stats.trade_count
    {
        return Err(invalid("分钟累计量价与真实日K统计不一致"));
    }
    if let (Some(first), Some(last)) = (bars.first(), bars.last()) {
        if first.open != candle.open
            || last.close != candle.close
            || bars.iter().map(|bar| bar.high).max() != Some(candle.high)
            || bars.iter().map(|bar| bar.low).min() != Some(candle.low)
        {
            return Err(invalid("分钟OHLC与真实日K不一致"));
        }
    }
    Ok(())
}

impl GameSession {
    pub(super) fn record_retained_trades(
        &mut self,
        instant: CivilInstant,
        phase: TradingPhase,
        events: &[Event],
    ) -> Result<(), SessionError> {
        for event in events {
            if let Event::Trade { code, .. } = event {
                if !matches!(self.stock_day_status(code)?, DayStatus::Trading) {
                    return Err(invalid("休市证券不能产生分钟成交"));
                }
            }
        }
        self.state
            .retained_market_history
            .record(instant, phase, events)
    }

    pub(super) fn finish_retained_history(&mut self, date: CivilDate) -> Result<(), SessionError> {
        let time = super::candles::candle_date_time(date);
        let candles = self
            .state
            .candle_book
            .histories()
            .iter()
            .filter_map(|(code, history)| {
                history
                    .iter_rev()
                    .next()
                    .filter(|candle| candle.time == time)
                    .map(|candle| (code.clone(), candle.clone()))
            })
            .collect();
        self.state.retained_market_history.finish(
            date,
            &self.state.civil_clock,
            &self.state.setup,
            &candles,
        )
    }

    pub fn market_history_page(
        &self,
        request: &MarketHistoryRequest,
    ) -> Result<MarketHistoryPage, SessionError> {
        self.require_healthy()?;
        if !self.state.markets.contains_key(&request.code) {
            return Err(SessionError::UnknownHistoryStock(request.code.clone()));
        }
        if request.date_from > request.date_to
            || request.page_size == 0
            || request
                .after
                .is_some_and(|date| date < request.date_from || date > request.date_to)
        {
            return Err(invalid("历史日期范围、排他游标或page_size不合法"));
        }
        let mut cursor = match request.after {
            Some(date) if date == request.date_to => None,
            Some(date) => Some(date.next().map_err(|error| invalid(error.to_string()))?),
            None => Some(request.date_from),
        };
        let settled = self.civil_clock().settled_through();
        let mut entries = Vec::new();
        while let Some(date) = cursor {
            if date > request.date_to || entries.len() >= request.page_size as usize {
                break;
            }
            let daily_candle = self.state.candle_book.histories()[&request.code]
                .iter_rev()
                .find(|candle| candle.time == super::candles::candle_date_time(date))
                .cloned();
            let (availability, bars) = if date < self.state.setup.start_date {
                (MarketHistoryAvailability::BeforeStart, Vec::new())
            } else if settled.is_none_or(|settled| date > settled) {
                (MarketHistoryAvailability::NotEnded, Vec::new())
            } else {
                let index = usize::try_from(date.days_since(self.state.setup.start_date))
                    .map_err(|error| invalid(error.to_string()))?;
                let day = self
                    .state
                    .retained_market_history
                    .days
                    .get(index)
                    .filter(|day| day.0.date == date)
                    .ok_or_else(|| invalid("已结束自然日缺失历史"))?;
                let session = day
                    .0
                    .securities
                    .get(&request.code)
                    .ok_or_else(|| invalid("已结束自然日缺失证券历史"))?;
                (
                    match session.status {
                        HistorySessionStatus::Closed => MarketHistoryAvailability::Closed,
                        HistorySessionStatus::Trading if session.bars.is_empty() => {
                            MarketHistoryAvailability::NoTrades
                        }
                        HistorySessionStatus::Trading => MarketHistoryAvailability::Traded,
                    },
                    session.bars.clone(),
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
                Some(date.next().map_err(|error| invalid(error.to_string()))?)
            };
        }
        Ok(MarketHistoryPage {
            code: request.code.clone(),
            next_cursor: cursor.and_then(|_| entries.last().map(|entry| entry.date)),
            entries,
            settled_through: settled,
        })
    }

    pub fn query_market_history_for(
        &mut self,
        account: crate::AccountId,
        request: &MarketHistoryRequest,
    ) -> Result<MarketHistoryPage, SessionError> {
        if !self.state.accounts.contains_key(&account) {
            return Err(SessionError::UnknownHistoryAccount(account));
        }
        let page = self.market_history_page(request)?;
        let minute = self.current_market_minute();
        self.state
            .history_reads
            .get_mut(&account)
            .ok_or(SessionError::UnknownHistoryAccount(account))?
            .record(&request.code, minute)
            .map_err(|error| SessionError::InvalidHistoryRead(error.to_string()))?;
        Ok(page)
    }

    pub(super) fn validate_active_minute_history(
        &self,
        active: &BTreeMap<StockCode, Vec<MinuteBar>>,
    ) -> Result<(), SessionError> {
        let time = super::candles::candle_date_time(self.civil_date());
        for (code, bars) in active {
            if !self.state.markets.contains_key(code)
                || bars.is_empty()
                || !matches!(self.stock_day_status(code)?, DayStatus::Trading)
            {
                return Err(invalid(
                    "活动分钟历史必须属于实际开市的已知证券，不能包含空数组",
                ));
            }
            let stock = self
                .state
                .setup
                .stocks
                .iter()
                .find(|stock| &stock.code == code)
                .ok_or_else(|| invalid("活动分钟事实缺少证券规格"))?;
            validate_stock_bars(stock, &self.state.setup, bars)?;
            let now = self.observation_civil_instant().second_of_day() / 60;
            if self.state.tick == 0
                || bars
                    .last()
                    .is_some_and(|bar| u32::from(bar.minute_of_day) > now)
            {
                return Err(invalid("活动分钟事实超出实际执行时间"));
            }
        }
        for (code, history) in self.state.candle_book.histories() {
            let candle = self.state.candle_book.active().get(code).or_else(|| {
                history
                    .iter_rev()
                    .next()
                    .filter(|candle| candle.time == time)
            });
            if let Some(candle) = candle {
                validate_bars_against_candle(
                    active.get(code).map(Vec::as_slice).unwrap_or(&[]),
                    candle,
                )?;
            } else if active.contains_key(code) {
                return Err(invalid("活动分钟历史缺少当日成交统计"));
            }
        }
        Ok(())
    }
}

pub(super) fn validate_saved_history(save: &super::SaveSlot) -> Result<(), SessionError> {
    let clock = CivilClock::from_parts_for_stocks(
        save.setup.start_date,
        &save.civil_clock,
        save.setup.stocks.iter().map(|stock| {
            (
                stock.code.clone(),
                super::session_calendar_exchange(stock.exchange),
            )
        }),
    )?;
    let expected_len = clock
        .settled_through()
        .map_or(0, |date| date.days_since(save.setup.start_date) + 1);
    if i64::try_from(save.retained_market_history.len()).ok() != Some(expected_len) {
        return Err(invalid("完整日终历史长度不覆盖所有已结束自然日"));
    }
    let mut expected = save.setup.start_date;
    for day in &save.retained_market_history {
        if day.date != expected || day.securities.len() != save.setup.stocks.len() {
            return Err(invalid("日终历史日期或证券集合不完整"));
        }
        for stock in &save.setup.stocks {
            let session = day
                .securities
                .get(&stock.code)
                .ok_or_else(|| invalid("日终历史缺少本局证券"))?;
            validate_stock_bars(stock, &save.setup, &session.bars)?;
            let is_open = matches!(
                clock.exchange_day_status(
                    super::session_calendar_exchange(stock.exchange),
                    day.date
                )?,
                DayStatus::Trading
            );
            if is_open != (session.status == HistorySessionStatus::Trading) {
                return Err(invalid("分钟历史开市状态与证券日历不一致"));
            }
            if !is_open {
                if !session.bars.is_empty() {
                    return Err(invalid("休市证券存在分钟事实"));
                }
            } else {
                let candle = save
                    .snapshot
                    .daily_candles
                    .get(&stock.code)
                    .and_then(|candles| {
                        candles.iter().find(|candle| {
                            candle.time == super::candles::candle_date_time(day.date)
                        })
                    })
                    .ok_or_else(|| invalid("日终分钟历史缺少对应真实日K"))?;
                validate_bars_against_candle(&session.bars, candle)?;
            }
        }
        expected = expected
            .next()
            .map_err(|error| invalid(error.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::AccountId;

    fn trade(seq: u64, cents: i64, qty: u32) -> Event {
        Event::Trade {
            seq,
            code: StockCode("600101".into()),
            price: Money::from_cents(cents),
            qty,
            maker: AccountId(1),
            taker: AccountId(2),
        }
    }

    #[test]
    fn real_trades_build_exact_minute_ohlcv_without_private_identity() {
        let mut history = RetainedMarketHistory::default();
        let instant =
            CivilInstant::from_hms(CivilDate::from_iso("2030-01-02").unwrap(), 9, 30, 1).unwrap();
        history
            .record(
                instant,
                TradingPhase::Continuous,
                &[trade(1, 1000, 100), trade(2, 1100, 200), trade(3, 900, 50)],
            )
            .unwrap();
        let bars = history
            .active
            .get(&StockCode("600101".into()))
            .expect("真实成交必须创建分钟事实");
        let bar = &bars[&(570, HistoryTradingPhase::Continuous)];
        assert_eq!(
            (
                bar.open.cents(),
                bar.high.cents(),
                bar.low.cents(),
                bar.close.cents()
            ),
            (1000, 1100, 900, 900)
        );
        assert_eq!(
            (bar.volume_shares, bar.turnover_cents, bar.trade_count),
            (350, 365000, 3)
        );
        let json = serde_json::to_string(bar).unwrap();
        assert!(!json.contains("maker") && !json.contains("taker"));
        assert!(json.contains("\"turnover_cents\":\"365000\""));
    }

    #[test]
    fn invalid_trade_does_not_mutate_existing_minute() {
        let mut history = RetainedMarketHistory::default();
        let instant =
            CivilInstant::from_hms(CivilDate::from_iso("2030-01-02").unwrap(), 9, 30, 1).unwrap();
        history
            .record(instant, TradingPhase::Continuous, &[trade(1, 1000, 100)])
            .unwrap();
        let before = serde_json::to_value(&history).unwrap();
        assert!(history
            .record(
                instant,
                TradingPhase::Continuous,
                &[trade(2, 1100, 100), trade(3, -1, 100)]
            )
            .is_err());
        assert_eq!(serde_json::to_value(&history).unwrap(), before);
    }

    #[test]
    fn legal_trade_turnover_can_exceed_u64_without_rounding() {
        let mut history = RetainedMarketHistory::default();
        let instant =
            CivilInstant::from_hms(CivilDate::from_iso("2030-01-02").unwrap(), 9, 30, 1).unwrap();
        let events = (1..=4)
            .map(|seq| trade(seq, 5_000_000_000_000_000_000, 1))
            .collect::<Vec<_>>();
        history
            .record(instant, TradingPhase::Continuous, &events)
            .unwrap();
        let bar =
            &history.active[&StockCode("600101".into())][&(570, HistoryTradingPhase::Continuous)];
        assert_eq!(bar.turnover_cents, 20_000_000_000_000_000_000_u128);
        assert_eq!(bar.volume_shares, 4);
        let value = serde_json::to_value(bar).unwrap();
        assert_eq!(value["turnover_cents"], "20000000000000000000");
        assert_eq!(serde_json::from_value::<MinuteBar>(value).unwrap(), *bar);
    }

    #[test]
    fn auction_completion_uses_actual_phase_time_not_indicative_volume() {
        let mut history = RetainedMarketHistory::default();
        let date = CivilDate::from_iso("2030-01-02").unwrap();
        let code = StockCode("600101".into());
        let event = Event::AuctionCompleted {
            seq: 2,
            tick: 1,
            phase: TradingPhase::CallAuction,
            code: code.clone(),
            clearing_price: Some(Money::from_cents(1000)),
            matched_volume: 100,
        };
        history
            .record(
                CivilInstant::from_hms(date, 9, 19, 59).unwrap(),
                TradingPhase::CallAuction,
                &[trade(1, 1000, 100), event],
            )
            .unwrap();
        assert!(history.active[&code].contains_key(&(565, HistoryTradingPhase::OpenAuction)));
        let indication = Event::AuctionTick {
            seq: 3,
            tick: 2,
            phase: TradingPhase::ClosingAuction,
            code: code.clone(),
            indicative_price: Some(Money::from_cents(1000)),
            matched_volume: 100,
            imbalance: 0,
        };
        history
            .record(
                CivilInstant::from_hms(date, 14, 59, 0).unwrap(),
                TradingPhase::ClosingAuction,
                &[indication],
            )
            .unwrap();
        assert_eq!(history.active[&code].len(), 1);
        let completion = Event::AuctionCompleted {
            seq: 5,
            tick: 3,
            phase: TradingPhase::ClosingAuction,
            code: code.clone(),
            clearing_price: Some(Money::from_cents(1100)),
            matched_volume: 200,
        };
        history
            .record(
                CivilInstant::from_hms(date, 14, 59, 59).unwrap(),
                TradingPhase::ClosingAuction,
                &[trade(4, 1100, 200), completion],
            )
            .unwrap();
        assert!(history.active[&code].contains_key(&(900, HistoryTradingPhase::ClosingAuction)));
        assert_eq!(history.active[&code].len(), 2);
    }

    #[test]
    fn minute_wire_rejects_numeric_and_noncanonical_counters() {
        let valid = serde_json::json!({
            "minute_of_day":570,"phase":"Continuous","open":"1000","high":"1000","low":"1000","close":"1000",
            "volume_shares":"100","turnover_cents":"100000","trade_count":"1"
        });
        serde_json::from_value::<MinuteBar>(valid.clone()).unwrap();
        for field in ["volume_shares", "turnover_cents", "trade_count"] {
            for invalid in [
                serde_json::json!(1),
                serde_json::json!("01"),
                serde_json::json!("+1"),
                serde_json::json!("-1"),
                serde_json::json!(""),
                serde_json::json!("1.0"),
            ] {
                let mut value = valid.clone();
                value[field] = invalid;
                assert!(
                    serde_json::from_value::<MinuteBar>(value).is_err(),
                    "{field} 的金额或股数不能接受非规范字符串"
                );
            }
        }
        let mut extra = valid.clone();
        extra["maker"] = serde_json::json!("1");
        assert!(serde_json::from_value::<MinuteBar>(extra).is_err());
        let mut missing = valid;
        missing.as_object_mut().unwrap().remove("turnover_cents");
        assert!(serde_json::from_value::<MinuteBar>(missing).is_err());
    }

    fn quiet_session() -> GameSession {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.start_date = CivilDate::from_iso("2030-01-02").unwrap();
        setup.ticks_per_day = 3;
        setup.stocks[0].float_shares = 1_000;
        let mut session = GameSession::new(setup, 71).unwrap();
        let tick = 1_000_000;
        let accounts = session
            .state
            .npc_attention
            .keys()
            .copied()
            .collect::<Vec<_>>();
        for account in accounts {
            session
                .state
                .npc_attention
                .get_mut(&account)
                .unwrap()
                .next_attention_candidate_tick = tick;
        }
        session.state.attention_scheduler = session
            .state
            .npc_attention
            .keys()
            .map(|account| (tick, *account))
            .collect();
        session.state.pending_npc = Some(super::super::PendingNpcBatch {
            observed_tick: 0,
            observed_accounts: Vec::new(),
            intents: Vec::new(),
            dependencies: Vec::new(),
        });
        session
    }

    fn prepare_buy(session: &mut GameSession) -> StockCode {
        let code = session.state.setup.stocks[0].code.clone();
        assert!(session.state.accounts[&AccountId(1)].sellable_qty(&code) >= 100);
        let id = session.state.next_order_id;
        let placed = session
            .state
            .markets
            .get_mut(&code)
            .unwrap()
            .place(crate::Order {
                id: crate::OrderId(id),
                side: crate::Side::Sell,
                price: Money::from_cents(1000),
                qty: 100,
                original_qty: 100,
                filled_qty: 0,
                filled_value: Money::ZERO,
                owner: AccountId(1),
                seq: id,
            })
            .unwrap();
        assert!(placed.trades.is_empty());
        session.state.next_order_id += 1;
        session.hydrate_or_validate_envelope_ledger().unwrap();
        session
            .enqueue_player_intent(
                AccountId(0),
                crate::Intent::PlaceLimit {
                    code: code.clone(),
                    side: crate::Side::Buy,
                    price: crate::LimitPrice::Fixed(Money::from_cents(1000)),
                    qty: 100,
                },
            )
            .unwrap();
        code
    }

    fn execute_buy(session: &mut GameSession) -> StockCode {
        let code = prepare_buy(session);
        let events = session.step().unwrap();
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, Event::Trade { .. }))
                .count(),
            1
        );
        code
    }

    #[test]
    fn actual_execution_checkpoint_and_day_end_restore_keep_exact_history() {
        let mut game = quiet_session();
        let code = execute_buy(&mut game);
        let private = game.save().unwrap();
        assert!(private.retained_market_history.is_empty());
        assert_eq!(private.runtime_state.active_minute_history[&code].len(), 1);
        let mut restored = GameSession::restore(&private).unwrap();
        assert_eq!(
            serde_json::to_value(restored.save().unwrap()).unwrap(),
            serde_json::to_value(&private).unwrap()
        );
        for branch in [&mut game, &mut restored] {
            for _ in 1..3 {
                branch.step().unwrap();
            }
            branch.end_civil_day().unwrap();
            let saved = branch.save().unwrap();
            assert!(saved.runtime_state.active_minute_history.is_empty());
            assert_eq!(saved.retained_market_history.len(), 1);
            assert_eq!(
                saved.retained_market_history[0].securities[&code].bars[0].volume_shares,
                100
            );
            let restored_day = GameSession::restore(&saved).unwrap();
            assert_eq!(
                serde_json::to_value(restored_day.save().unwrap()).unwrap(),
                serde_json::to_value(saved).unwrap()
            );
        }
        assert_eq!(
            serde_json::to_value(game.save().unwrap()).unwrap(),
            serde_json::to_value(restored.save().unwrap()).unwrap()
        );
    }

    #[test]
    fn failed_tick_and_failed_day_end_do_not_publish_history() {
        let mut game = quiet_session();
        let code = execute_buy(&mut game);
        let before = serde_json::to_value(&game.state.retained_market_history).unwrap();
        prepare_buy(&mut game);
        game.inject_post_shadow_failure(super::super::StepFatal::InvariantViolation {
            location: "分钟历史提交前".into(),
            description: "测试失败".into(),
        });
        assert!(game.step().is_err());
        assert_eq!(
            serde_json::to_value(&game.state.retained_market_history).unwrap(),
            before
        );
        let mut fresh = quiet_session();
        execute_buy(&mut fresh);
        assert!(fresh.end_civil_day().is_err());
        assert!(fresh.state.retained_market_history.days.is_empty());
        assert_eq!(fresh.state.retained_market_history.active[&code].len(), 1);
    }

    #[test]
    fn all_dates_remain_available_with_stable_pages_and_distinct_statuses() {
        let mut game = quiet_session();
        let code = game.state.setup.stocks[0].code.clone();
        for _ in 0..8 {
            if game.civil_clock().phase() == super::super::CivilPhase::IntradayTrading {
                for _ in 0..3 {
                    game.step().unwrap();
                }
            }
            game.end_civil_day().unwrap();
        }
        assert_eq!(game.state.retained_market_history.days.len(), 8);
        let start = CivilDate::from_iso("2030-01-01").unwrap();
        let end = CivilDate::from_iso("2030-01-10").unwrap();
        let mut request = MarketHistoryRequest {
            code: code.clone(),
            date_from: start,
            date_to: end,
            after: None,
            page_size: 2,
        };
        let before = game.state.history_reads.to_map();
        let first = game.market_history_page(&request).unwrap();
        assert_eq!(
            first.entries[0].availability,
            MarketHistoryAvailability::BeforeStart
        );
        assert_eq!(
            first.entries[1].availability,
            MarketHistoryAvailability::NoTrades
        );
        assert_eq!(game.state.history_reads.to_map(), before);
        let mut entries = first.entries;
        request.after = first.next_cursor;
        while request.after.is_some() {
            let next = game.market_history_page(&request).unwrap();
            request.after = next.next_cursor;
            entries.extend(next.entries);
        }
        assert_eq!(entries.len(), 10);
        assert!(entries.windows(2).all(|pair| pair[0].date < pair[1].date));
        assert_eq!(entries[4].availability, MarketHistoryAvailability::Closed);
        assert_eq!(entries[9].availability, MarketHistoryAvailability::NotEnded);
        assert!(entries.iter().all(|entry| entry.bars.is_empty()));
        request.after = None;
        game.query_market_history_for(AccountId(0), &request)
            .unwrap();
        assert_eq!(
            game.state.history_reads[&AccountId(0)].stocks[&code].read_count,
            1
        );
        assert!(game.state.history_reads[&AccountId(1)].stocks.is_empty());
        request.page_size = 0;
        assert!(game
            .query_market_history_for(AccountId(0), &request)
            .is_err());
        assert_eq!(
            game.state.history_reads[&AccountId(0)].stocks[&code].read_count,
            1
        );
    }

    #[test]
    fn completed_history_is_shared_and_bad_archive_facts_are_rejected() {
        let mut game = quiet_session();
        let code = execute_buy(&mut game);
        for _ in 1..3 {
            game.step().unwrap();
        }
        game.end_civil_day().unwrap();
        let shadow = game.clone_for_tick_shadow().unwrap();
        assert!(Arc::ptr_eq(
            &game.state.retained_market_history.days[0].0,
            &shadow.state.retained_market_history.days[0].0
        ));
        let saved = game.save().unwrap();
        let mut duplicate = saved.clone();
        duplicate
            .retained_market_history
            .push(duplicate.retained_market_history[0].clone());
        assert!(GameSession::restore(&duplicate).is_err());
        let mut missing = saved.clone();
        missing.retained_market_history.clear();
        assert!(GameSession::restore(&missing).is_err());
        let mut changed = saved.clone();
        changed.retained_market_history[0]
            .securities
            .get_mut(&code)
            .unwrap()
            .bars[0]
            .turnover_cents += 1;
        assert!(GameSession::restore(&changed).is_err());
        let mut leaked = saved;
        leaked.runtime_state.active_minute_history.insert(
            code,
            leaked.retained_market_history[0]
                .securities
                .values()
                .next()
                .unwrap()
                .bars
                .clone(),
        );
        assert!(GameSession::restore(&leaked).is_err());
    }

    #[test]
    fn restore_rejects_auction_phases_disabled_by_the_actual_session() {
        let mut game = quiet_session();
        let code = execute_buy(&mut game);
        for _ in 1..3 {
            game.step().unwrap();
        }
        game.end_civil_day().unwrap();
        let saved = game.save().unwrap();
        assert_eq!(saved.setup.auction_ticks, 0);
        assert_eq!(saved.setup.closing_auction_ticks, 0);
        for (phase, minute) in [
            (HistoryTradingPhase::OpenAuction, 565),
            (HistoryTradingPhase::ClosingAuction, 900),
        ] {
            let mut forged = saved.clone();
            let bar = &mut forged.retained_market_history[0]
                .securities
                .get_mut(&code)
                .unwrap()
                .bars[0];
            bar.phase = phase;
            bar.minute_of_day = minute;
            assert!(
                GameSession::restore(&forged).is_err(),
                "关闭的竞价阶段不能制造真实历史"
            );
        }
        let mut forbidden_continuous = saved;
        forbidden_continuous.setup.closing_auction_ticks = 1;
        forbidden_continuous.retained_market_history[0]
            .securities
            .get_mut(&code)
            .unwrap()
            .bars[0]
            .minute_of_day = 898;
        assert!(
            GameSession::restore(&forbidden_continuous).is_err(),
            "14:57后属于配置的收盘竞价，不应接受连续竞价历史"
        );
    }

    #[test]
    fn nullable_history_fields_require_an_explicit_null_or_value() {
        let request = serde_json::json!({"code":"600888","date_from":"2030-01-02","date_to":"2030-01-03","after":null,"page_size":2});
        serde_json::from_value::<MarketHistoryRequest>(request.clone()).unwrap();
        let mut missing = request;
        missing.as_object_mut().unwrap().remove("after");
        assert!(
            serde_json::from_value::<MarketHistoryRequest>(missing).is_err(),
            "after必须显式提供日期或null，不允许缺失默认补齐"
        );
        let page = serde_json::json!({"code":"600888","entries":[],"next_cursor":null,"settled_through":null});
        serde_json::from_value::<MarketHistoryPage>(page.clone()).unwrap();
        for field in ["next_cursor", "settled_through"] {
            let mut missing = page.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<MarketHistoryPage>(missing).is_err(),
                "{field}必须显式提供"
            );
        }
        let entry = serde_json::json!({"date":"2030-01-02","availability":"NotEnded","bars":[],"daily_candle":null});
        serde_json::from_value::<MarketHistoryEntry>(entry.clone()).unwrap();
        let mut missing = entry;
        missing.as_object_mut().unwrap().remove("daily_candle");
        assert!(
            serde_json::from_value::<MarketHistoryEntry>(missing).is_err(),
            "daily_candle必须显式提供真实日K或null"
        );
    }

    #[test]
    fn minute_prices_obey_the_securities_own_tick() {
        let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
        setup.stocks[0].tick = Money::from_cents(10);
        let mut bar = MinuteBar {
            minute_of_day: 570,
            phase: HistoryTradingPhase::Continuous,
            open: Money::from_cents(1000),
            high: Money::from_cents(1010),
            low: Money::from_cents(1000),
            close: Money::from_cents(1010),
            volume_shares: 100,
            turnover_cents: 100500,
            trade_count: 2,
        };
        validate_stock_bars(&setup.stocks[0], &setup, &[bar.clone()]).unwrap();
        bar.open = Money::from_cents(1001);
        validate_bar(&bar).unwrap();
        assert!(
            matches!(validate_stock_bars(&setup.stocks[0],&setup,&[bar]),Err(SessionError::InvalidSave(message)) if message.contains("最小变动单位"))
        );
    }
}

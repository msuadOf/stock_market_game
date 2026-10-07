use super::*;

impl GameSession {
    pub(in crate::session) fn ingress_calendar_publication(
        &self,
    ) -> Result<Option<super::shared_ingress::CalendarPublication>, SessionError> {
        self.ingress
            .as_ref()
            .map(|binding| binding.source.calendar_publication())
            .transpose()
    }

    pub(in crate::session) fn publish_ingress_calendar(
        &self,
        expected: Option<super::shared_ingress::CalendarPublication>,
    ) -> Result<(), SessionError> {
        match (&self.ingress, expected) {
            (Some(binding), Some(expected)) => {
                binding.source.publish_calendar(expected, self.civil_date())
            }
            (None, None) => Ok(()),
            _ => Err(SessionError::InvalidSave(
                "自然日发布事务的收件入口身份发生变化".into(),
            )),
        }
    }
    pub fn stock_day_status(&self, code: &StockCode) -> Result<crate::DayStatus, SessionError> {
        let exchange = self.state.civil_clock.stock_exchange(code)
            .ok_or_else(|| {
                SessionError::InvalidSetup(format!("交易日历查询指向未知证券 {}", code.0))
            })?;
        Ok(self
            .state
            .civil_clock
            .exchange_day_status(exchange, self.civil_date())?)
    }

    pub fn stock_trading_day(&self, code: &StockCode) -> Result<u64, SessionError> {
        let history = self
            .state
            .candle_book
            .histories()
            .get(code)
            .ok_or_else(|| {
                SessionError::InvalidSetup(format!("交易日计数查询指向未知证券 {}", code.0))
            })?;
        let completed = history.len().checked_sub(360).ok_or_else(|| {
            SessionError::InvalidSave(format!("证券 {} 缺少完整360个交易日虚拟前史", code.0))
        })?;
        u64::try_from(completed)
            .map_err(|_| SessionError::InvalidSave("证券交易日计数超出u64".into()))
    }

    pub fn stock_market_minute(&self, code: &StockCode) -> Result<u64, SessionError> {
        let start = self
            .stock_trading_day(code)?
            .checked_mul(u64::from(GAME_INTRADAY_MINUTES_PER_DAY))
            .ok_or_else(|| SessionError::InvalidSave("证券交易分钟计数溢出".into()))?;
        if !matches!(self.stock_day_status(code)?, crate::DayStatus::Trading) {
            return Ok(start);
        }
        let day_tick = self.tick() % self.state.setup.ticks_per_day;
        let continuous_ticks = self.state.setup.ticks_per_day
            - self.state.setup.auction_ticks
            - self.state.setup.closing_auction_ticks;
        let completed = completed_market_minute_count(
            day_tick
                .saturating_sub(self.state.setup.auction_ticks)
                .min(continuous_ticks),
            continuous_ticks,
        )
        .map_err(|error| SessionError::InvalidSave(error.to_string()))?;
        start
            .checked_add(u64::from(completed))
            .ok_or_else(|| SessionError::InvalidSave("证券交易分钟计数溢出".into()))
    }

    pub(super) fn validate_player_calendar(&self, intent: &Intent) -> Result<(), SessionError> {
        let code = match intent {
            Intent::PlaceLimit { code, .. }
            | Intent::PlaceMarket { code, .. }
            | Intent::Cancel { code, .. } => code,
        };
        if let Some(stock) = self
            .state
            .setup
            .stocks
            .iter()
            .find(|stock| &stock.code == code)
        {
            if let crate::DayStatus::Closed(reason) = self.stock_day_status(code)? {
                return Err(crate::calendar::CalendarError::NotATradingDay {
                    exchange: session_calendar_exchange(stock.exchange),
                    date: self.civil_date(),
                    reason,
                }
                .into());
            }
        }
        Ok(())
    }
}

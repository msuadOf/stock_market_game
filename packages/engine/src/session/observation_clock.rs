use super::*;

/// 日内观察秒映射（D3a-2c 件2 提取为共享纯函数：session 观察钟与组线程
/// 组装 [`crate::session::DecisionChainObservation`] 的 `now` 用同一映射，
/// 字段不得分叉）。三段：竞价申报窗（09:15 起每窗等分 15 分钟）→ 连续
/// （09:30 起 4 小时，午休 11:30–13:00 折算）→ 收盘竞价（14:57 起）。
pub(crate) use super::exchange_calendar_model::intraday_observation_second;

impl GameSession {
    pub(crate) fn observation_civil_instant(&self) -> CivilInstant {
        let day_tick = self.state.tick - u64::from(self.state.day) * self.state.setup.ticks_per_day;
        if day_tick == 0
            && self.state.tick > 0
            && self.state.civil_clock.phase() == super::civil_clock::CivilPhase::IntradayTrading
        {
            let completed_market_days = self.state.tick / self.state.setup.ticks_per_day;
            let expected_sessions = self
                .state
                .civil_clock
                .completed_trading_sessions_expected()
                .expect("validated civil clock session projection");
            if completed_market_days == u64::from(expected_sessions) {
                return CivilInstant::new(self.civil_date(), 15 * 3600)
                    .expect("validated civil session date");
            }
        }
        let second = intraday_observation_second(
            day_tick,
            self.state.setup.ticks_per_day,
            self.state.setup.auction_ticks,
            self.state.setup.closing_auction_ticks,
        );
        CivilInstant::new(self.civil_date(), second).expect("validated civil session date")
    }
}

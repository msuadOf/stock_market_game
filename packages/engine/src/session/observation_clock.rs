use super::*;

impl GameSession {
    pub(super) fn observation_civil_instant(&self) -> CivilInstant {
        let day_tick = self.state.tick - u64::from(self.state.day) * self.state.setup.ticks_per_day;
        if day_tick == 0
            && self.state.day > 0
            && self.state.civil_clock.phase() == CivilPhase::IntradayTrading
            && self.state.day
                == self
                    .state
                    .civil_clock
                    .completed_trading_sessions_expected()
                    .expect("已校验自然日范围内的会话数可计算")
        {
            return CivilInstant::new(self.civil_date(), 15 * 3600).expect("收盘时刻合法");
        }
        observation_instant_at(self.civil_date(), day_tick, &self.state.setup)
    }
}

pub(super) fn observation_instant_at(
    date: CivilDate,
    day_tick: u64,
    setup: &SessionSetup,
) -> CivilInstant {
    let continuous = setup.ticks_per_day - setup.auction_ticks - setup.closing_auction_ticks;
    let second = if day_tick < setup.auction_ticks {
        9 * 3600 + 15 * 60 + day_tick * 900 / setup.auction_ticks
    } else if setup.closing_auction_ticks > 0
        && day_tick >= setup.ticks_per_day - setup.closing_auction_ticks
    {
        14 * 3600
            + 57 * 60
            + (day_tick - (setup.ticks_per_day - setup.closing_auction_ticks)) * 180
                / setup.closing_auction_ticks
    } else {
        let duration = if setup.closing_auction_ticks > 0 {
            14_220
        } else {
            14_400
        };
        let seconds = (day_tick - setup.auction_ticks) * duration / continuous;
        9 * 3600 + 30 * 60 + seconds + if seconds >= 7200 { 5400 } else { 0 }
    };
    CivilInstant::new(
        date,
        u32::try_from(second).expect("validated intraday seconds"),
    )
    .expect("validated civil session date")
}

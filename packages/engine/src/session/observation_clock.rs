use super::*;

impl GameSession {
    pub(super) fn observation_civil_instant(&self) -> CivilInstant {
        let day_tick = self.state.tick - u64::from(self.state.day) * self.state.setup.ticks_per_day;
        let continuous = self.state.setup.ticks_per_day
            - self.state.setup.auction_ticks
            - self.state.setup.closing_auction_ticks;
        let second = if day_tick < self.state.setup.auction_ticks {
            9 * 3600 + 15 * 60 + day_tick * 900 / self.state.setup.auction_ticks
        } else if self.state.setup.closing_auction_ticks > 0
            && day_tick >= self.state.setup.ticks_per_day - self.state.setup.closing_auction_ticks
        {
            14 * 3600
                + 57 * 60
                + (day_tick
                    - (self.state.setup.ticks_per_day - self.state.setup.closing_auction_ticks))
                    * 180
                    / self.state.setup.closing_auction_ticks
        } else {
            let duration = if self.state.setup.closing_auction_ticks > 0 {
                14_220
            } else {
                14_400
            };
            let seconds = (day_tick - self.state.setup.auction_ticks) * duration / continuous;
            9 * 3600 + 30 * 60 + seconds + if seconds >= 7200 { 5400 } else { 0 }
        };
        CivilInstant::new(
            self.civil_date(),
            u32::try_from(second).expect("validated intraday seconds"),
        )
        .expect("validated civil session date")
    }
}

use super::*;

impl GameSession {
    pub(super) fn observation_civil_instant(&self) -> CivilInstant {
        let day_tick = self.tick - u64::from(self.day) * self.setup.ticks_per_day;
        let continuous =
            self.setup.ticks_per_day - self.setup.auction_ticks - self.setup.closing_auction_ticks;
        let second = if day_tick < self.setup.auction_ticks {
            9 * 3600 + 15 * 60 + day_tick * 900 / self.setup.auction_ticks
        } else if self.setup.closing_auction_ticks > 0
            && day_tick >= self.setup.ticks_per_day - self.setup.closing_auction_ticks
        {
            14 * 3600
                + 57 * 60
                + (day_tick - (self.setup.ticks_per_day - self.setup.closing_auction_ticks)) * 180
                    / self.setup.closing_auction_ticks
        } else {
            let duration = if self.setup.closing_auction_ticks > 0 {
                14_220
            } else {
                14_400
            };
            let seconds = (day_tick - self.setup.auction_ticks) * duration / continuous;
            9 * 3600 + 30 * 60 + seconds + if seconds >= 7200 { 5400 } else { 0 }
        };
        CivilInstant::new(
            self.civil_date(),
            u32::try_from(second).expect("validated intraday seconds"),
        )
        .expect("validated civil session date")
    }
}

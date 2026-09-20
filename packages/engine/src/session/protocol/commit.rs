use super::{
    AuctionPoint, AuctionPointKind, ContinuousPoint, EventFact, TickFrame, TickTimeseriesPayload,
};
use crate::session::{Event, GameSession, StepFatal};

impl GameSession {
    pub fn step_frame(&mut self) -> Result<TickFrame, StepFatal> {
        let seq_from = self.seq();
        let events = self.step()?;
        let snapshot = self.runtime_snapshot();
        let facts =
            super::attach_facts(&events).map_err(|error| StepFatal::InvariantViolation {
                location: "step_frame".into(),
                description: error.to_string(),
            })?;
        let timeseries_payload = project_timeseries(&facts, snapshot);
        Ok(TickFrame {
            facts,
            tick: self.tick(),
            events,
            timeseries_payload,
            seq_from,
            seq_to: self.seq(),
        })
    }
}

pub fn project_timeseries(facts: &[EventFact], snapshot: crate::Snapshot) -> TickTimeseriesPayload {
    let mut timeseries_payload = TickTimeseriesPayload {
        markets: snapshot.markets,
        active_daily_candles: snapshot.active_daily_candles,
        ..TickTimeseriesPayload::default()
    };
    for fact in facts {
        let event = &fact.event;
        match event {
            Event::AuctionTick {
                code,
                phase,
                indicative_price,
                matched_volume,
                imbalance,
                tick,
                ..
            } => {
                timeseries_payload
                    .auction_points
                    .entry(code.clone())
                    .or_default()
                    .push(AuctionPoint {
                        key: fact.key.clone(),
                        tick: *tick,
                        kind: AuctionPointKind::Indication,
                        phase: *phase,
                        indicative_price: *indicative_price,
                        matched_volume: *matched_volume,
                        imbalance: Some(*imbalance),
                    });
            }
            Event::AuctionCompleted {
                code,
                tick,
                phase,
                clearing_price,
                matched_volume,
                ..
            } => {
                timeseries_payload
                    .auction_points
                    .entry(code.clone())
                    .or_default()
                    .push(AuctionPoint {
                        key: fact.key.clone(),
                        tick: *tick,
                        phase: *phase,
                        kind: AuctionPointKind::Completion,
                        indicative_price: *clearing_price,
                        matched_volume: *matched_volume,
                        imbalance: None,
                    });
            }
            Event::PriceTick {
                code,
                tick,
                last_price,
                daily_candle,
                bids,
                asks,
                ..
            } => {
                timeseries_payload.continuous_points.insert(
                    code.clone(),
                    ContinuousPoint {
                        tick: *tick,
                        phase: crate::TradingPhase::Continuous,
                        last_price: *last_price,
                        cumulative_volume: daily_candle.volume,
                        bids: bids.clone(),
                        asks: asks.clone(),
                    },
                );
            }
            Event::DayBoundary {
                closed_daily_candles,
                ..
            } => {
                timeseries_payload.closed_daily_candles = closed_daily_candles.clone();
            }
            Event::Trade { .. }
            | Event::CivilDateAdvanced { .. }
            | Event::CompanyDisclosurePublished { .. }
            | Event::IntentRejected { .. }
            | Event::SettlementError { .. }
            | Event::ResourceLimit { .. }
            | Event::OrderCanceled { .. }
            | Event::OrderAccepted { .. } => {}
        }
    }
    for points in timeseries_payload.auction_points.values_mut() {
        points.sort_by(|left, right| left.key.cmp(&right.key));
    }
    timeseries_payload
}

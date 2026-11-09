use super::{ProtocolError, TickFrame};
use std::collections::BTreeSet;

/// 已在 producer 验证并提取的报价序号区间，使用 `(seq_from, seq_to]`。
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct QuoteCoverage {
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq_from: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq_to: u64,
}

/// 条件单所需的原始逐点价格；不复制盘口与 candle。
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct PriceObservation {
    pub code: crate::StockId,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub seq: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    #[ts(type = "number")]
    pub tick: u64,
    pub last_price: crate::Money,
}

pub(super) fn validate_coverage(frame: &TickFrame) -> Result<(), ProtocolError> {
    let invalid = || ProtocolError::SequenceCoverage {
        from: frame.seq_from,
        to: frame.seq_to,
    };
    let (Some(ranges), Some(observations)) = (&frame.quote_coverage, &frame.price_observations)
    else {
        if frame.quote_coverage.is_some() || frame.price_observations.is_some() {
            return Err(invalid());
        }
        return super::validate_sequence(
            frame.facts.iter().map(|fact| fact.event.seq()),
            frame.facts.len(),
            frame.seq_from,
            frame.seq_to,
        );
    };
    let max = crate::orderbook::js_safe_u64::MAX;
    if frame.seq_from > frame.seq_to || frame.seq_to > max {
        return Err(invalid());
    }
    let mut previous = frame.seq_from;
    let mut width = 0_u64;
    let mut segments = Vec::with_capacity(ranges.len() + frame.facts.len());
    for range in ranges {
        if range.seq_from < previous
            || range.seq_from >= range.seq_to
            || range.seq_to > frame.seq_to
        {
            return Err(invalid());
        }
        width = width
            .checked_add(range.seq_to - range.seq_from)
            .ok_or_else(invalid)?;
        segments.push((range.seq_from, range.seq_to));
        previous = range.seq_to;
    }
    if width != observations.len() as u64 {
        return Err(invalid());
    }
    let mut range_index = 0;
    let mut previous_seq = frame.seq_from;
    let mut previous_tick = frame.tick_from.unwrap_or(frame.tick);
    for point in observations {
        while range_index < ranges.len() && point.seq > ranges[range_index].seq_to {
            range_index += 1;
        }
        if point.seq <= previous_seq
            || point.tick < previous_tick
            || point.tick > frame.tick
            || point.last_price.cents() < 0
            || range_index == ranges.len()
            || point.seq <= ranges[range_index].seq_from
        {
            return Err(invalid());
        }
        previous_seq = point.seq;
        previous_tick = point.tick;
    }
    let mut seen = BTreeSet::new();
    let mut chronology: Vec<_> = observations
        .iter()
        .map(|point| (point.seq, point.tick))
        .collect();
    for (index, fact) in frame.facts.iter().enumerate() {
        let seq = fact.event.seq();
        if matches!(fact.event, crate::Event::PriceTick { .. })
            || seq <= frame.seq_from
            || seq > frame.seq_to
            || !seen.insert(seq)
        {
            return Err(invalid());
        }
        segments.push((seq - 1, seq));
        chronology.push((
            seq,
            frame
                .fact_ticks
                .as_ref()
                .map_or(frame.tick, |ticks| ticks[index]),
        ));
    }
    chronology.sort_unstable();
    if chronology.windows(2).any(|pair| pair[0].1 > pair[1].1) {
        return Err(invalid());
    }
    segments.sort_unstable();
    let mut cursor = frame.seq_from;
    for (from, to) in segments {
        if from != cursor {
            return Err(invalid());
        }
        cursor = to;
    }
    if cursor != frame.seq_to {
        return Err(invalid());
    }
    Ok(())
}

/// 与 Web `TradingTimeline.minuteSlot` 相同的整数展示槽，绝非交易分钟键。
pub(super) fn minute_slot(tick: u64, timing: [u64; 3]) -> Result<u64, ProtocolError> {
    let [total, opening, closing] = timing;
    let continuous = total
        .checked_sub(opening)
        .and_then(|v| v.checked_sub(closing))
        .filter(|v| *v > 0)
        .ok_or(ProtocolError::TickContinuity { tick })?;
    let offset = tick
        .checked_sub(1)
        .ok_or(ProtocolError::TickContinuity { tick })?
        % total;
    if offset < opening {
        return Err(ProtocolError::TickContinuity { tick });
    }
    let seconds = if closing == 0 || offset < total - closing {
        (u128::from(offset - opening) * if closing == 0 { 14400 } else { 14220 })
            / u128::from(continuous)
    } else {
        14220 + u128::from(offset - (total - closing)) * 180 / u128::from(closing)
    };
    Ok(seconds as u64 / 60)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn frame() -> TickFrame {
        TickFrame {
            quote_coverage: Some(vec![QuoteCoverage {
                seq_from: 0,
                seq_to: 3,
            }]),
            price_observations: Some(
                (1..=3)
                    .map(|seq| PriceObservation {
                        code: crate::StockId(0),
                        seq,
                        tick: seq,
                        last_price: crate::Money::from_cents(100),
                    })
                    .collect(),
            ),
            tick_from: Some(1),
            fact_ticks: Some(Vec::new()),
            tick: 3,
            facts: Vec::new(),
            timeseries_payload: Default::default(),
            seq_from: 0,
            seq_to: 3,
        }
    }
    #[test]
    fn compressed_quote_coverage_is_exact_and_observations_are_complete() {
        frame().validate().unwrap();
        let mut missing = frame();
        missing.price_observations.as_mut().unwrap().remove(1);
        assert!(missing.validate().is_err());
        let mut duplicate = frame();
        duplicate.price_observations.as_mut().unwrap()[1].seq = 1;
        assert!(duplicate.validate().is_err());
        let mut backwards = frame();
        backwards.price_observations.as_mut().unwrap()[1].tick = 0;
        assert!(backwards.validate().is_err());
        let mut gap = frame();
        gap.quote_coverage.as_mut().unwrap()[0].seq_from = 1;
        assert!(gap.validate().is_err());
        let mut overlap = frame();
        overlap
            .quote_coverage
            .as_mut()
            .unwrap()
            .push(QuoteCoverage {
                seq_from: 1,
                seq_to: 2,
            });
        assert!(overlap.validate().is_err());
        let mut outside = frame();
        outside.quote_coverage.as_mut().unwrap()[0].seq_to = 4;
        assert!(outside.validate().is_err());
    }
    #[test]
    fn quote_chart_slot_matches_default_and_custom_web_integer_mapping() {
        assert_eq!(minute_slot(901, [15300, 900, 180]).unwrap(), 0);
        assert_eq!(minute_slot(961, [15300, 900, 180]).unwrap(), 1);
        assert_eq!(
            (1..=4)
                .map(|tick| minute_slot(tick, [4, 0, 0]).unwrap())
                .collect::<Vec<_>>(),
            vec![0, 60, 120, 180]
        );
        assert!(minute_slot(900, [15300, 900, 180]).is_err());
    }
    #[test]
    fn quote_and_retained_fact_coverage_keep_source_time_and_retry_payload() {
        use crate::session::protocol::{
            EngineUpdate, EventFact, ReplayDecision, ReplayGuard, TickBatch,
        };
        let mut mixed = frame();
        let event = crate::Event::CivilDateAdvanced {
            seq: 2,
            settled_date: crate::CivilDate::from_iso("2030-01-02").unwrap(),
            next_date: crate::CivilDate::from_iso("2030-01-03").unwrap(),
            next_status: crate::DayStatus::Trading,
        };
        mixed.facts = vec![EventFact {
            key: crate::session::pipeline::EventStableKey::for_event(&event, 0),
            event,
        }];
        mixed.fact_ticks = Some(vec![2]);
        mixed.quote_coverage = Some(vec![
            QuoteCoverage {
                seq_from: 0,
                seq_to: 1,
            },
            QuoteCoverage {
                seq_from: 2,
                seq_to: 3,
            },
        ]);
        mixed.price_observations.as_mut().unwrap().remove(1);
        mixed.validate().unwrap();
        let mut wrong_time = mixed.clone();
        wrong_time.price_observations.as_mut().unwrap()[1].tick = 1;
        assert!(
            wrong_time.validate().is_err(),
            "报价与保留事实共同按 seq 保持原 tick 顺序"
        );
        let update = EngineUpdate::TickBatch(Box::new(TickBatch {
            frames: vec![mixed],
            runtime_snapshot: None,
            runtime_delta: None,
        }));
        let mut replay = ReplayGuard::new(0, 0);
        assert_eq!(replay.ingest(&update).unwrap(), ReplayDecision::Applied);
        let mut changed = update.clone();
        if let EngineUpdate::TickBatch(batch) = &mut changed {
            batch.frames[0].price_observations.as_mut().unwrap()[0].last_price =
                crate::Money::from_cents(200);
        }
        assert!(
            replay.ingest(&changed).is_err(),
            "观察价格改变不能当作相同重试"
        );
    }
}

//! 一个市场会话的便携调度与测速策略；宿主只提供单调时钟样本。
use super::{Cadence, Speed};
use libos::clock::Instant;
use std::time::Duration;

pub const BASE_TICK_MS: u64 = 1_000;
pub const MAX_SPEED_MULTIPLIER: f64 = BASE_TICK_MS as f64;
const SAMPLE_MIN: Duration = Duration::from_millis(500);
// 为不同 Instant 实现留出相同的可表示余量；禁止用宿主独有的上限判断倍速。
const MAX_PERIOD: Duration = Duration::from_secs(100 * 365 * 24 * 60 * 60);

#[derive(Clone, Copy, Debug, thiserror::Error, PartialEq, Eq)]
pub enum SpeedError {
    #[error("市场倍速必须在 (0, 1000] 之间或为正无穷")]
    InvalidMultiplier,
    #[error("市场倍速过小，固定周期不可表示")]
    UnrepresentablePeriod,
}

pub fn validate_speed(base_ms: u64, multiplier: f64, now: Instant) -> Result<Speed, SpeedError> {
    if multiplier == f64::INFINITY {
        return Ok(Speed::Fastest);
    }
    if !multiplier.is_finite() || multiplier <= 0.0 || multiplier > MAX_SPEED_MULTIPLIER {
        return Err(SpeedError::InvalidMultiplier);
    }
    let seconds = ((base_ms as f64 / 1_000.0) / multiplier).max(0.001);
    let period =
        Duration::try_from_secs_f64(seconds).map_err(|_| SpeedError::UnrepresentablePeriod)?;
    if period > MAX_PERIOD || now.checked_add(period).is_none() {
        return Err(SpeedError::UnrepresentablePeriod);
    }
    Ok(Speed::Fixed(period))
}

#[derive(Clone, Copy, Debug, serde::Serialize, PartialEq)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum RequestedSpeed {
    Fixed { multiplier: f64 },
    Fastest,
}

#[derive(Clone, Debug, serde::Serialize, PartialEq)]
pub struct SpeedMetrics {
    pub requested: RequestedSpeed,
    pub actual_multiplier: Option<f64>,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    pub sample_duration_ms: u64,
    #[serde(with = "crate::orderbook::js_safe_u64")]
    pub sample_ticks: u64,
    pub running: bool,
}

pub struct Pacing {
    pub cadence: Cadence,
    base_ms: u64,
    sample_started: Instant,
    sample_start_tick: u64,
    stats: SpeedMetrics,
}

impl Pacing {
    pub fn new(base_ms: u64, tick: u64, now: Instant) -> Self {
        let Speed::Fixed(period) =
            validate_speed(base_ms, 1.0, now).expect("市场初始时钟必须能表示 1x")
        else {
            unreachable!("1x 是固定周期")
        };
        Self {
            cadence: Cadence::new(period),
            base_ms,
            sample_started: now,
            sample_start_tick: tick,
            stats: SpeedMetrics {
                requested: RequestedSpeed::Fixed { multiplier: 1.0 },
                actual_multiplier: Some(0.0),
                sample_duration_ms: 0,
                sample_ticks: 0,
                running: false,
            },
        }
    }

    pub fn apply_speed(
        &mut self,
        multiplier: f64,
        tick: u64,
        now: Instant,
    ) -> Result<(), SpeedError> {
        let speed = validate_speed(self.base_ms, multiplier, now)?;
        self.cadence
            .set_speed(speed, now)
            .map_err(|_| SpeedError::UnrepresentablePeriod)?;
        self.stats.requested = match speed {
            Speed::Fixed(_) => RequestedSpeed::Fixed { multiplier },
            Speed::Fastest => RequestedSpeed::Fastest,
        };
        self.reset_sample(tick, now);
        Ok(())
    }

    pub fn set_running(&mut self, running: bool, tick: u64, now: Instant) {
        if self.is_running() != running {
            self.cadence.set_running(running, now);
            self.reset_sample(tick, now);
        }
    }

    pub fn pause_at_civil_boundary(&mut self, tick: u64, now: Instant) {
        self.cadence.set_running(false, now);
        self.reset_sample(tick, now);
    }

    /// 错误停止保留最后完整窗口，普通暂停则重置为 0。
    pub fn stop_after_failure(&mut self, now: Instant) {
        self.cadence.set_running(false, now);
        self.stats.running = false;
    }

    pub fn reset_after_restore(&mut self, tick: u64, now: Instant) {
        self.cadence.reanchor(now);
        self.reset_sample(tick, now);
    }

    fn reset_sample(&mut self, tick: u64, now: Instant) {
        self.sample_started = now;
        self.sample_start_tick = tick;
        self.stats.running = self.is_running();
        self.stats.actual_multiplier = if self.stats.running { None } else { Some(0.0) };
        self.stats.sample_duration_ms = 0;
        self.stats.sample_ticks = 0;
    }

    pub fn refresh_metrics(&mut self, tick: u64, now: Instant) -> SpeedMetrics {
        let elapsed = now.saturating_duration_since(self.sample_started);
        if self.is_running() && elapsed >= SAMPLE_MIN {
            let ticks = tick.saturating_sub(self.sample_start_tick);
            self.stats.actual_multiplier = Some(ticks as f64 / elapsed.as_secs_f64());
            self.stats.sample_duration_ms = elapsed.as_millis().min(u128::from(u64::MAX)) as u64;
            self.stats.sample_ticks = ticks;
            self.sample_started = now;
            self.sample_start_tick = tick;
        }
        self.stats.clone()
    }

    pub const fn requested_speed(&self) -> RequestedSpeed {
        self.stats.requested
    }

    pub const fn is_running(&self) -> bool {
        self.cadence.is_running()
    }
    pub const fn is_fastest(&self) -> bool {
        self.cadence.is_fastest()
    }
    pub const fn tick_interval(&self) -> Duration {
        self.cadence.period()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_validator_rejects_unrepresentable_and_out_of_range_without_state_change() {
        let now = Instant::now();
        let mut pacing = Pacing::new(BASE_TICK_MS, 10, now);
        for invalid in [0.0, -1.0, f64::NAN, f64::NEG_INFINITY, 1_001.0] {
            assert_eq!(
                pacing.apply_speed(invalid, 10, now),
                Err(SpeedError::InvalidMultiplier)
            );
        }
        assert_eq!(
            pacing.apply_speed(f64::MIN_POSITIVE, 10, now),
            Err(SpeedError::UnrepresentablePeriod)
        );
        assert_eq!(pacing.tick_interval(), Duration::from_secs(1));
        pacing.apply_speed(1_000.0, 10, now).unwrap();
        assert_eq!(pacing.tick_interval(), Duration::from_millis(1));
        pacing.apply_speed(f64::INFINITY, 10, now).unwrap();
        assert!(pacing.is_fastest());
    }

    #[test]
    fn telemetry_preserves_the_last_complete_window_on_failure_and_resets_on_control() {
        let now = Instant::now();
        let mut pacing = Pacing::new(BASE_TICK_MS, 10, now);
        assert_eq!(pacing.refresh_metrics(10, now).actual_multiplier, Some(0.0));
        pacing.set_running(true, 10, now);
        assert_eq!(
            pacing
                .refresh_metrics(12, now + Duration::from_millis(499))
                .actual_multiplier,
            None
        );
        let later = now + Duration::from_secs(2);
        let active = pacing.refresh_metrics(12, later);
        assert_eq!(
            (
                active.actual_multiplier,
                active.sample_ticks,
                active.sample_duration_ms
            ),
            (Some(1.0), 2, 2_000)
        );
        pacing.stop_after_failure(later);
        let failure = pacing.refresh_metrics(12, later);
        assert!(!failure.running);
        assert_eq!(failure.actual_multiplier, Some(1.0));
        pacing.reset_after_restore(1, later);
        let restored = pacing.refresh_metrics(1, later);
        assert_eq!(
            (
                restored.actual_multiplier,
                restored.sample_ticks,
                restored.sample_duration_ms
            ),
            (Some(0.0), 0, 0)
        );
    }
}

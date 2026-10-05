//! 策略实例采样助手：观察频率换算、区间采样与个体订单规模采样。

use super::*;

pub(super) fn daily_observations_to_tick_probability(
    observations_per_day: f64,
    ticks_per_day: u64,
) -> f64 {
    -(-observations_per_day / ticks_per_day as f64).exp_m1()
}

pub(super) fn sample_between(rng: &mut dyn Rng, low: f64, high: f64) -> f64 {
    low + (high - low) * rng.next_f64()
}

/// 以配置数量为群体中心，为每个 NPC 固定采样一个 60%–140% 的个体订单规模。
/// 可形成整手的基准始终返回 100 股整数倍；不足一手的显式配置保持原值，避免静默放大。
const MAX_ORDER_SIZE_BASELINE: u32 = ((u32::MAX as u64) * 5 / 7) as u32;

pub(super) fn validate_order_size_sampling_range(
    baseline: u32,
    param: &'static str,
) -> Result<(), StrategyError> {
    if baseline > MAX_ORDER_SIZE_BASELINE {
        return Err(StrategyError::InvalidParam {
            param,
            reason: format!(
                "{baseline} cannot represent the complete 60%-140% sampling range; maximum is {MAX_ORDER_SIZE_BASELINE}"
            ),
        });
    }
    Ok(())
}

pub(super) fn sample_individual_order_size(
    rng: &mut dyn Rng,
    baseline: u32,
    param: &'static str,
) -> Result<u32, StrategyError> {
    validate_order_size_sampling_range(baseline, param)?;
    const LOT: u64 = 100;
    if baseline < LOT as u32 {
        return Ok(baseline);
    }
    let baseline = u64::from(baseline);
    let lower_shares = baseline * 3 / 5;
    let upper_shares = baseline * 7 / 5;
    let lower_lots = lower_shares.div_ceil(LOT).max(1);
    let upper_lots = (upper_shares / LOT).max(lower_lots);
    let lots = sample_u64_inclusive(rng, lower_lots, upper_lots);
    Ok((lots * LOT) as u32)
}

fn sample_u64_inclusive(rng: &mut dyn Rng, low: u64, high: u64) -> u64 {
    let width = high - low + 1;
    low + ((rng.next_f64() * width as f64) as u64).min(width - 1)
}

#[cfg(test)]
mod observation_probability_tests {
    use super::daily_observations_to_tick_probability;

    fn assert_probability(observations_per_day: f64, ticks_per_day: u64, expected_bits: u64) {
        let actual = daily_observations_to_tick_probability(observations_per_day, ticks_per_day);
        assert!(actual.is_finite() && actual > 0.0 && actual <= 1.0);
        assert!(
            actual.to_bits().abs_diff(expected_bits) <= 1,
            "观察概率应在高精度参考值的一 ULP 内：actual={actual}, expected={}",
            f64::from_bits(expected_bits)
        );
    }

    #[test]
    fn moderate_observation_probability_avoids_subtractive_cancellation() {
        assert_probability(2.4, 60, 0x3fa4_1368_18ff_472b);
    }

    #[test]
    fn rare_observation_probability_retains_relative_precision() {
        assert_probability(0.1, 100_000_000, 0x3e11_2e0b_e801_f1d9);
    }

    #[test]
    fn positive_observation_probability_does_not_round_to_zero_at_u64_tick_limit() {
        assert_probability(0.1, u64::MAX, 0x3bb9_9999_9999_999a);
    }
}

#[cfg(test)]
mod order_size_sampling_tests {
    use super::{sample_individual_order_size, Rng};

    struct FixedRng(f64);

    impl Rng for FixedRng {
        fn next_f64(&mut self) -> f64 {
            self.0
        }

        fn next_range_u32(&mut self, lo: u32, _hi: u32) -> u32 {
            lo
        }
    }

    #[test]
    fn individual_order_size_uses_bounded_board_lots() {
        assert_eq!(
            sample_individual_order_size(&mut FixedRng(0.0), 2_000, "order_size").unwrap(),
            1_200
        );
        assert_eq!(
            sample_individual_order_size(&mut FixedRng(0.999_999), 2_000, "order_size").unwrap(),
            2_800
        );
        assert_eq!(
            sample_individual_order_size(&mut FixedRng(0.0), 150, "order_size").unwrap(),
            100
        );
        assert_eq!(
            sample_individual_order_size(&mut FixedRng(0.999_999), 150, "order_size").unwrap(),
            200
        );
    }

    #[test]
    fn sub_lot_configuration_is_not_silently_inflated() {
        assert_eq!(
            sample_individual_order_size(&mut FixedRng(0.5), 99, "order_size").unwrap(),
            99
        );
    }

    #[test]
    fn largest_complete_sampling_range_is_accepted_and_the_next_value_is_rejected() {
        let sampled = sample_individual_order_size(
            &mut FixedRng(0.999_999),
            super::MAX_ORDER_SIZE_BASELINE,
            "order_size",
        )
        .unwrap();
        assert_eq!(sampled % 100, 0);
        assert!(sampled > super::MAX_ORDER_SIZE_BASELINE);
        assert!(sample_individual_order_size(
            &mut FixedRng(0.5),
            super::MAX_ORDER_SIZE_BASELINE + 1,
            "order_size"
        )
        .is_err());
    }
}

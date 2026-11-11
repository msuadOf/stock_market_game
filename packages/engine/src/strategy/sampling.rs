//! 策略实例采样助手：观察频率换算、区间采样与个体订单规模采样。

use super::*;

#[cfg(feature = "verification-harness")]
thread_local! {
    static OBSERVATION_TRACE: std::cell::RefCell<Option<Vec<ObservationTrace>>> = const { std::cell::RefCell::new(None) };
    static OBSERVATION_ACCOUNT: std::cell::Cell<Option<u64>> = const { std::cell::Cell::new(None) };
    static OBSERVATION_MATH: std::cell::Cell<Option<fn(f64) -> f64>> = const { std::cell::Cell::new(None) };
}

#[cfg(feature = "verification-harness")]
struct ObservationMathGuard(Option<fn(f64) -> f64>);

#[cfg(feature = "verification-harness")]
impl Drop for ObservationMathGuard {
    fn drop(&mut self) { OBSERVATION_MATH.with(|math| math.set(self.0)); }
}

/// 验证专用：仅在闭包期间替换当前线程的概率计算函数。
#[cfg(feature = "verification-harness")]
pub fn verification_with_observation_math<T>(math: fn(f64) -> f64, run: impl FnOnce() -> T) -> T {
    let previous = OBSERVATION_MATH.with(|slot| slot.replace(Some(math)));
    let _guard = ObservationMathGuard(previous);
    run()
}

/// 验证专用：保留概率换算的输入、除法结果和输出的原始位值。
#[cfg(feature = "verification-harness")]
#[derive(Debug, PartialEq, Eq)]
pub struct ObservationTrace {
    pub account_id: Option<u64>,
    pub observations_bits: u64,
    pub ticks_per_day: u64,
    pub argument_bits: u64,
    pub probability_bits: u64,
    pub canonical_bits: Option<u64>,
}

/// 验证专用：为同一账户的最近一次换算记录规范化结果，校验原始位值。
#[cfg(feature = "verification-harness")]
pub(crate) fn verification_record_observation_canonical(id: u64, raw: f64, canonical: f64) {
    OBSERVATION_TRACE.with(|trace| {
        if let Some(trace) = trace.borrow_mut().as_mut() {
            let entry = trace.last_mut().expect("缺少观察概率换算记录");
            assert_eq!(entry.account_id, Some(id), "观察概率账户不匹配");
            assert_eq!(entry.probability_bits, raw.to_bits(), "观察概率原始值不匹配");
            assert!(entry.canonical_bits.is_none(), "观察概率重复规范化");
            entry.canonical_bits = Some(canonical.to_bits());
        }
    });
}

#[cfg(feature = "verification-harness")]
struct ObservationAccountGuard(Option<u64>);

#[cfg(feature = "verification-harness")]
impl Drop for ObservationAccountGuard {
    fn drop(&mut self) { OBSERVATION_ACCOUNT.with(|account| account.set(self.0)); }
}

/// 为当前线程中的策略构造关联账户身份，退出闭包时恢复先前身份。
#[cfg(feature = "verification-harness")]
pub(crate) fn verification_observation_account<T>(id: u64, run: impl FnOnce() -> T) -> T {
    let previous = OBSERVATION_ACCOUNT.with(|account| account.replace(Some(id)));
    let _guard = ObservationAccountGuard(previous);
    run()
}

#[cfg(feature = "verification-harness")]
struct ObservationTraceGuard;

#[cfg(feature = "verification-harness")]
impl Drop for ObservationTraceGuard {
    fn drop(&mut self) {
        OBSERVATION_TRACE.with(|trace| { trace.borrow_mut().take(); });
    }
}

/// 仅在闭包期间采集当前线程的观察概率换算；异常退出时清理追踪状态。
#[cfg(feature = "verification-harness")]
pub fn verification_trace_observations<T>(run: impl FnOnce() -> T) -> (T, Vec<ObservationTrace>) {
    OBSERVATION_TRACE.with(|trace| {
        let mut trace = trace.borrow_mut();
        assert!(trace.is_none(), "观察概率追踪不允许嵌套");
        *trace = Some(Vec::new());
    });
    let _guard = ObservationTraceGuard;
    let result = run();
    let trace = OBSERVATION_TRACE.with(|trace| trace.borrow_mut().take().expect("观察概率追踪状态丢失"));
    (result, trace)
}

pub(super) fn daily_observations_to_tick_probability(
    observations_per_day: f64,
    ticks_per_day: u64,
) -> f64 {
    #[cfg(not(feature = "verification-harness"))]
    {
        -(-observations_per_day / ticks_per_day as f64).exp_m1()
    }
    #[cfg(feature = "verification-harness")]
    {
        let argument = -observations_per_day / ticks_per_day as f64;
        let probability = OBSERVATION_MATH.with(|math| match math.get() {
            Some(candidate) => candidate(argument),
            None => -argument.exp_m1(),
        });
        OBSERVATION_TRACE.with(|trace| {
            if let Some(trace) = trace.borrow_mut().as_mut() {
                trace.push(ObservationTrace {
                    account_id: OBSERVATION_ACCOUNT.with(|account| account.get()),
                    observations_bits: observations_per_day.to_bits(),
                    ticks_per_day,
                    argument_bits: argument.to_bits(),
                    probability_bits: probability.to_bits(),
                    canonical_bits: None,
                });
            }
        });
        probability
    }
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

    #[cfg(feature = "verification-harness")]
    #[test]
    fn 验证候选计算仅作用于闭包并在异常后恢复() {
        let original = daily_observations_to_tick_probability(5.5, 15_300);
        let candidate = super::verification_with_observation_math(|argument| argument * -0.5, || {
            let before = daily_observations_to_tick_probability(5.5, 15_300);
            let nested = super::verification_with_observation_math(|_| 0.25, || daily_observations_to_tick_probability(5.5, 15_300));
            assert_eq!(nested, 0.25);
            assert_eq!(daily_observations_to_tick_probability(5.5, 15_300), before);
            let child = std::thread::spawn(|| daily_observations_to_tick_probability(5.5, 15_300)).join().unwrap();
            assert_eq!(child.to_bits(), original.to_bits());
            before
        });
        assert_eq!(candidate.to_bits(), (5.5_f64 / 15_300.0 * 0.5).to_bits());
        assert_eq!(daily_observations_to_tick_probability(5.5, 15_300).to_bits(), original.to_bits());
        let (candidate, trace) = super::verification_trace_observations(|| {
            super::verification_with_observation_math(|_| 0.25, || daily_observations_to_tick_probability(5.5, 15_300))
        });
        assert_eq!(trace[0].probability_bits, candidate.to_bits());
        let failure = std::panic::catch_unwind(|| super::verification_with_observation_math(|_| 0.1, || panic!("验证恢复")));
        assert!(failure.is_err());
        assert_eq!(daily_observations_to_tick_probability(5.5, 15_300).to_bits(), original.to_bits());
    }

    #[cfg(feature = "verification-harness")]
    #[test]
    fn 验证追踪记录原始输入与结果并正确结束() {
        let (result, trace) = super::verification_trace_observations(|| {
            daily_observations_to_tick_probability(5.5, 15_300)
        });
        assert_eq!(trace.len(), 1);
        assert_eq!(trace[0].observations_bits, 5.5_f64.to_bits());
        assert_eq!(trace[0].ticks_per_day, 15_300);
        assert_eq!(trace[0].argument_bits, (-5.5_f64 / 15_300.0).to_bits());
        assert_eq!(trace[0].probability_bits, result.to_bits());
        assert_eq!(trace[0].account_id, None);
        let (_, named) = super::verification_trace_observations(|| {
            super::verification_observation_account(310, || daily_observations_to_tick_probability(5.5, 15_300))
        });
        assert_eq!(named[0].account_id, Some(310));
        let (_, canonical) = super::verification_trace_observations(|| {
            let probability = super::verification_observation_account(310, || daily_observations_to_tick_probability(5.5, 15_300));
            let adjusted = f64::from_bits(probability.to_bits() + 1);
            super::verification_record_observation_canonical(310, probability, adjusted);
        });
        assert_eq!(canonical[0].canonical_bits, Some(canonical[0].probability_bits + 1));
        let (_, nested) = super::verification_trace_observations(|| {
            super::verification_observation_account(310, || {
                super::verification_observation_account(311, || daily_observations_to_tick_probability(5.5, 15_300));
                let failed = std::panic::catch_unwind(|| super::verification_observation_account(312, || panic!("验证账户恢复")));
                assert!(failed.is_err());
                daily_observations_to_tick_probability(5.5, 15_300);
            });
            daily_observations_to_tick_probability(5.5, 15_300);
        });
        assert_eq!(nested.iter().map(|value| value.account_id).collect::<Vec<_>>(), vec![Some(311), Some(310), None]);
        let (_, next) = super::verification_trace_observations(|| ());
        assert!(next.is_empty());
        let failed = std::panic::catch_unwind(|| super::verification_trace_observations(|| panic!("验证清理")));
        assert!(failed.is_err());
        let (_, after_failure) = super::verification_trace_observations(|| ());
        assert!(after_failure.is_empty());
    }

    #[cfg(feature = "verification-harness")]
    #[test]
    fn 规范化追踪拒绝缺失错账户错原值和重复写入() {
        for scenario in 0..4 {
            let result = std::panic::catch_unwind(|| super::verification_trace_observations(|| {
                if scenario == 0 {
                    super::verification_record_observation_canonical(310, 0.5, 0.5);
                    return;
                }
                let raw = super::verification_observation_account(310, || daily_observations_to_tick_probability(5.5, 15_300));
                match scenario {
                    1 => super::verification_record_observation_canonical(311, raw, raw),
                    2 => super::verification_record_observation_canonical(310, 0.5, raw),
                    3 => {
                        super::verification_record_observation_canonical(310, raw, raw);
                        super::verification_record_observation_canonical(310, raw, raw);
                    }
                    _ => unreachable!(),
                }
            }));
            assert!(result.is_err(), "场景 {scenario} 必须显式失败");
        }
    }

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

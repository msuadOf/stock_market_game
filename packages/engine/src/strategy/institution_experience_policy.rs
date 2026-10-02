//! Frozen, individually sampled institution experience thresholds.
//!
//! The ranges below are replaceable game assumptions, not historical formulas or
//! calibrated empirical statistics. This module only stores policy data; it does
//! not turn market or experience observations into intents.

use super::{InstitutionStyle, Rng};

const POLICY_VERSION: u32 = 1;
const ADVERSE_MOVE_RANGE_BP: (u32, u32) = (300, 1000);

struct PolicyRanges {
    loss: (u32, u32),
    profit: (u32, u32),
    pause: (u32, u32),
    failures: (u32, u32),
}

/// Individual response when the cost-relative loss threshold is crossed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum InstitutionLossResponse {
    /// Keep holding or add; this policy does not create an order.
    HoldOrAdd,
    /// Pause Buy activity and review at the next own observation.
    PauseAndReview,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum InstitutionExperiencePolicyError {
    #[error("institution experience policy version must be 1, got {0}")]
    UnsupportedVersion(u32),
    #[error("{field} must be in 1..=10000 bp, got {value}")]
    InvalidBasisPoints { field: &'static str, value: u32 },
    #[error("risk_resume_drawdown_bp {resume} must be below risk_pause_drawdown_bp {pause}")]
    InvalidDrawdownHysteresis { pause: u32, resume: u32 },
    #[error("risk_pause_failed_buys must be positive, got {0}")]
    InvalidFailedBuyThreshold(u32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(
    into = "PersistedInstitutionExperiencePolicy",
    try_from = "PersistedInstitutionExperiencePolicy"
)]
pub struct InstitutionExperiencePolicy {
    /// Only version 1 is currently accepted.
    policy_version: u32,
    /// Selected response when the individual's cost-loss threshold is reached.
    loss_response: InstitutionLossResponse,
    /// Individual cost-relative loss trigger; threshold interpretation is wired elsewhere.
    cost_loss_threshold_bp: u32,
    /// Individual cost-relative profit threshold; this policy does not emit an action.
    cost_profit_threshold_bp: u32,
    /// Individual own-net-worth drawdown threshold for pausing new Buy activity.
    risk_pause_drawdown_bp: u32,
    /// Individual drawdown recovery threshold; resume is only considered at the next own observation.
    risk_resume_drawdown_bp: u32,
    /// Count of real failed-buy experiences that can trigger a Buy pause.
    risk_pause_failed_buys: u32,
    /// Adverse price move after a real Buy; this field alone does not classify adverse selection.
    adverse_move_threshold_bp: u32,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PersistedInstitutionExperiencePolicy {
    pub policy_version: u32,
    pub loss_response: InstitutionLossResponse,
    pub cost_loss_threshold_bp: u32,
    pub cost_profit_threshold_bp: u32,
    pub risk_pause_drawdown_bp: u32,
    pub risk_resume_drawdown_bp: u32,
    pub risk_pause_failed_buys: u32,
    pub adverse_move_threshold_bp: u32,
}

impl InstitutionExperiencePolicy {
    #[expect(
        clippy::too_many_arguments,
        reason = "Preserve the public positional constructor's source compatibility"
    )]
    pub fn new(
        policy_version: u32,
        loss_response: InstitutionLossResponse,
        cost_loss_threshold_bp: u32,
        cost_profit_threshold_bp: u32,
        risk_pause_drawdown_bp: u32,
        risk_resume_drawdown_bp: u32,
        risk_pause_failed_buys: u32,
        adverse_move_threshold_bp: u32,
    ) -> Result<Self, InstitutionExperiencePolicyError> {
        if policy_version != POLICY_VERSION {
            return Err(InstitutionExperiencePolicyError::UnsupportedVersion(
                policy_version,
            ));
        }
        for (field, value) in [
            ("cost_loss_threshold_bp", cost_loss_threshold_bp),
            ("cost_profit_threshold_bp", cost_profit_threshold_bp),
            ("risk_pause_drawdown_bp", risk_pause_drawdown_bp),
            ("risk_resume_drawdown_bp", risk_resume_drawdown_bp),
            ("adverse_move_threshold_bp", adverse_move_threshold_bp),
        ] {
            if !(1..=10_000).contains(&value) {
                return Err(InstitutionExperiencePolicyError::InvalidBasisPoints { field, value });
            }
        }
        if risk_pause_failed_buys == 0 {
            return Err(InstitutionExperiencePolicyError::InvalidFailedBuyThreshold(
                risk_pause_failed_buys,
            ));
        }
        if risk_resume_drawdown_bp >= risk_pause_drawdown_bp {
            return Err(
                InstitutionExperiencePolicyError::InvalidDrawdownHysteresis {
                    pause: risk_pause_drawdown_bp,
                    resume: risk_resume_drawdown_bp,
                },
            );
        }
        Ok(Self {
            policy_version,
            loss_response,
            cost_loss_threshold_bp,
            cost_profit_threshold_bp,
            risk_pause_drawdown_bp,
            risk_resume_drawdown_bp,
            risk_pause_failed_buys,
            adverse_move_threshold_bp,
        })
    }

    /// Samples a frozen instance using a dedicated policy RNG.
    ///
    /// Each threshold consumes one draw; resume drawdown is exactly half the
    /// sampled pause threshold. These per-style ranges are replaceable game
    /// assumptions, not historical formulas or empirical calibrations. Draw
    /// order is response, loss, profit, pause drawdown, failed buys, adverse move;
    /// version is fixed and resume drawdown is derived without an RNG draw.
    pub fn sample(style: InstitutionStyle, rng: &mut dyn Rng) -> Self {
        let ranges = ranges_for_style(style);
        let loss_response = if rng.next_f64() < 0.5 {
            InstitutionLossResponse::HoldOrAdd
        } else {
            InstitutionLossResponse::PauseAndReview
        };
        let cost_loss_threshold_bp = sample_inclusive(ranges.loss.0, ranges.loss.1, rng);
        let cost_profit_threshold_bp = sample_inclusive(ranges.profit.0, ranges.profit.1, rng);
        let risk_pause_drawdown_bp = sample_inclusive(ranges.pause.0, ranges.pause.1, rng);
        let risk_pause_failed_buys = sample_inclusive(ranges.failures.0, ranges.failures.1, rng);
        let adverse_move_threshold_bp =
            sample_inclusive(ADVERSE_MOVE_RANGE_BP.0, ADVERSE_MOVE_RANGE_BP.1, rng);
        Self::new(
            POLICY_VERSION,
            loss_response,
            cost_loss_threshold_bp,
            cost_profit_threshold_bp,
            risk_pause_drawdown_bp,
            risk_pause_drawdown_bp / 2,
            risk_pause_failed_buys,
            adverse_move_threshold_bp,
        )
        .expect("institution style sampling ranges always satisfy policy invariants")
    }

    /// Explicit deterministic midpoint preset for standalone belief-book construction.
    /// It does not consume RNG; production session setup can replace it with `sample`.
    pub fn default_for_style(style: InstitutionStyle) -> Self {
        let ranges = ranges_for_style(style);
        let risk_pause_drawdown_bp = midpoint(ranges.pause);
        Self::new(
            POLICY_VERSION,
            InstitutionLossResponse::HoldOrAdd,
            midpoint(ranges.loss),
            midpoint(ranges.profit),
            risk_pause_drawdown_bp,
            risk_pause_drawdown_bp / 2,
            midpoint(ranges.failures),
            midpoint(ADVERSE_MOVE_RANGE_BP),
        )
        .expect("institution style midpoint presets satisfy policy invariants")
    }

    pub fn policy_version(&self) -> u32 {
        self.policy_version
    }

    pub fn loss_response(&self) -> InstitutionLossResponse {
        self.loss_response
    }

    pub fn cost_loss_threshold_bp(&self) -> u32 {
        self.cost_loss_threshold_bp
    }

    pub fn cost_profit_threshold_bp(&self) -> u32 {
        self.cost_profit_threshold_bp
    }

    pub fn risk_pause_drawdown_bp(&self) -> u32 {
        self.risk_pause_drawdown_bp
    }

    pub fn risk_resume_drawdown_bp(&self) -> u32 {
        self.risk_resume_drawdown_bp
    }

    pub fn risk_pause_failed_buys(&self) -> u32 {
        self.risk_pause_failed_buys
    }

    pub fn adverse_move_threshold_bp(&self) -> u32 {
        self.adverse_move_threshold_bp
    }
}

impl From<InstitutionExperiencePolicy> for PersistedInstitutionExperiencePolicy {
    fn from(policy: InstitutionExperiencePolicy) -> Self {
        Self {
            policy_version: policy.policy_version,
            loss_response: policy.loss_response,
            cost_loss_threshold_bp: policy.cost_loss_threshold_bp,
            cost_profit_threshold_bp: policy.cost_profit_threshold_bp,
            risk_pause_drawdown_bp: policy.risk_pause_drawdown_bp,
            risk_resume_drawdown_bp: policy.risk_resume_drawdown_bp,
            risk_pause_failed_buys: policy.risk_pause_failed_buys,
            adverse_move_threshold_bp: policy.adverse_move_threshold_bp,
        }
    }
}

impl TryFrom<PersistedInstitutionExperiencePolicy> for InstitutionExperiencePolicy {
    type Error = InstitutionExperiencePolicyError;

    fn try_from(policy: PersistedInstitutionExperiencePolicy) -> Result<Self, Self::Error> {
        Self::new(
            policy.policy_version,
            policy.loss_response,
            policy.cost_loss_threshold_bp,
            policy.cost_profit_threshold_bp,
            policy.risk_pause_drawdown_bp,
            policy.risk_resume_drawdown_bp,
            policy.risk_pause_failed_buys,
            policy.adverse_move_threshold_bp,
        )
    }
}

fn sample_inclusive(low: u32, high: u32, rng: &mut dyn Rng) -> u32 {
    let width = u64::from(high - low) + 1;
    low + ((rng.next_f64() * width as f64) as u64).min(width - 1) as u32
}

fn midpoint((low, high): (u32, u32)) -> u32 {
    low + (high - low) / 2
}

fn ranges_for_style(style: InstitutionStyle) -> PolicyRanges {
    let (loss, profit, pause, failures) = match style {
        InstitutionStyle::DeepValue => ((800, 1600), (2000, 4000), (3000, 5000), (3, 5)),
        InstitutionStyle::Growth => ((600, 1200), (1200, 2400), (2000, 3500), (2, 4)),
        InstitutionStyle::Balanced => ((500, 1000), (1000, 2000), (1800, 3000), (2, 4)),
        InstitutionStyle::Defensive => ((300, 800), (600, 1400), (1000, 2000), (1, 3)),
        InstitutionStyle::ActiveTrader => ((100, 500), (400, 1200), (800, 1800), (1, 3)),
    };
    PolicyRanges {
        loss,
        profit,
        pause,
        failures,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positional_constructor_preserves_wire_format() {
        let policy = InstitutionExperiencePolicy::new(
            1,
            InstitutionLossResponse::PauseAndReview,
            800,
            1200,
            3000,
            1500,
            3,
            500,
        )
        .unwrap();
        let expected = serde_json::json!({
            "policy_version": 1,
            "loss_response": "PauseAndReview",
            "cost_loss_threshold_bp": 800,
            "cost_profit_threshold_bp": 1200,
            "risk_pause_drawdown_bp": 3000,
            "risk_resume_drawdown_bp": 1500,
            "risk_pause_failed_buys": 3,
            "adverse_move_threshold_bp": 500,
        });
        assert_eq!(serde_json::to_value(policy).unwrap(), expected);
        assert_eq!(
            InstitutionExperiencePolicy::try_from(PersistedInstitutionExperiencePolicy::from(
                policy
            )),
            Ok(policy)
        );
    }

    #[test]
    fn positional_constructor_preserves_validation_error_precedence() {
        for (version, loss, failures, resume, expected) in [
            (
                0,
                0,
                0,
                3000,
                InstitutionExperiencePolicyError::UnsupportedVersion(0),
            ),
            (
                1,
                0,
                0,
                3000,
                InstitutionExperiencePolicyError::InvalidBasisPoints {
                    field: "cost_loss_threshold_bp",
                    value: 0,
                },
            ),
            (
                1,
                800,
                0,
                3000,
                InstitutionExperiencePolicyError::InvalidFailedBuyThreshold(0),
            ),
            (
                1,
                800,
                3,
                3000,
                InstitutionExperiencePolicyError::InvalidDrawdownHysteresis {
                    pause: 3000,
                    resume: 3000,
                },
            ),
        ] {
            let policy = PersistedInstitutionExperiencePolicy {
                policy_version: version,
                loss_response: InstitutionLossResponse::HoldOrAdd,
                cost_loss_threshold_bp: loss,
                cost_profit_threshold_bp: 1200,
                risk_pause_drawdown_bp: 3000,
                risk_resume_drawdown_bp: resume,
                risk_pause_failed_buys: failures,
                adverse_move_threshold_bp: 500,
            };
            assert_eq!(InstitutionExperiencePolicy::try_from(policy), Err(expected));
            assert_eq!(
                InstitutionExperiencePolicy::new(
                    policy.policy_version,
                    policy.loss_response,
                    policy.cost_loss_threshold_bp,
                    policy.cost_profit_threshold_bp,
                    policy.risk_pause_drawdown_bp,
                    policy.risk_resume_drawdown_bp,
                    policy.risk_pause_failed_buys,
                    policy.adverse_move_threshold_bp,
                ),
                Err(expected)
            );
        }
    }

    struct FixedRng {
        draw: f64,
        calls: usize,
    }

    impl Rng for FixedRng {
        fn next_f64(&mut self) -> f64 {
            self.calls += 1;
            self.draw
        }

        fn next_range_u32(&mut self, low: u32, _high: u32) -> u32 {
            low
        }
    }

    #[test]
    fn sampling_is_style_bounded_freezable_and_allows_both_responses() {
        let styles = [
            (
                InstitutionStyle::DeepValue,
                (800, 1600),
                (2000, 4000),
                (3000, 5000),
                (3, 5),
            ),
            (
                InstitutionStyle::Growth,
                (600, 1200),
                (1200, 2400),
                (2000, 3500),
                (2, 4),
            ),
            (
                InstitutionStyle::Balanced,
                (500, 1000),
                (1000, 2000),
                (1800, 3000),
                (2, 4),
            ),
            (
                InstitutionStyle::Defensive,
                (300, 800),
                (600, 1400),
                (1000, 2000),
                (1, 3),
            ),
            (
                InstitutionStyle::ActiveTrader,
                (100, 500),
                (400, 1200),
                (800, 1800),
                (1, 3),
            ),
        ];
        for (style, loss, profit, pause, failures) in styles {
            let mut low_rng = FixedRng {
                draw: 0.0,
                calls: 0,
            };
            let low = InstitutionExperiencePolicy::sample(style, &mut low_rng);
            assert_eq!(low_rng.calls, 6);
            assert_eq!(low.policy_version(), 1);
            assert_eq!(low.loss_response(), InstitutionLossResponse::HoldOrAdd);
            assert_eq!(low.cost_loss_threshold_bp(), loss.0);
            assert_eq!(low.cost_profit_threshold_bp(), profit.0);
            assert_eq!(low.risk_pause_drawdown_bp(), pause.0);
            assert_eq!(low.risk_resume_drawdown_bp(), pause.0 / 2);
            assert_eq!(low.risk_pause_failed_buys(), failures.0);
            assert_eq!(low.adverse_move_threshold_bp(), 300);
            let frozen = serde_json::to_vec(&low).unwrap();
            let restored: InstitutionExperiencePolicy = serde_json::from_slice(&frozen).unwrap();
            assert_eq!(restored, low);

            let mut high_rng = FixedRng {
                draw: 0.999_999,
                calls: 0,
            };
            let high = InstitutionExperiencePolicy::sample(style, &mut high_rng);
            assert_eq!(high_rng.calls, 6);
            assert_eq!(
                high.loss_response(),
                InstitutionLossResponse::PauseAndReview
            );
            assert_eq!(high.cost_loss_threshold_bp(), loss.1);
            assert_eq!(high.cost_profit_threshold_bp(), profit.1);
            assert_eq!(high.risk_pause_drawdown_bp(), pause.1);
            assert_eq!(high.risk_resume_drawdown_bp(), pause.1 / 2);
            assert_eq!(high.risk_pause_failed_buys(), failures.1);
            assert_eq!(high.adverse_move_threshold_bp(), 1000);
        }
    }

    #[test]
    fn style_presets_are_explicit_midpoints_and_hold_by_default() {
        let expected = [
            (InstitutionStyle::DeepValue, 1200, 3000, 4000, 2000, 4, 650),
            (InstitutionStyle::Growth, 900, 1800, 2750, 1375, 3, 650),
            (InstitutionStyle::Balanced, 750, 1500, 2400, 1200, 3, 650),
            (InstitutionStyle::Defensive, 550, 1000, 1500, 750, 2, 650),
            (InstitutionStyle::ActiveTrader, 300, 800, 1300, 650, 2, 650),
        ];
        for (style, loss, profit, pause, resume, failures, adverse) in expected {
            let preset = InstitutionExperiencePolicy::default_for_style(style);
            assert_eq!(preset.policy_version(), 1);
            assert_eq!(preset.loss_response(), InstitutionLossResponse::HoldOrAdd);
            assert_eq!(preset.cost_loss_threshold_bp(), loss);
            assert_eq!(preset.cost_profit_threshold_bp(), profit);
            assert_eq!(preset.risk_pause_drawdown_bp(), pause);
            assert_eq!(preset.risk_resume_drawdown_bp(), resume);
            assert_eq!(preset.risk_pause_failed_buys(), failures);
            assert_eq!(preset.adverse_move_threshold_bp(), adverse);
        }
    }

    #[test]
    fn strict_serde_requires_all_eight_fields_and_rejects_invalid_values() {
        let mut rng = FixedRng {
            draw: 0.5,
            calls: 0,
        };
        let policy = InstitutionExperiencePolicy::sample(InstitutionStyle::Balanced, &mut rng);
        let value = serde_json::to_value(policy).unwrap();
        let fields = [
            "policy_version",
            "loss_response",
            "cost_loss_threshold_bp",
            "cost_profit_threshold_bp",
            "risk_pause_drawdown_bp",
            "risk_resume_drawdown_bp",
            "risk_pause_failed_buys",
            "adverse_move_threshold_bp",
        ];
        assert_eq!(value.as_object().unwrap().len(), fields.len());
        for field in fields {
            let mut missing = value.clone();
            missing.as_object_mut().unwrap().remove(field);
            assert!(
                serde_json::from_value::<InstitutionExperiencePolicy>(missing).is_err(),
                "{field}"
            );
        }
        for (field, invalid) in [
            ("policy_version", serde_json::json!(2)),
            ("loss_response", serde_json::json!("Sell")),
            ("cost_loss_threshold_bp", serde_json::json!(0)),
            ("cost_loss_threshold_bp", serde_json::json!(10001)),
            ("cost_profit_threshold_bp", serde_json::json!(0)),
            ("risk_pause_drawdown_bp", serde_json::json!(0)),
            ("risk_resume_drawdown_bp", serde_json::json!(10000)),
            ("risk_pause_failed_buys", serde_json::json!(0)),
            ("adverse_move_threshold_bp", serde_json::json!(10001)),
        ] {
            let mut malformed = value.clone();
            malformed
                .as_object_mut()
                .unwrap()
                .insert(field.to_owned(), invalid);
            assert!(
                serde_json::from_value::<InstitutionExperiencePolicy>(malformed).is_err(),
                "{field}"
            );
        }
        let mut extra = value;
        extra
            .as_object_mut()
            .unwrap()
            .insert("legacy_default".to_owned(), serde_json::json!(true));
        assert!(serde_json::from_value::<InstitutionExperiencePolicy>(extra).is_err());
    }
}

//! Versioned, validated urgency thresholds persisted with the simulation policy.

use super::UrgencyError;
use serde::{Deserialize, Deserializer};

pub const URGENCY_POLICY_VERSION: u32 = 1;

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize)]
pub struct UrgencyPolicy {
    pub policy_version: u32,
    pub drop_30min_threshold_bp: i32,
    pub drop_1min_threshold_bp: i32,
    pub urgent_drawdown_threshold_bp: u32,
    pub patient_confidence_threshold_bp: u32,
    pub urgent_remaining_trading_days: u32,
    pub resume_signal_threshold_bp: i32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawUrgencyPolicy {
    policy_version: u32,
    drop_30min_threshold_bp: i32,
    drop_1min_threshold_bp: i32,
    urgent_drawdown_threshold_bp: u32,
    patient_confidence_threshold_bp: u32,
    urgent_remaining_trading_days: u32,
    resume_signal_threshold_bp: i32,
}

impl Default for UrgencyPolicy {
    fn default() -> Self {
        Self {
            policy_version: URGENCY_POLICY_VERSION,
            drop_30min_threshold_bp: -300,
            drop_1min_threshold_bp: -75,
            urgent_drawdown_threshold_bp: 2000,
            patient_confidence_threshold_bp: 4000,
            urgent_remaining_trading_days: 1,
            resume_signal_threshold_bp: 2000,
        }
    }
}

impl<'de> Deserialize<'de> for UrgencyPolicy {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let raw = RawUrgencyPolicy::deserialize(deserializer)?;
        let policy = Self {
            policy_version: raw.policy_version,
            drop_30min_threshold_bp: raw.drop_30min_threshold_bp,
            drop_1min_threshold_bp: raw.drop_1min_threshold_bp,
            urgent_drawdown_threshold_bp: raw.urgent_drawdown_threshold_bp,
            patient_confidence_threshold_bp: raw.patient_confidence_threshold_bp,
            urgent_remaining_trading_days: raw.urgent_remaining_trading_days,
            resume_signal_threshold_bp: raw.resume_signal_threshold_bp,
        };
        policy.validate().map_err(serde::de::Error::custom)?;
        Ok(policy)
    }
}

impl UrgencyPolicy {
    pub(super) fn validate(&self) -> Result<(), UrgencyError> {
        if self.policy_version != URGENCY_POLICY_VERSION {
            return Err(UrgencyError::UnsupportedPolicyVersion {
                value: self.policy_version,
                expected: URGENCY_POLICY_VERSION,
            });
        }
        for (field, value, valid) in [
            (
                "drop_30min_threshold_bp",
                i64::from(self.drop_30min_threshold_bp),
                self.drop_30min_threshold_bp < 0,
            ),
            (
                "drop_1min_threshold_bp",
                i64::from(self.drop_1min_threshold_bp),
                self.drop_1min_threshold_bp < 0,
            ),
            (
                "urgent_drawdown_threshold_bp",
                i64::from(self.urgent_drawdown_threshold_bp),
                self.urgent_drawdown_threshold_bp <= 10_000,
            ),
            (
                "patient_confidence_threshold_bp",
                i64::from(self.patient_confidence_threshold_bp),
                self.patient_confidence_threshold_bp <= 10_000,
            ),
            (
                "urgent_remaining_trading_days",
                i64::from(self.urgent_remaining_trading_days),
                self.urgent_remaining_trading_days > 0,
            ),
            (
                "resume_signal_threshold_bp",
                i64::from(self.resume_signal_threshold_bp),
                (0..=10_000).contains(&self.resume_signal_threshold_bp),
            ),
        ] {
            if !valid {
                return Err(UrgencyError::InvalidPolicy { field, value });
            }
        }
        Ok(())
    }
}

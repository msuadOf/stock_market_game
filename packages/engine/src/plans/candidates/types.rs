#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct SignalScore(i32);

impl SignalScore {
    pub fn new(value: i32) -> Result<Self, CandidateError> {
        if !(-10_000..=10_000).contains(&value) {
            return Err(CandidateError::InvalidSignalScore { value });
        }
        Ok(Self(value))
    }

    pub(super) const fn bounded(value: i32) -> Self {
        debug_assert!(value >= -10_000 && value <= 10_000);
        Self(value)
    }

    pub const fn value(self) -> i32 {
        self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignalComponent {
    Fundamental,
    Trend,
    PriceVolume,
    Technical,
    ExperienceCost,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SignalUnavailableReason {
    ZeroWeight,
    FundamentalUnavailable,
    MissingThirtyMinuteReturn,
    MissingFiveDayReturn,
    MissingRelativeVolume,
    MissingOrderBookImbalance,
    MissingSma20,
    MissingSma60,
    MissingRsi14,
    MissingObservation,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExcludedSignal {
    pub component: SignalComponent,
    pub reason: SignalUnavailableReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignalContribution {
    pub(super) score: Option<SignalScore>,
    pub(super) unavailable_atoms: Vec<SignalUnavailableReason>,
}

impl SignalContribution {
    pub fn available(score: SignalScore) -> Self {
        Self {
            score: Some(score),
            unavailable_atoms: Vec::new(),
        }
    }

    pub fn unavailable(reason: SignalUnavailableReason) -> Self {
        Self {
            score: None,
            unavailable_atoms: vec![reason],
        }
    }

    pub const fn score(&self) -> Option<SignalScore> {
        self.score
    }

    pub fn unavailable_atoms(&self) -> &[SignalUnavailableReason] {
        &self.unavailable_atoms
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateSignals {
    pub fundamental: SignalContribution,
    pub trend: SignalContribution,
    pub price_volume: SignalContribution,
    pub technical: SignalContribution,
    pub experience: SignalContribution,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CandidateAssessment {
    Scored {
        score: SignalScore,
        used_weight_bp: u32,
        excluded: Vec<ExcludedSignal>,
    },
    InsufficientInformation {
        excluded: Vec<ExcludedSignal>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum CandidateError {
    #[error("signal score {value} is outside -10000..=10000")]
    InvalidSignalScore { value: i32 },
    #[error("normalization threshold must be positive, got {threshold}")]
    InvalidThreshold { threshold: i64 },
    #[error("{field} must be positive, got {cents} cents")]
    NonPositiveMoney { field: &'static str, cents: i64 },
    #[error("fundamental range is inverted: {low_cents}..{high_cents} cents")]
    InvertedValuationRange { low_cents: i64, high_cents: i64 },
    #[error("position fraction {value} bp is outside 0..=10000")]
    InvalidPositionFraction { value: u32 },
    #[error("board-lot size {lot_size} is invalid for mainland A shares; expected 100")]
    InvalidBoardLotSize { lot_size: u32 },
    #[error("checked arithmetic overflow while computing {step}")]
    ArithmeticOverflow { step: &'static str },
}

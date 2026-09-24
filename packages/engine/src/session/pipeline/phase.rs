/// Tick phase vocabulary. Ranks never depend on declaration order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TickPhase {
    ExpiryShadow,
    SealAllocationSnapshot,
    DecisionShadow,
    AccountValidation,
    StockProcessing,
    ReceiptAggregation,
    SettlementShadow,
    DerivationAudit,
    PreCommitValidation,
    CommitTick,
}

impl TickPhase {
    pub const ALL: [Self; 10] = [
        Self::ExpiryShadow,
        Self::SealAllocationSnapshot,
        Self::DecisionShadow,
        Self::AccountValidation,
        Self::StockProcessing,
        Self::ReceiptAggregation,
        Self::SettlementShadow,
        Self::DerivationAudit,
        Self::PreCommitValidation,
        Self::CommitTick,
    ];

    pub const fn rank(self) -> u8 {
        match self {
            Self::ExpiryShadow => 0,
            Self::SealAllocationSnapshot => 1,
            Self::DecisionShadow => 2,
            Self::AccountValidation => 3,
            Self::StockProcessing => 4,
            Self::ReceiptAggregation => 5,
            Self::SettlementShadow => 6,
            Self::DerivationAudit => 7,
            Self::PreCommitValidation => 8,
            Self::CommitTick => 9,
        }
    }

    pub const fn name(self) -> &'static str {
        match self {
            Self::ExpiryShadow => "expiry_shadow",
            Self::SealAllocationSnapshot => "seal_allocation_snapshot",
            Self::DecisionShadow => "decision_shadow",
            Self::AccountValidation => "account_validation",
            Self::StockProcessing => "stock_processing",
            Self::ReceiptAggregation => "receipt_aggregation",
            Self::SettlementShadow => "settlement_shadow",
            Self::DerivationAudit => "derivation_audit",
            Self::PreCommitValidation => "pre_commit_validation",
            Self::CommitTick => "commit_tick",
        }
    }
}

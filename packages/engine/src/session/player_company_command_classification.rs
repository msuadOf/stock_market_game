//! 玩家提案的 typed 分类；制度判断继续由共同 approve 执行。

use super::PlayerCompanyCommandIssue;
use crate::company::CompanyErrorClass::{self, *};
use crate::session::company_command_candidates::{
    CashDividendApprovalIssue, CompanyCommandCause, CompanyCommandError,
};
use crate::session::player_company_command_candidates::{
    PlayerCommandCause, PlayerCommandError, PlayerCommandIssue,
};
use crate::session::stock_distribution_command_candidate::{
    StockDistributionApprovalCause, StockDistributionApprovalError, StockDistributionApprovalIssue,
};

pub(super) fn command_issue(issue: PlayerCompanyCommandIssue) -> CompanyErrorClass {
    use PlayerCompanyCommandIssue::*;
    match issue {
        MissingCompanySystem | PendingCommandImplementation => UnsupportedOperation,
        UnknownIssuer | UnlistedIssuer => InvalidInput,
        MissingSecurity
        | CandidateConfigurationMismatch
        | CandidateIssuerMismatch
        | RevisionExhausted
        | RevisionChanged
        | RuntimeLockPoisoned
        | RuntimeTickMismatch => SystemState,
    }
}

pub(super) fn player(error: &PlayerCommandError) -> CompanyErrorClass {
    use PlayerCommandCause::*;
    match error.cause.as_ref() {
        Validation(issue) => match issue {
            PlayerCommandIssue::UnknownIssuer
            | PlayerCommandIssue::NotListed
            | PlayerCommandIssue::NonPositiveGross
            | PlayerCommandIssue::NonPositiveRatio
            | PlayerCommandIssue::NonPositiveRepurchaseWindow => InvalidInput,
            PlayerCommandIssue::NoNewShares => BusinessCondition,
            PlayerCommandIssue::RightsOfferingDisabled => UnsupportedOperation,
            PlayerCommandIssue::UnexpectedProposalKind
            | PlayerCommandIssue::UnknownStock
            | PlayerCommandIssue::StockMismatch
            | PlayerCommandIssue::MissingRegistry
            | PlayerCommandIssue::ShareCountOverflow => SystemState,
        },
        NoHolding { .. } => BusinessCondition,
        InvalidSecurityCode { .. } | Accounting(_) => SystemState,
        Calendar(_) => InvalidInput,
        Company(error) => error.classification(),
        Plan(error) => cash(error),
        Approval(error) => cash_approval(error),
        StockApproval(error) => stock_approval(error),
        RightsPlan(error) => rights(error),
        SplitPlan(error) => split(error),
        RepurchasePlan(error) => {
            use crate::company::issuer_repurchase::IssuerRepurchaseError::*;
            match error {
                InvalidPlan { .. } | InvalidTradingDate { .. } => InvalidInput,
                CalendarLookup(_)
                | WrongStage { .. }
                | FillExceedsBudget { .. }
                | CancellationExceedsFills { .. }
                | CancellationExceedsInventory { .. }
                | FillExceedsExecutionCap { .. }
                | UnitReplay { .. } => SystemState,
            }
        }
    }
}

fn cash(error: &crate::company::cash_dividend::CashDividendError) -> CompanyErrorClass {
    use crate::company::cash_dividend::CashDividendError::*;
    match error {
        InvalidPlan { .. } | InvalidTradingDate { .. } | InvalidExDividendDate { .. } => {
            InvalidInput
        }
        ExceedsDistributableAmount { .. } | NoEligibleHolders => BusinessCondition,
        WrongStage { .. }
        | SnapshotMismatch { .. }
        | Overflow { .. }
        | CalendarLookup { .. }
        | IncompletePaymentResults { .. }
        | UnexpectedPaymentHolder { .. }
        | PaymentAmountMismatch { .. }
        | MissingFailureReason { .. }
        | PaymentIdentityConflict { .. }
        | PaymentDateRegression { .. }
        | AlreadyPaid
        | ShareRegistry(_) => SystemState,
    }
}

fn cash_approval(error: &CompanyCommandError) -> CompanyErrorClass {
    match error.cause.as_ref() {
        CompanyCommandCause::Company(error) => error.classification(),
        CompanyCommandCause::Plan(error) => cash(error),
        CompanyCommandCause::Validation(issue) => match issue {
            CashDividendApprovalIssue::NoEligibleShares
            | CashDividendApprovalIssue::AmountExceedsPlan
            | CashDividendApprovalIssue::ConflictingIdentity => BusinessCondition,
            CashDividendApprovalIssue::UnsupportedRightsCombination => UnsupportedOperation,
            CashDividendApprovalIssue::UnsupportedFormula => UnsupportedOperation,
            CashDividendApprovalIssue::DateMismatch
            | CashDividendApprovalIssue::ExchangeMismatch
            | CashDividendApprovalIssue::UnknownIssuer
            | CashDividendApprovalIssue::StockMismatch
            | CashDividendApprovalIssue::MissingRegistry
            | CashDividendApprovalIssue::DuplicateRegistry
            | CashDividendApprovalIssue::RegistryMismatch
            | CashDividendApprovalIssue::FutureRegistry
            | CashDividendApprovalIssue::GrossOverflow
            | CashDividendApprovalIssue::MissingLegalFacts => SystemState,
        },
        CompanyCommandCause::Registry(_)
        | CompanyCommandCause::Accounting(_)
        | CompanyCommandCause::Money(_) => SystemState,
    }
}

fn stock_approval(error: &StockDistributionApprovalError) -> CompanyErrorClass {
    match error.cause.as_ref() {
        StockDistributionApprovalCause::Company(error) => error.classification(),
        StockDistributionApprovalCause::CapitalTransition(error) => capital(error),
        StockDistributionApprovalCause::Validation(issue) => match issue {
            StockDistributionApprovalIssue::SameExDateDistribution
            | StockDistributionApprovalIssue::SameExDateRights
            | StockDistributionApprovalIssue::SameExDateSplit => UnsupportedOperation,
            StockDistributionApprovalIssue::ConflictingIdentity
            | StockDistributionApprovalIssue::ConflictingRegistrationIdentity
            | StockDistributionApprovalIssue::NonIntegralPar => BusinessCondition,
            StockDistributionApprovalIssue::DateMismatch
            | StockDistributionApprovalIssue::ExchangeMismatch
            | StockDistributionApprovalIssue::UnknownIssuer
            | StockDistributionApprovalIssue::StockMismatch
            | StockDistributionApprovalIssue::MissingRegistry
            | StockDistributionApprovalIssue::DuplicateRegistry
            | StockDistributionApprovalIssue::RegistryMismatch
            | StockDistributionApprovalIssue::FutureRegistry
            | StockDistributionApprovalIssue::MissingLegalFacts
            | StockDistributionApprovalIssue::NonPositiveOrOverflowPar
            | StockDistributionApprovalIssue::CapitalOverflow
            | StockDistributionApprovalIssue::MissingDeclaredFact
            | StockDistributionApprovalIssue::CapitalContextMismatch => SystemState,
        },
        StockDistributionApprovalCause::Plan(error) => {
            use crate::company::stock_distribution::StockDistributionError::*;
            match error {
                InvalidPlan { .. } | InvalidTradingDate { .. } | InvalidExRightsDate { .. } => {
                    InvalidInput
                }
                ApprovedTotalInfeasible { .. } => BusinessCondition,
                SourceLotAttributionConflict { .. } => UnsupportedOperation,
                ArithmeticOverflow
                | WrongStage { .. }
                | SnapshotMismatch { .. }
                | CalendarLookup { .. }
                | ShareRegistry(_) => SystemState,
            }
        }
        StockDistributionApprovalCause::Registry(_)
        | StockDistributionApprovalCause::Accounting(_) => SystemState,
    }
}

pub(super) fn capital(
    error: &crate::session::capital_command_candidates::CapitalCommandError,
) -> CompanyErrorClass {
    use crate::session::capital_command_candidates::CapitalCommandError::*;
    match error {
        Company(error) => error.classification(),
        Rights(error) => rights(error),
        Split(error) => split(error),
        Duplicate { .. } | InvalidSplitPar { .. } => BusinessCondition,
        SameExDate { .. } | PendingShareUnitTransition { .. } => UnsupportedOperation,
        Capability(error) => match error {
            crate::session::corporate_actions::rights_offering_day_end::RightsOfferingCandidateError::UnsupportedLock { .. } => UnsupportedOperation,
            crate::session::corporate_actions::rights_offering_day_end::RightsOfferingCandidateError::MissingAccount { .. }
            | crate::session::corporate_actions::rights_offering_day_end::RightsOfferingCandidateError::NegativeCash { .. }
            | crate::session::corporate_actions::rights_offering_day_end::RightsOfferingCandidateError::Domain { .. }
            | crate::session::corporate_actions::rights_offering_day_end::RightsOfferingCandidateError::Invalid { .. } => SystemState,
        },
        ContextMismatch => SystemState,
    }
}

fn rights(error: &crate::company::rights_offering::RightsOfferingError) -> CompanyErrorClass {
    use crate::company::rights_offering::RightsOfferingError::*;
    match error {
        InvalidPlan { .. }
        | InvalidTradingDate { .. }
        | InvalidExRightsDate { .. }
        | InvalidSettlementDate { .. } => InvalidInput,
        EntitlementTotalInfeasible { .. } => BusinessCondition,
        SubscriptionStrategyUnimplemented { .. } => UnsupportedOperation,
        ArithmeticOverflow | CalendarLookup(_) | SnapshotMismatch { .. } | ShareRegistry(_) => {
            SystemState
        }
    }
}

fn split(error: &crate::company::share_split::ShareSplitError) -> CompanyErrorClass {
    use crate::company::share_split::ShareSplitError::*;
    match error {
        InvalidPlan { .. } | InvalidTradingDate { .. } | InvalidExRightsDate { .. } => InvalidInput,
        ZeroIssuedShares { .. } => BusinessCondition,
        ArithmeticOverflow
        | WrongStage { .. }
        | SnapshotMismatch { .. }
        | CalendarLookup { .. }
        | ShareRegistry(_) => SystemState,
    }
}

pub(super) fn repurchase_session(error: &crate::session::SessionError) -> CompanyErrorClass {
    use crate::session::RepurchaseApprovalError::*;
    match error {
        crate::session::SessionError::RepurchaseApproval(error) => match error {
            Disabled => UnsupportedOperation,
            InvalidSource { .. } | SourceIssuerMismatch { .. } => InvalidInput,
            InsufficientHolding { .. }
            | EventConflict { .. }
            | PlanConflict { .. }
            | ShareLimit { .. } => BusinessCondition,
            InvalidPlan { .. } | MissingBinding { .. } | Transaction { .. } => SystemState,
            #[cfg(test)]
            InjectedFailure { .. } => SystemState,
        },
        // 共同批准的其它 Session 错误属于其事务内部，不承诺业务恢复。
        _ => SystemState,
    }
}

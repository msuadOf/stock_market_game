//! 三宿主共用的公司写命令错误投影；Rust cause 保持原类型。

use crate::session::{PlayerCompanyCommandError, ReportCorrectionError, RightsSubscriptionError};

#[derive(Debug, thiserror::Error)]
pub enum CompanyWriteError {
    #[error("{0}")]
    PlayerProposal(#[from] PlayerCompanyCommandError),
    #[error("{0}")]
    ReportCorrection(#[from] ReportCorrectionError),
    #[error("配股认购受理失败：{source}")]
    RightsSubscription {
        account: crate::AccountId,
        event_id: String,
        #[source]
        source: RightsSubscriptionError,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum CompanyWriteFailure {
    PlayerProposal {
        account: crate::AccountId,
        company: crate::company::CompanyId,
        kind: crate::session::PlayerProposalKind,
        day: crate::CivilDate,
        classification: crate::company::CompanyErrorClass,
        reason: String,
    },
    ReportCorrection(ReportCorrectionFailure),
    RightsSubscription {
        account: crate::AccountId,
        event_id: String,
        classification: crate::company::CompanyErrorClass,
        reason: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "kind", content = "data")]
pub enum ReportCorrectionFailure {
    Step(crate::session::StepFatal),
    StaleEpoch,
    OperationConflict {
        operation_id: String,
        completed: bool,
    },
    NotPending {
        operation_id: String,
    },
    AlreadyCompleted {
        operation_id: String,
    },
    EmptyOperationId,
    EmptyReason,
    EmptyEntries,
    Journal {
        reason: String,
    },
    FutureEntry {
        source_id: crate::accounting::BusinessEventId,
        date: crate::CivilDate,
        current: crate::CivilDate,
    },
    RevisionExhausted,
}

impl CompanyWriteError {
    /// 只识别真实错误链中的 StepFatal；宿主还须检查本次命令后的健康门。
    pub fn step_fatal(&self) -> Option<&crate::session::StepFatal> {
        let mut current: Option<&(dyn std::error::Error + 'static)> = Some(self);
        while let Some(error) = current {
            if let Some(fatal) = error.downcast_ref::<crate::session::StepFatal>() {
                return Some(fatal);
            }
            current = error.source();
        }
        None
    }

    pub fn public_failure(&self) -> CompanyWriteFailure {
        match self {
            Self::PlayerProposal(error) => CompanyWriteFailure::PlayerProposal {
                account: error.account,
                company: error.company,
                kind: error.kind,
                day: error.day,
                classification: error.classification(),
                reason: if error.classification() == crate::company::CompanyErrorClass::SystemState
                {
                    "公司提案因内部状态校验失败而拒绝，请反馈当前公司、提案类别和日期".into()
                } else {
                    error.to_string()
                },
            },
            Self::RightsSubscription {
                account,
                event_id,
                source,
            } => {
                use RightsSubscriptionError as E;
                let classification = match source {
                    E::Disabled | E::UnsupportedRuntime | E::UnsupportedPlan { .. } => {
                        crate::company::CompanyErrorClass::UnsupportedOperation
                    }
                    E::InvalidQuantity | E::UnknownAccount { .. } => {
                        crate::company::CompanyErrorClass::InvalidInput
                    }
                    E::UnknownEvent { .. }
                    | E::OutsidePaymentWindow { .. }
                    | E::NoEntitlement { .. }
                    | E::ExceedsEntitlement { .. }
                    | E::Duplicate { .. } => crate::company::CompanyErrorClass::BusinessCondition,
                    E::UnsupportedTaxMode | E::UnsupportedSettlementScope { .. } => {
                        crate::company::CompanyErrorClass::UnsupportedOperation
                    }
                    E::UnsupportedAccountKind => crate::company::CompanyErrorClass::InvalidInput,
                    E::RevisionExhausted => crate::company::CompanyErrorClass::SystemState,
                    E::MissingCompanyState | E::Capability { .. } | E::Step(_) => {
                        crate::company::CompanyErrorClass::SystemState
                    }
                };
                CompanyWriteFailure::RightsSubscription {
                    account: *account,
                    event_id: event_id.clone(),
                    classification,
                    reason: source.to_string(),
                }
            }
            Self::ReportCorrection(error) => {
                use ReportCorrectionError as E;
                use ReportCorrectionFailure as F;
                CompanyWriteFailure::ReportCorrection(match error {
                    E::Step(fatal) => F::Step(fatal.clone()),
                    E::StaleEpoch => F::StaleEpoch,
                    E::OperationConflict {
                        operation_id,
                        completed,
                    } => F::OperationConflict {
                        operation_id: operation_id.clone(),
                        completed: *completed,
                    },
                    E::NotPending { operation_id } => F::NotPending {
                        operation_id: operation_id.clone(),
                    },
                    E::AlreadyCompleted { operation_id } => F::AlreadyCompleted {
                        operation_id: operation_id.clone(),
                    },
                    E::EmptyOperationId => F::EmptyOperationId,
                    E::EmptyReason => F::EmptyReason,
                    E::EmptyEntries => F::EmptyEntries,
                    E::Journal { .. } => F::Journal {
                        reason: "更正凭证不满足会计不变量，请检查借贷方向、金额及科目".into(),
                    },
                    E::FutureEntry {
                        source_id,
                        date,
                        current,
                    } => F::FutureEntry {
                        source_id: *source_id,
                        date: *date,
                        current: *current,
                    },
                    E::RevisionExhausted => F::RevisionExhausted,
                })
            }
        }
    }
}

/// 只确认该更正命令被受理；不证明重新入队、发布或已完成。
#[derive(Debug, serde::Serialize)]
pub struct ReportCorrectionAcknowledgement {
    pub operation_id: String,
}

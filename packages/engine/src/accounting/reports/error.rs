//! 报表域错误（K3，任务 13）。类型化、绝不静默；透传变体装箱压缩 Err
//! 值域（任务 12 `DeclaredSide` 先例——错误路径非热路径）。

use crate::accounting::amount::AccountingAmount;
use crate::accounting::consolidation::ConsolidationError;
use crate::accounting::error::AccountingError;
use crate::accounting::ledger::LedgerAccountId;
use crate::accounting::period::AccountingPeriod;
use thiserror::Error;

/// 报表生成/校验失败。
#[derive(Debug, Error)]
pub enum ReportError {
    #[error("accounting failure: {0}")]
    Accounting(Box<AccountingError>),
    #[error("consolidation failure: {0}")]
    Consolidation(Box<ConsolidationError>),
    #[error("account {code} has no statement classification")]
    UnclassifiedAccount { code: LedgerAccountId },
    #[error("duplicate classification for {code}: {first} vs {second}")]
    DuplicateClassification {
        code: LedgerAccountId,
        first: &'static str,
        second: &'static str,
    },
    #[error(
        "notes cross-foot mismatch on {line}: statement {statement_total} vs notes {notes_total}"
    )]
    NotesCrossFootMismatch {
        line: &'static str,
        statement_total: AccountingAmount,
        notes_total: AccountingAmount,
    },
    #[error(
        "balance sheet unbalanced: assets {assets} vs liabilities+equity {liabilities_equity}"
    )]
    BalanceSheetNotBalanced {
        assets: AccountingAmount,
        liabilities_equity: AccountingAmount,
    },
    #[error(
        "equity cross-foot mismatch: opening+changes {opening_plus_changes} vs closing {closing}"
    )]
    EquityCrossFootMismatch {
        opening_plus_changes: AccountingAmount,
        closing: AccountingAmount,
    },
    #[error("cash flow cross-foot mismatch: {opening_plus_changes} vs closing {closing}")]
    CashFlowCrossFootMismatch {
        opening_plus_changes: AccountingAmount,
        closing: AccountingAmount,
    },
    #[error("indirect reconciliation mismatch: direct {direct} vs indirect {indirect}")]
    IndirectReconciliationMismatch {
        direct: AccountingAmount,
        indirect: AccountingAmount,
    },
    #[error("trial balance unbalanced: debits {total_debits} vs credits {total_credits}")]
    TrialBalanceUnbalanced {
        total_debits: AccountingAmount,
        total_credits: AccountingAmount,
    },
    #[error("comparative fabricated at {location}: prior window has no history")]
    ComparativeFabricated { location: &'static str },
    #[error("invalid report kind at {period}: {reason}")]
    InvalidReportKind {
        period: AccountingPeriod,
        reason: &'static str,
    },
    #[error("consolidated income mismatch: derived {derived} vs task-12 output {output}")]
    IncomeConsistencyMismatch {
        derived: AccountingAmount,
        output: AccountingAmount,
    },
    #[error("internal window inconsistency: {detail}")]
    InternalWindowInconsistent { detail: String },
    #[error("restatement is not supported for consolidated scopes")]
    ConsolidatedRestatementUnsupported,
}

/// `?` 直转（装箱由 From 承担——thiserror 的 `#[from]` 不覆盖值→箱路径）。
impl From<AccountingError> for ReportError {
    fn from(err: AccountingError) -> Self {
        ReportError::Accounting(Box::new(err))
    }
}

/// `?` 直转（同上）。
impl From<ConsolidationError> for ReportError {
    fn from(err: ConsolidationError) -> Self {
        ReportError::Consolidation(Box::new(err))
    }
}

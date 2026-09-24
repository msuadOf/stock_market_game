//! Atomic P6 settlement plus retail-experience projection.

use super::retail_projection::{
    canonical_unseen_receipts, project_retail_receipts, RetailProjectionError,
    RetailProjectionInput, RetailProjectionSeen, RetailReceiptEvent,
};
use super::settlement::{prepare_receipt_settlements, SettlementApplication};
use super::{EnvelopeReceipt, StepFatal};
use crate::session::{account_book::AccountBook, account_paged_map::AccountPagedMap};
use crate::{
    Account, AccountId, AccountKind, GameSession, Position, RetailExperienceState, StockCode,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct P6TransactionOutput {
    pub(super) settlement: SettlementApplication,
    pub(super) events: Vec<RetailReceiptEvent>,
}

pub(super) struct PreparedP6Transaction {
    pub(super) account_patch: BTreeMap<AccountId, Account>,
    pub(super) retail_patch: BTreeMap<AccountId, RetailExperienceState>,
    pub(super) seen: RetailProjectionSeen,
    pub(super) output: P6TransactionOutput,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum P6TransactionError {
    #[error(transparent)]
    Settlement(#[from] StepFatal),
    #[error(transparent)]
    Projection(#[from] RetailProjectionError),
}

/// Applies P6 directly to the three replay-sensitive containers owned by a
/// prospective session shadow. Preparation touches only settled accounts and
/// commits after the retail projection also succeeds.
pub(super) fn apply_session_p6_transaction(
    session: &mut GameSession,
    receipts: &[EnvelopeReceipt],
) -> Result<P6TransactionOutput, P6TransactionError> {
    let market_minute = session.current_market_minute();
    let t1_enabled = session.setup.t1_enabled;
    apply_p6_transaction(
        &mut session.accounts,
        &mut session.retail_experience,
        &mut session.retail_projection_seen,
        market_minute,
        receipts,
        t1_enabled,
    )
}

/// Runs settlement and retail projection on private shadows and commits all
/// three authoritative containers together only after both phases succeed.
pub(super) fn apply_p6_transaction(
    accounts: &mut AccountBook,
    retail_experience: &mut AccountPagedMap<crate::RetailExperienceState>,
    seen: &mut RetailProjectionSeen,
    market_minute: u64,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<P6TransactionOutput, P6TransactionError> {
    let prepared = prepare_p6_transaction(
        accounts,
        retail_experience,
        seen,
        market_minute,
        receipts,
        t1_enabled,
    )?;
    accounts.extend(prepared.account_patch);
    retail_experience.extend(prepared.retail_patch);
    *seen = prepared.seen;
    Ok(prepared.output)
}

/// Produces a detached P6 patch so P4-P7 need not clone every account before
/// asking P6 to clone the accounts touched by this receipt batch.
pub(super) fn prepare_p6_transaction(
    accounts: &AccountBook,
    retail_experience: &AccountPagedMap<crate::RetailExperienceState>,
    seen: &RetailProjectionSeen,
    market_minute: u64,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<PreparedP6Transaction, P6TransactionError> {
    let canonical: Vec<EnvelopeReceipt> = canonical_unseen_receipts(receipts, seen)?
        .into_iter()
        .cloned()
        .collect();
    let (account_shadow, settlement) =
        prepare_receipt_settlements(accounts, &canonical, t1_enabled)?;
    // Source observation and P6 fills prune their own changed accounts. Saves
    // reject overfull unheld watchlists, so an empty receipt batch has no
    // retail account to visit or repair.
    let retail_accounts: BTreeSet<AccountId> = canonical
        .iter()
        .filter(|receipt| receipt.kind == super::ReceiptKind::Fill)
        .filter_map(|receipt| {
            accounts
                .get(&receipt.envelope.account)
                .filter(|account| account.kind == AccountKind::Retail)
                .map(|_| receipt.envelope.account)
        })
        .collect();
    let positions_before = positions_of_retail(accounts, &retail_accounts);
    let mut positions_after = positions_before.clone();
    for (account_id, account) in &account_shadow {
        if retail_accounts.contains(account_id) {
            positions_after.insert(*account_id, account.positions.clone());
        }
    }
    let projection = project_retail_receipts(RetailProjectionInput {
        retail_experience,
        retail_accounts: &retail_accounts,
        positions_before: &positions_before,
        positions_after: &positions_after,
        seen,
        market_minute,
        receipts: &canonical,
    })?;

    Ok(PreparedP6Transaction {
        account_patch: account_shadow,
        retail_patch: projection.retail_experience,
        seen: projection.seen,
        output: P6TransactionOutput {
            settlement,
            events: projection.events,
        },
    })
}

fn positions_of_retail(
    accounts: &AccountBook,
    retail_accounts: &BTreeSet<AccountId>,
) -> BTreeMap<AccountId, BTreeMap<StockCode, Position>> {
    retail_accounts
        .iter()
        .filter_map(|account_id| {
            accounts
                .get(account_id)
                .map(|account| (*account_id, account.positions.clone()))
        })
        .collect()
}

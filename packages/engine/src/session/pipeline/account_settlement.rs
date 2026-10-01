//! Atomic Settlement settlement plus retail-experience projection.

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
pub(super) struct SettlementTransactionOutput {
    pub(super) settlement: SettlementApplication,
    pub(super) events: Vec<RetailReceiptEvent>,
}

pub(super) struct PreparedSettlementTransaction {
    pub(super) account_patch: BTreeMap<AccountId, Account>,
    pub(super) retail_patch: BTreeMap<AccountId, RetailExperienceState>,
    pub(super) seen: RetailProjectionSeen,
    pub(super) output: SettlementTransactionOutput,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum SettlementTransactionError {
    #[error(transparent)]
    Settlement(#[from] StepFatal),
    #[error(transparent)]
    Projection(#[from] RetailProjectionError),
}

/// Applies Settlement directly to the three replay-sensitive containers owned by a
/// prospective session shadow. Preparation touches only settled accounts and
/// commits after the retail projection also succeeds.
pub(super) fn apply_session_settlement_transaction(
    session: &mut GameSession,
    receipts: &[EnvelopeReceipt],
) -> Result<SettlementTransactionOutput, SettlementTransactionError> {
    let market_minute = session.current_market_minute();
    let t1_enabled = session.setup.t1_enabled;
    apply_settlement_transaction(
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
pub(super) fn apply_settlement_transaction(
    accounts: &mut AccountBook,
    retail_experience: &mut AccountPagedMap<crate::RetailExperienceState>,
    seen: &mut RetailProjectionSeen,
    market_minute: u64,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<SettlementTransactionOutput, SettlementTransactionError> {
    let prepared = prepare_settlement_transaction(
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

/// Produces a detached Settlement patch so P4-Projection need not clone every account before
/// asking Settlement to clone the accounts touched by this receipt batch.
pub(super) fn prepare_settlement_transaction(
    accounts: &AccountBook,
    retail_experience: &AccountPagedMap<crate::RetailExperienceState>,
    seen: &RetailProjectionSeen,
    market_minute: u64,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<PreparedSettlementTransaction, SettlementTransactionError> {
    let canonical: Vec<EnvelopeReceipt> = canonical_unseen_receipts(receipts, seen)?
        .into_iter()
        .cloned()
        .collect();
    let (account_shadow, settlement) =
        prepare_receipt_settlements(accounts, &canonical, t1_enabled)?;
    // Source observation and Settlement fills prune their own changed accounts. Saves
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

    Ok(PreparedSettlementTransaction {
        account_patch: account_shadow,
        retail_patch: projection.retail_experience,
        seen: projection.seen,
        output: SettlementTransactionOutput {
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

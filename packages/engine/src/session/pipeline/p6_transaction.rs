//! Atomic P6 settlement plus retail-experience projection.

use super::retail_projection::{
    canonical_unseen_receipts, project_retail_receipts, RetailProjectionError,
    RetailProjectionInput, RetailProjectionSeen, RetailReceiptEvent,
};
use super::settlement::{apply_receipt_settlements, SettlementApplication};
use super::{EnvelopeReceipt, StepFatal};
use crate::{
    Account, AccountId, AccountKind, GameSession, Position, RetailExperienceState, StockCode,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct P6TransactionOutput {
    pub(super) settlement: SettlementApplication,
    pub(super) events: Vec<RetailReceiptEvent>,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum P6TransactionError {
    #[error(transparent)]
    Settlement(#[from] StepFatal),
    #[error(transparent)]
    Projection(#[from] RetailProjectionError),
}

/// Applies P6 directly to the three replay-sensitive containers owned by a
/// prospective session shadow. The underlying transaction clones accounts and
/// projection state before committing, so an error leaves the shadow unchanged.
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
    accounts: &mut BTreeMap<AccountId, Account>,
    retail_experience: &mut BTreeMap<AccountId, RetailExperienceState>,
    seen: &mut RetailProjectionSeen,
    market_minute: u64,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<P6TransactionOutput, P6TransactionError> {
    let canonical: Vec<EnvelopeReceipt> = canonical_unseen_receipts(receipts, seen)?
        .into_iter()
        .cloned()
        .collect();
    let retail_accounts: BTreeSet<AccountId> = accounts
        .iter()
        .filter_map(|(account_id, account)| {
            (account.kind == AccountKind::Retail).then_some(*account_id)
        })
        .collect();
    let positions_before = positions_of(accounts);
    let mut account_shadow = clone_accounts(accounts)?;
    let settlement = apply_receipt_settlements(&mut account_shadow, &canonical, t1_enabled)?;
    let positions_after = positions_of(&account_shadow);
    let projection = project_retail_receipts(RetailProjectionInput {
        retail_experience,
        retail_accounts: &retail_accounts,
        positions_before: &positions_before,
        positions_after: &positions_after,
        seen,
        market_minute,
        receipts: &canonical,
    })?;

    *accounts = account_shadow;
    *retail_experience = projection.retail_experience;
    *seen = projection.seen;
    Ok(P6TransactionOutput {
        settlement,
        events: projection.events,
    })
}

fn clone_accounts(
    accounts: &BTreeMap<AccountId, Account>,
) -> Result<BTreeMap<AccountId, Account>, StepFatal> {
    accounts
        .iter()
        .map(|(account_id, account)| {
            account
                .clone_for_shadow()
                .map(|shadow| (*account_id, shadow))
                .map_err(|error| StepFatal::InvariantViolation {
                    description: format!("could not clone P6 account shadow: {error}"),
                    location: "pipeline::p6_transaction".to_owned(),
                })
        })
        .collect()
}

fn positions_of(
    accounts: &BTreeMap<AccountId, Account>,
) -> BTreeMap<AccountId, BTreeMap<StockCode, Position>> {
    accounts
        .iter()
        .map(|(account_id, account)| (*account_id, account.positions.clone()))
        .collect()
}

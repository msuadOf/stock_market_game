//! 原子准备 Settlement 及经验 projection，成功后一起采纳账户 patch。

use super::institutional_experience_projection::project_institutional_experience;
use super::retail_projection::{
    canonical_unseen_receipts, project_retail_receipts, RetailProjectionError,
    RetailProjectionInput, RetailProjectionSeen, RetailReceiptEvent,
};
use super::settlement::{ReceiptSettlementPlan, SettlementApplication};
use super::{EnvelopeReceipt, StepFatal};
use crate::session::decision_chain::personal_state::BeliefParticipantState;
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
    pub(super) belief_patch: BTreeMap<AccountId, crate::strategy::BeliefBook>,
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

/// 在 session shadow 的重放敏感容器中应用 Settlement。
/// 只准备实际成交账户，两个经验 projection 均成功后再一起提交。
pub(super) fn apply_session_settlement_transaction(
    session: &mut GameSession,
    receipts: &[EnvelopeReceipt],
) -> Result<SettlementTransactionOutput, SettlementTransactionError> {
    let moment = crate::experience::ExperienceMoment {
        civil_date: session.civil_date(),
        market_minute: session.current_market_minute(),
        trading_day: u64::from(session.state.day),
    };
    let t1_enabled = session.state.setup.t1_enabled;
    apply_settlement_transaction_with_beliefs(
        &mut session.state.accounts,
        &mut session.state.retail_experience,
        &mut session.state.belief_participants,
        &mut session.state.retail_projection_seen,
        moment,
        receipts,
        t1_enabled,
    )
}

/// 在私有 shadow 中结算和投影；全部阶段成功后一起提交受影响的权威容器。
#[cfg(test)]
pub(super) fn apply_settlement_transaction(
    accounts: &mut AccountBook,
    retail_experience: &mut AccountPagedMap<crate::RetailExperienceState>,
    seen: &mut RetailProjectionSeen,
    market_minute: u64,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<SettlementTransactionOutput, SettlementTransactionError> {
    let mut belief_participants = AccountPagedMap::default();
    apply_settlement_transaction_with_beliefs(
        accounts,
        retail_experience,
        &mut belief_participants,
        seen,
        test_experience_moment(market_minute),
        receipts,
        t1_enabled,
    )
}

pub(super) fn apply_settlement_transaction_with_beliefs(
    accounts: &mut AccountBook,
    retail_experience: &mut AccountPagedMap<crate::RetailExperienceState>,
    belief_participants: &mut AccountPagedMap<BeliefParticipantState>,
    seen: &mut RetailProjectionSeen,
    moment: crate::experience::ExperienceMoment,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<SettlementTransactionOutput, SettlementTransactionError> {
    let prepared = prepare_settlement_transaction_with_beliefs(
        accounts,
        retail_experience,
        belief_participants,
        seen,
        moment,
        receipts,
        t1_enabled,
    )?;
    apply_prepared_settlement_transaction(
        accounts,
        retail_experience,
        belief_participants,
        seen,
        prepared,
    )
}

fn apply_prepared_settlement_transaction(
    accounts: &mut AccountBook,
    retail_experience: &mut AccountPagedMap<crate::RetailExperienceState>,
    belief_participants: &mut AccountPagedMap<BeliefParticipantState>,
    seen: &mut RetailProjectionSeen,
    prepared: PreparedSettlementTransaction,
) -> Result<SettlementTransactionOutput, SettlementTransactionError> {
    accounts.extend(prepared.account_patch);
    retail_experience.extend(prepared.retail_patch);
    for (account_id, belief) in prepared.belief_patch {
        let mut participant = belief_participants
            .remove(&account_id)
            .expect("prepared institutional belief participant is missing");
        *participant.belief_mut() = belief;
        belief_participants.insert(account_id, participant);
    }
    *seen = prepared.seen;
    Ok(prepared.output)
}

/// 准备独立 Settlement patch，仅克隆本批收据实际触及的账户。
#[cfg(test)]
pub(super) fn prepare_settlement_transaction(
    accounts: &AccountBook,
    retail_experience: &AccountPagedMap<crate::RetailExperienceState>,
    seen: &RetailProjectionSeen,
    market_minute: u64,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<PreparedSettlementTransaction, SettlementTransactionError> {
    let belief_participants = AccountPagedMap::default();
    prepare_settlement_transaction_with_beliefs(
        accounts,
        retail_experience,
        &belief_participants,
        seen,
        test_experience_moment(market_minute),
        receipts,
        t1_enabled,
    )
}

pub(super) fn prepare_settlement_transaction_with_beliefs(
    accounts: &AccountBook,
    retail_experience: &AccountPagedMap<crate::RetailExperienceState>,
    belief_participants: &AccountPagedMap<BeliefParticipantState>,
    seen: &RetailProjectionSeen,
    moment: crate::experience::ExperienceMoment,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<PreparedSettlementTransaction, SettlementTransactionError> {
    let canonical: Vec<EnvelopeReceipt> = canonical_unseen_receipts(receipts, seen)?
        .into_iter()
        .cloned()
        .collect();
    let (account_shadow, settlement) =
        ReceiptSettlementPlan::from_receipts(&canonical)?.prepare_accounts(accounts, t1_enabled)?;
    // Source observation 和 Settlement Fill 各自修剪发生变更的账户。
    // Save 会拒绝超限的未持仓 watchlist，空收据批次没有要访问或修复的 retail 账户。
    let experience_accounts: BTreeSet<AccountId> = canonical
        .iter()
        .filter(|receipt| receipt.kind == super::ReceiptKind::Fill)
        .filter_map(|receipt| {
            accounts
                .get(&receipt.envelope.account)
                .filter(|account| matches!(account.kind(), AccountKind::Retail | AccountKind::Inst))
                .map(|_| receipt.envelope.account)
        })
        .collect();
    let retail_accounts: BTreeSet<AccountId> = experience_accounts
        .iter()
        .filter(|account_id| {
            accounts
                .get(account_id)
                .is_some_and(|account| account.kind() == AccountKind::Retail)
        })
        .copied()
        .collect();
    let positions_before = positions_of_accounts(accounts, &experience_accounts);
    let mut positions_after = positions_before.clone();
    for (account_id, account) in &account_shadow {
        if experience_accounts.contains(account_id) {
            positions_after.insert(*account_id, account.positions().clone());
        }
    }
    let projection = project_retail_receipts(RetailProjectionInput {
        retail_experience,
        retail_accounts: &retail_accounts,
        positions_before: &positions_before,
        positions_after: &positions_after,
        seen,
        market_minute: moment.market_minute,
        receipts: &canonical,
    })?;
    let belief_patch = project_institutional_experience(
        &account_shadow,
        belief_participants,
        &positions_before,
        &positions_after,
        seen,
        moment,
        &canonical,
    )?;

    Ok(PreparedSettlementTransaction {
        account_patch: account_shadow,
        retail_patch: projection.retail_experience,
        belief_patch,
        seen: projection.seen,
        output: SettlementTransactionOutput {
            settlement,
            events: projection.events,
        },
    })
}

#[cfg(test)]
fn test_experience_moment(market_minute: u64) -> crate::experience::ExperienceMoment {
    crate::experience::ExperienceMoment {
        civil_date: crate::calendar::CivilDate::from_ymd(2030, 1, 1).expect("test date is valid"),
        market_minute,
        trading_day: market_minute,
    }
}

fn positions_of_accounts(
    accounts: &AccountBook,
    experience_accounts: &BTreeSet<AccountId>,
) -> BTreeMap<AccountId, BTreeMap<StockCode, Position>> {
    experience_accounts
        .iter()
        .filter_map(|account_id| {
            accounts
                .get(account_id)
                .map(|account| (*account_id, account.positions().clone()))
        })
        .collect()
}

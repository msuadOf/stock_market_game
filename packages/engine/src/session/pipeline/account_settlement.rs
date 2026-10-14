//! 原子准备 Settlement 及经验 projection，成功后一起采纳账户 patch。

use super::fill_fact::ReceiptBatch;
use super::institutional_experience_projection::project_institutional_experience;
use super::retail_projection::{
    canonical_unseen_receipts, project_retail_receipts_dated, RetailProjectionError,
    RetailProjectionInput, RetailProjectionSeen, RetailReceiptEvent,
};
use super::settlement::{ReceiptSettlementPlan, SettlementApplication};
use super::{EnvelopeReceipt, StepFatal};
use crate::session::decision_chain::personal_state::BeliefParticipantState;
use crate::session::{account_book::AccountBook, account_paged_map::AccountPagedMap};
use crate::{
    Account, AccountId, AccountKind, GameSession, Position, RetailExperienceState, StockId,
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
#[cfg(test)]
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
    let compute = session.compute_context().clone();
    apply_settlement_transaction_with_beliefs(
        &compute,
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
        &crate::ComputeContext::shared_default().expect("测试计算上下文"),
        accounts,
        retail_experience,
        &mut belief_participants,
        seen,
        test_experience_moment(market_minute),
        receipts,
        t1_enabled,
    )
}

#[cfg(test)]
pub(super) fn apply_settlement_transaction_with_beliefs(
    compute: &crate::ComputeContext,
    accounts: &mut AccountBook,
    retail_experience: &mut AccountPagedMap<crate::RetailExperienceState>,
    belief_participants: &mut AccountPagedMap<BeliefParticipantState>,
    seen: &mut RetailProjectionSeen,
    moment: crate::experience::ExperienceMoment,
    receipts: &[EnvelopeReceipt],
    t1_enabled: bool,
) -> Result<SettlementTransactionOutput, SettlementTransactionError> {
    let batch = fixture_receipt_batch(receipts)?;
    let prepared = prepare_settlement_transaction_with_beliefs(
        compute,
        accounts,
        retail_experience,
        belief_participants,
        seen,
        moment,
        &batch,
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
    let batch = fixture_receipt_batch(receipts)?;
    prepare_settlement_transaction_with_beliefs(
        &crate::ComputeContext::shared_default().expect("测试计算上下文"),
        accounts,
        retail_experience,
        &belief_participants,
        seen,
        test_experience_moment(market_minute),
        &batch,
        t1_enabled,
    )
}

pub(super) fn prepare_settlement_transaction_with_beliefs(
    compute: &crate::ComputeContext,
    accounts: &AccountBook,
    retail_experience: &AccountPagedMap<crate::RetailExperienceState>,
    belief_participants: &AccountPagedMap<BeliefParticipantState>,
    seen: &RetailProjectionSeen,
    moment: crate::experience::ExperienceMoment,
    receipts: &ReceiptBatch,
    t1_enabled: bool,
) -> Result<PreparedSettlementTransaction, SettlementTransactionError> {
    // seen门只选择产出点同批事实的引用；不再从raw重新派生qty/gross。
    let canonical = canonical_unseen_receipts(receipts.raw(), seen)?;
    let mut seen_after = seen.clone();
    for receipt in &canonical {
        seen_after
            .insert(receipt.index, receipt.local_key.clone())
            .map_err(SettlementTransactionError::Projection)?;
    }
    let selected: BTreeSet<_> = canonical
        .iter()
        .map(|row| super::ReceiptIndex(row.index))
        .collect();
    let fill_facts = receipts.fill_facts().select(&selected);
    let (account_shadow, settlement) = ReceiptSettlementPlan::from_fill_facts(fill_facts.iter())?
        .prepare_accounts(compute, accounts, t1_enabled)?;
    // Source observation 和 Settlement Fill 各自修剪发生变更的账户。
    // Save 会拒绝超限的未持仓 watchlist，空收据批次没有要访问或修复的 retail 账户。
    let experience_accounts: BTreeSet<AccountId> = fill_facts
        .iter()
        .filter_map(|fact| {
            accounts
                .get(&fact.key().account)
                .filter(|account| matches!(account.kind(), AccountKind::Retail | AccountKind::Inst))
                .map(|_| fact.key().account)
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
    let projection = project_retail_receipts_dated(
        compute,
        RetailProjectionInput {
            retail_experience,
            retail_accounts: &retail_accounts,
            positions_before: &positions_before,
            positions_after: &positions_after,
            market_minute: moment.market_minute,
            fill_facts,
        },
        moment,
    )?;
    let belief_patch = project_institutional_experience(
        compute,
        &account_shadow,
        belief_participants,
        &positions_before,
        &positions_after,
        moment,
        fill_facts,
    )?;

    Ok(PreparedSettlementTransaction {
        account_patch: account_shadow,
        retail_patch: projection.retail_experience,
        belief_patch,
        seen: seen_after,
        output: SettlementTransactionOutput {
            settlement,
            events: projection.events,
        },
    })
}

/// 独立旧结算测试的fixture产出桥，不能进入生产market链。
#[cfg(test)]
fn fixture_receipt_batch(
    receipts: &[EnvelopeReceipt],
) -> Result<ReceiptBatch, SettlementTransactionError> {
    let canonical = canonical_unseen_receipts(receipts, &RetailProjectionSeen::default())?;
    Ok(ReceiptBatch::from_aggregation(
        canonical.into_iter().cloned().collect(),
    )?)
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
) -> BTreeMap<AccountId, BTreeMap<StockId, Position>> {
    experience_accounts
        .iter()
        .filter_map(|account_id| {
            accounts
                .get(account_id)
                .map(|account| (*account_id, account.positions().clone()))
        })
        .collect()
}

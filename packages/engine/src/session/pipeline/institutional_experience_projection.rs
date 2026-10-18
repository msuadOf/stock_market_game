use super::fill_fact::FillFactView;
use super::retail_projection::{
    project_institutional_receipts, RetailProjectionError, RetailProjectionInput,
};
use crate::session::account_paged_map::AccountPagedMap;
use crate::{Account, AccountId, AccountKind, Position, StockId};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn project_institutional_experience(
    compute: &crate::ComputeContext,
    accounts: &BTreeMap<AccountId, Account>,
    belief_participants: &AccountPagedMap<
        crate::session::decision_chain::personal_state::BeliefParticipantState,
    >,
    positions_before: &BTreeMap<AccountId, BTreeMap<StockId, Position>>,
    positions_after: &BTreeMap<AccountId, BTreeMap<StockId, Position>>,
    moment: crate::experience::ExperienceMoment,
    fill_facts: FillFactView<'_>,
) -> Result<BTreeMap<AccountId, crate::strategy::BeliefBook>, RetailProjectionError> {
    // 分类走 typed facts（账户身份已在事实键内）；不再触碰收据本体或 seen。
    let institutional_accounts: BTreeSet<AccountId> = fill_facts
        .iter()
        .filter_map(|fact| {
            accounts
                .get(&fact.key().account)
                .filter(|account| account.kind() == AccountKind::Inst)
                .filter(|account| {
                    account
                        .strategy()
                        .is_some_and(|strategy| strategy.belief_chain_params().is_some())
                })
                .map(|account| account.id())
        })
        .collect();
    if institutional_accounts.is_empty() {
        return Ok(BTreeMap::new());
    }

    let experiences = institutional_accounts
        .iter()
        .filter_map(|account| {
            belief_participants
                .get(account)
                .map(|book| (*account, book.belief().experience().clone()))
        })
        .collect::<BTreeMap<_, _>>();
    if let Some(account) = institutional_accounts
        .iter()
        .find(|account| !experiences.contains_key(account))
    {
        return Err(RetailProjectionError::MissingInstitutionExperience { account: *account });
    }
    let experience_map: AccountPagedMap<_> = experiences.into();
    let projection = project_institutional_receipts(
        compute,
        RetailProjectionInput {
            retail_experience: &experience_map,
            retail_accounts: &institutional_accounts,
            positions_before,
            positions_after,
            market_minute: moment.market_minute,
            fill_facts,
        },
        moment,
    )?;

    projection
        .retail_experience
        .into_iter()
        .map(|(account, experience)| {
            let mut book = belief_participants
                .get(&account)
                .map(|participant| participant.belief().clone())
                .ok_or(RetailProjectionError::MissingInstitutionExperience { account })?;
            *book.experience_mut() = experience;
            Ok((account, book))
        })
        .collect()
}

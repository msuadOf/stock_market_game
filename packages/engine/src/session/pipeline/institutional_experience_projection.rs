use super::retail_projection::{
    project_institutional_receipts, RetailProjectionError, RetailProjectionInput,
    RetailProjectionSeen,
};
use crate::session::account_paged_map::AccountPagedMap;
use crate::{Account, AccountId, AccountKind, Position, StockCode};
use std::collections::{BTreeMap, BTreeSet};

pub(super) fn project_institutional_experience(
    accounts: &BTreeMap<AccountId, Account>,
    belief_books: &AccountPagedMap<crate::strategy::BeliefBook>,
    positions_before: &BTreeMap<AccountId, BTreeMap<StockCode, Position>>,
    positions_after: &BTreeMap<AccountId, BTreeMap<StockCode, Position>>,
    seen: &RetailProjectionSeen,
    moment: crate::experience::ExperienceMoment,
    receipts: &[super::EnvelopeReceipt],
) -> Result<BTreeMap<AccountId, crate::strategy::BeliefBook>, RetailProjectionError> {
    let institutional_accounts: BTreeSet<AccountId> = receipts
        .iter()
        .filter(|receipt| receipt.kind == super::ReceiptKind::Fill)
        .filter_map(|receipt| {
            accounts
                .get(&receipt.envelope.account)
                .filter(|account| account.kind == AccountKind::Inst)
                .filter(|account| {
                    account
                        .strategy
                        .as_ref()
                        .is_some_and(|strategy| strategy.belief_chain_params().is_some())
                })
                .map(|account| account.id)
        })
        .collect();
    if institutional_accounts.is_empty() {
        return Ok(BTreeMap::new());
    }

    let experiences = institutional_accounts
        .iter()
        .filter_map(|account| {
            belief_books
                .get(account)
                .map(|book| (*account, book.experience().clone()))
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
        RetailProjectionInput {
            retail_experience: &experience_map,
            retail_accounts: &institutional_accounts,
            positions_before,
            positions_after,
            seen,
            market_minute: moment.market_minute,
            receipts,
        },
        moment,
    )?;

    projection
        .retail_experience
        .into_iter()
        .map(|(account, experience)| {
            let mut book = belief_books
                .get(&account)
                .cloned()
                .ok_or(RetailProjectionError::MissingInstitutionExperience { account })?;
            *book.experience_mut() = experience;
            Ok((account, book))
        })
        .collect()
}

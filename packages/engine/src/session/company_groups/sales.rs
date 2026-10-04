use super::{invalid, GroupStructure};
use crate::accounting::consolidation::IntercompanySale;
use crate::accounting::{
    AccountingAmount, AccountingPeriod, BusinessKind, JournalEntry, LedgerAccountId, PostingSide,
};
use crate::company::industrial::{IndustrialBooks, InventorySourceEvent};
use crate::company::operations::CompanyOperations;
use crate::company::CompanyId;
use crate::information::InformationError;
use std::collections::{BTreeMap, BTreeSet};

fn net(
    entry: &JournalEntry,
    account: &LedgerAccountId,
) -> Result<AccountingAmount, InformationError> {
    let mut amount = AccountingAmount::ZERO;
    for line in entry.lines.iter().filter(|line| &line.account == account) {
        amount = match line.side {
            PostingSide::Debit => amount.add(line.amount),
            PostingSide::Credit => amount.sub(line.amount),
        }
        .map_err(|error| invalid(error.to_string()))?;
    }
    Ok(amount)
}

fn inventory_source(
    books: &IndustrialBooks,
    event: crate::accounting::BusinessEventId,
) -> Result<&InventorySourceEvent, InformationError> {
    let sources = books
        .inventory_source_events()
        .iter()
        .filter(|source| source.event == event)
        .collect::<Vec<_>>();
    if sources.len() != 1 {
        return Err(invalid(format!(
            "internal sale source {} requires exactly one inventory item identity",
            event.value()
        )));
    }
    Ok(sources[0])
}

fn remaining_cost(
    books: &IndustrialBooks,
    purchase: &InventorySourceEvent,
    bound: AccountingPeriod,
) -> Result<AccountingAmount, InformationError> {
    books
        .validate_inventory_source_events()
        .map_err(|error| invalid(error.to_string()))?;
    let entries = books
        .books()
        .journal()
        .entries()
        .map(|entry| (entry.source, entry))
        .collect::<BTreeMap<_, _>>();
    let items = books
        .inventory_source_events()
        .iter()
        .filter(|source| {
            source.account == purchase.account && entries[&source.event].period() <= bound
        })
        .map(|source| &source.item)
        .collect::<BTreeSet<_>>();
    if items.len() != 1 {
        return Err(invalid(format!("internal inventory account {} contains multiple item identities; ambiguous cost pool unsupported", purchase.account.0)));
    }
    let mut sources = books
        .inventory_source_events()
        .iter()
        .filter(|source| source.item == purchase.item && entries[&source.event].period() <= bound)
        .collect::<Vec<_>>();
    sources.sort_by_key(|source| (entries[&source.event].date, source.event));
    let mut pool = AccountingAmount::ZERO;
    let mut remaining = AccountingAmount::ZERO;
    let mut original = None;
    for source in sources {
        let amount = net(entries[&source.event], &source.account)?;
        if amount.is_positive() {
            pool = pool
                .add(amount)
                .map_err(|error| invalid(error.to_string()))?;
            if source.event == purchase.event {
                remaining = amount;
                original = Some(amount);
            }
        } else if amount.is_negative() {
            let issued = amount.neg().map_err(|error| invalid(error.to_string()))?;
            if !pool.is_positive() || issued > pool {
                return Err(invalid(format!(
                    "internal inventory source {} issue {} exceeds cost pool {}",
                    source.event.value(),
                    issued,
                    pool
                )));
            }
            let product = remaining
                .cents()
                .checked_mul(issued.cents())
                .ok_or_else(|| invalid("internal inventory proportional cost overflow".into()))?;
            let allocated = crate::accounting::rhe_div(product, pool.cents())
                .map_err(|error| invalid(error.to_string()))?;
            remaining = remaining
                .sub(AccountingAmount::from_cents(allocated))
                .map_err(|error| invalid(error.to_string()))?;
            pool = pool
                .sub(issued)
                .map_err(|error| invalid(error.to_string()))?;
        }
        if remaining.is_negative() || (pool.is_zero() && !remaining.is_zero()) {
            return Err(invalid(format!(
                "internal inventory residual {} inconsistent with pool {}",
                remaining, pool
            )));
        }
    }
    let original = original.ok_or_else(|| {
        invalid(format!(
            "internal purchase source {} has no positive receipt before cutoff",
            purchase.event.value()
        ))
    })?;
    if remaining > original {
        return Err(invalid(format!(
            "internal inventory residual {} exceeds original receipt {}",
            remaining, original
        )));
    }
    Ok(remaining)
}

pub(super) fn derive_sales(
    group: &GroupStructure,
    ops: &CompanyOperations,
    bound: Option<AccountingPeriod>,
) -> Result<Vec<IntercompanySale>, InformationError> {
    let ids = std::iter::once(group.root.clone())
        .chain(group.holdings.iter().map(|holding| holding.company.clone()))
        .collect::<BTreeSet<_>>();
    let cutoff = match bound {
        Some(period) => period,
        None => {
            let date = ops.next_expected_date().prev()?;
            AccountingPeriod::from_ymd(date.year(), date.month())
                .map_err(|error| invalid(error.to_string()))?
        }
    };
    let mut sales = Vec::new();
    let mut pairs = BTreeSet::new();
    for seller_id in &ids {
        let seller = ops
            .company(seller_id)
            .ok_or_else(|| invalid(format!("unknown internal seller {}", seller_id.0)))?;
        let Some(seller_books) = seller.books().as_industrial() else {
            continue;
        };
        let entries = seller_books
            .books()
            .journal()
            .entries()
            .map(|entry| (entry.source, entry))
            .collect::<BTreeMap<_, _>>();
        for identity in seller_books
            .trade_counterparty_events()
            .iter()
            .filter(|identity| identity.account.0 == "1122")
        {
            let entry = entries[&identity.event];
            let buyer_id = CompanyId(identity.counterparty.0.clone());
            if entry.kind != BusinessKind::CreditSale
                || entry.period() > cutoff
                || !ids.contains(&buyer_id)
                || buyer_id == *seller_id
            {
                continue;
            }
            let buyer = ops
                .company(&buyer_id)
                .ok_or_else(|| invalid(format!("unknown internal buyer {}", buyer_id.0)))?;
            let buyer_books = buyer.books().as_industrial().ok_or_else(|| {
                invalid(format!(
                    "internal inventory sale to non-Industrial buyer {} unsupported",
                    buyer_id.0
                ))
            })?;
            let buyer_entries = buyer_books
                .books()
                .journal()
                .entries()
                .map(|entry| (entry.source, entry))
                .collect::<BTreeMap<_, _>>();
            let gross = net(entry, &identity.account)?;
            let mut matched = Vec::new();
            for purchase in buyer_books
                .trade_counterparty_events()
                .iter()
                .filter(|purchase| {
                    purchase.account.0 == "2202" && purchase.counterparty.0 == seller_id.0
                })
            {
                let buyer_entry = buyer_entries[&purchase.event];
                if buyer_entry.kind == BusinessKind::CreditSale
                    && buyer_entry.date == entry.date
                    && buyer_entry.period() <= cutoff
                    && net(buyer_entry, &purchase.account)?
                        .neg()
                        .map_err(|error| invalid(error.to_string()))?
                        == gross
                {
                    matched.push(buyer_entry);
                }
            }
            if matched.len() != 1 {
                return Err(invalid(format!(
                    "internal sale {} source {} has {} matching purchase sources for {}",
                    seller_id.0,
                    identity.event.value(),
                    matched.len(),
                    buyer_id.0
                )));
            }
            let purchase = inventory_source(buyer_books, matched[0].source)?;
            let unsold = remaining_cost(buyer_books, purchase, cutoff)?;
            if entry.period().year() != cutoff.year() {
                if !unsold.is_zero() {
                    return Err(invalid(format!("cross-year unsold internal sale {} to {} unsupported by current-period worksheet policy", seller_id.0, buyer_id.0)));
                }
                continue;
            }
            if !pairs.insert((seller_id.clone(), buyer_id.clone())) {
                return Err(invalid(format!(
                    "multiple internal sale batches for {} to {} unsupported",
                    seller_id.0, buyer_id.0
                )));
            }
            let seller_source = inventory_source(seller_books, identity.event)?;
            let revenue_account = LedgerAccountId("6001".into());
            let cost_account = LedgerAccountId("6401".into());
            let invoice = net(entry, &revenue_account)?
                .neg()
                .map_err(|error| invalid(error.to_string()))?;
            let cost = net(entry, &cost_account)?;
            if net(entry, &seller_source.account)?
                .neg()
                .map_err(|error| invalid(error.to_string()))?
                != cost
                || net(matched[0], &purchase.account)? != invoice
            {
                return Err(invalid(format!(
                    "internal sale {} to {} inventory source costs do not match invoice and cost",
                    seller_id.0, buyer_id.0
                )));
            }
            sales.push(IntercompanySale {
                seller: crate::accounting::consolidation::MemberId(seller_id.0.clone()),
                buyer: crate::accounting::consolidation::MemberId(buyer_id.0),
                revenue_account,
                cost_account,
                inventory_account: purchase.account.clone(),
                invoice_amount: invoice,
                cost_amount: cost,
                unsold_inventory: unsold,
            });
        }
    }
    Ok(sales)
}

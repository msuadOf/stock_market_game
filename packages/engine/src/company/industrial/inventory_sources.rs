use super::{IndustrialBooks, IndustrialError};
use crate::accounting::{BusinessEventId, InventoryItemCode, LedgerAccountId};
use std::collections::{BTreeMap, BTreeSet};

/// 存货移动的来源身份；成本金额取 Journal，数量与计价仍由 InventoryLedger 管理。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct InventorySourceEvent {
    pub event: BusinessEventId,
    pub item: InventoryItemCode,
    pub account: LedgerAccountId,
}

impl IndustrialBooks {
    pub fn inventory_source_events(&self) -> &[InventorySourceEvent] {
        &self.inventory_source_events
    }
    pub(super) fn record_inventory_source(
        &mut self,
        event: BusinessEventId,
        item: &InventoryItemCode,
        account: &LedgerAccountId,
    ) {
        self.inventory_source_events.push(InventorySourceEvent {
            event,
            item: item.clone(),
            account: account.clone(),
        });
    }
    pub(crate) fn validate_inventory_source_events(&self) -> Result<(), IndustrialError> {
        let entries = self
            .books()
            .journal()
            .entries()
            .map(|entry| (entry.source, entry))
            .collect::<BTreeMap<_, _>>();
        let mut seen = BTreeSet::new();
        let mut covered_accounts = BTreeSet::new();
        let mut covered_items = BTreeSet::new();
        for source in &self.inventory_source_events {
            let detail = format!(
                "invalid inventory source {} item {} account {}",
                source.event.value(),
                source.item.0,
                source.account.0
            );
            if !seen.insert((source.event, source.item.clone()))
                || !entries.get(&source.event).is_some_and(|entry| {
                    entry
                        .lines
                        .iter()
                        .any(|line| line.account == source.account)
                })
                || !self
                    .inventory()
                    .get(&source.item)
                    .is_some_and(|item| item.account() == &source.account)
            {
                return Err(IndustrialError::Accounting(
                    crate::accounting::AccountingError::ChartInvalid { detail },
                ));
            }
            if !covered_accounts.insert((source.event, source.account.clone()))
                && entries[&source.event].kind != crate::accounting::BusinessKind::OpeningBalance
            {
                return Err(IndustrialError::Accounting(
                    crate::accounting::AccountingError::ChartInvalid {
                        detail: format!(
                            "inventory event {} account {} has ambiguous item identities",
                            source.event.value(),
                            source.account.0
                        ),
                    },
                ));
            }
            covered_items.insert(source.item.clone());
        }
        let accounts = self
            .inventory()
            .iter()
            .map(|(_, item)| item.account().clone())
            .collect::<BTreeSet<_>>();
        for entry in entries.values() {
            for line in &entry.lines {
                if accounts.contains(&line.account)
                    && !covered_accounts.contains(&(entry.source, line.account.clone()))
                {
                    return Err(IndustrialError::Accounting(
                        crate::accounting::AccountingError::ChartInvalid {
                            detail: format!(
                                "inventory Journal event {} account {} missing item identity",
                                entry.source.value(),
                                line.account.0
                            ),
                        },
                    ));
                }
            }
        }
        for (item, _) in self.inventory().iter() {
            if !covered_items.contains(item) {
                return Err(IndustrialError::Accounting(
                    crate::accounting::AccountingError::ChartInvalid {
                        detail: format!("inventory item {} missing source identity", item.0),
                    },
                ));
            }
        }
        Ok(())
    }
}

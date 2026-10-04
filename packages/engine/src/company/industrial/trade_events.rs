use std::collections::{BTreeMap, BTreeSet};

use crate::accounting::{BusinessEventId, LedgerAccountId};
use crate::company::CounterpartyId;

use super::{IndustrialBooks, IndustrialError};

/// 往来分录的对手方身份；日期、金额和借贷方向始终从 Journal 读取。
#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize)]
pub struct TradeCounterpartyEvent {
    pub event: BusinessEventId,
    pub counterparty: CounterpartyId,
    pub account: LedgerAccountId,
}

impl IndustrialBooks {
    pub fn trade_counterparty_events(&self) -> &[TradeCounterpartyEvent] {
        &self.trade_counterparty_events
    }

    pub(super) fn record_trade_counterparty(
        &mut self,
        event: BusinessEventId,
        counterparty: &CounterpartyId,
        account: &str,
    ) {
        self.trade_counterparty_events.push(TradeCounterpartyEvent {
            event,
            counterparty: counterparty.clone(),
            account: LedgerAccountId(account.into()),
        });
    }

    fn trade_event_error(detail: String) -> IndustrialError {
        IndustrialError::Accounting(crate::accounting::AccountingError::ChartInvalid { detail })
    }

    pub(crate) fn validate_trade_counterparty_events(&self) -> Result<(), IndustrialError> {
        let entries = self
            .books()
            .journal()
            .entries()
            .map(|entry| (entry.source, entry))
            .collect::<BTreeMap<_, _>>();
        let mut seen = BTreeSet::new();
        for identity in &self.trade_counterparty_events {
            if !seen.insert((identity.event, identity.account.clone())) {
                return Err(Self::trade_event_error(format!(
                    "duplicate trade counterparty event {} account {}",
                    identity.event.value(),
                    identity.account.0
                )));
            }
            if self.counterparties().get(&identity.counterparty).is_none() {
                return Err(Self::trade_event_error(format!(
                    "trade event {} unknown counterparty {}",
                    identity.event.value(),
                    identity.counterparty.0
                )));
            }
            if ![super::chart::acct::AR, super::chart::acct::PAYABLE]
                .contains(&identity.account.0.as_str())
            {
                return Err(Self::trade_event_error(format!(
                    "trade event {} invalid trade account {}",
                    identity.event.value(),
                    identity.account.0
                )));
            }
            let entry = entries.get(&identity.event).ok_or_else(|| {
                Self::trade_event_error(format!(
                    "trade counterparty event {} missing Journal source",
                    identity.event.value()
                ))
            })?;
            if !entry
                .lines
                .iter()
                .any(|line| line.account == identity.account)
            {
                return Err(Self::trade_event_error(format!(
                    "trade counterparty event {} missing Journal account {}",
                    identity.event.value(),
                    identity.account.0
                )));
            }
        }
        for entry in entries.values() {
            if entry.kind == crate::accounting::BusinessKind::OpeningBalance {
                continue;
            }
            for line in &entry.lines {
                if [super::chart::acct::AR, super::chart::acct::PAYABLE]
                    .contains(&line.account.0.as_str())
                    && !seen.contains(&(entry.source, line.account.clone()))
                {
                    return Err(Self::trade_event_error(format!(
                        "Journal trade event {} account {} missing counterparty identity",
                        entry.source.value(),
                        line.account.0
                    )));
                }
            }
        }
        Ok(())
    }
}

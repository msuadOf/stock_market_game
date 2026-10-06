use crate::company::share_registry::{
    AcquisitionSource, DayNetChange, HolderId, MovementScope, NetAcquisition, ShareDayRequest,
    ShareRegistry, ShareRestriction,
};
use crate::company::stock_distribution::{holder_credit_lots, StockDistributionBook};
use crate::{account::StockCode, orderbook::AccountId};
use std::collections::{BTreeMap, BTreeSet};
use thiserror::Error;

#[derive(Clone, Debug, Default, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct SessionCorporateActions {
    #[ts(type = "import(\"../../save/schema/corporate-actions\").ShareRegistry[]")]
    pub registries: Vec<ShareRegistry>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").CashDividendBook[]")]
    pub dividends: Vec<crate::company::cash_dividend::CashDividendBook>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").CashDividendTaxBook[]")]
    pub dividend_tax_books: Vec<crate::company::cash_dividend_tax::CashDividendTaxBook>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").StockDistributionBook[]")]
    pub stock_distributions: Vec<StockDistributionBook>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").AccountDividendGrossReceipt[]")]
    pub account_gross_receipts: Vec<AccountDividendGrossReceipt>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").ExternalDividendReceipt[]")]
    pub external_receipts: Vec<ExternalDividendReceipt>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").AppliedExReferenceGroup[]")]
    pub applied_ex_reference_groups: Vec<AppliedExReferenceGroup>,
}

/// 已应用到行情前收锚的除权除息组：同一证券同一除权日只产生一个参考价，
/// 组合事实同时列出参与合计的现金分红计划与送转事件。
#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AppliedExReferenceGroup {
    pub date: crate::calendar::CivilDate,
    pub stock: StockCode,
    pub cash_plan_ids: Vec<String>,
    pub stock_event_ids: Vec<String>,
    pub reference: crate::company::ex_reference_price::ExReferencePrice,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum DividendTaxStatus {
    IndividualPublicMarket,
    TreatmentNotConfigured,
}

fn holder_key(holder: &HolderId) -> String {
    match holder {
        HolderId::Account(account) => format!("account-{}", account.0),
        HolderId::External(name) => format!("external-{name}"),
        HolderId::IssuerTreasury => "issuer-treasury".to_owned(),
    }
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AccountDividendGrossReceipt {
    pub payment_id: String,
    pub plan_id: String,
    #[ts(type = "string")]
    pub account: AccountId,
    pub paid_on: crate::calendar::CivilDate,
    pub gross: crate::money::Money,
    pub tax_status: DividendTaxStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct ExternalDividendReceipt {
    pub payment_id: String,
    pub plan_id: String,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").HolderId")]
    pub holder: HolderId,
    pub paid_on: crate::calendar::CivilDate,
    pub gross: crate::money::Money,
    pub tax_status: DividendTaxStatus,
}

#[derive(Clone, Debug, Eq, PartialEq, Error)]
pub enum SessionCorporateActionsError {
    #[error("公司行为股东名册配置无效：{0}")]
    Invalid(String),
}

impl SessionCorporateActions {
    pub(crate) fn process_dividends_on_day_end(
        &mut self,
        day: crate::calendar::CivilDate,
        calendar: &crate::calendar::TradingCalendar,
        issuers: &crate::company::identity::IssuerRegistry,
        company_system: &mut crate::company::CompanySystem,
        accounts: &mut super::account_book::AccountBook,
    ) -> Result<(), SessionCorporateActionsError> {
        for index in 0..self.dividends.len() {
            let plan = self.dividends[index].plan().clone();
            let status = self.dividends[index].status().clone();
            if status == crate::company::cash_dividend::CashDividendStatus::Approved {
                if day > plan.announced_on {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "分红计划 {} 错过公告日",
                        plan.plan_id
                    )));
                }
                if day == plan.announced_on {
                    self.dividends[index].announce(day).map_err(|error| {
                        SessionCorporateActionsError::Invalid(error.to_string())
                    })?;
                }
            }
            let status = self.dividends[index].status().clone();
            if day == plan.registered_on {
                if status != crate::company::cash_dividend::CashDividendStatus::Announced {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "分红计划 {} 登记日前未完成公告",
                        plan.plan_id
                    )));
                }
                let registry_index = self
                    .registries
                    .iter()
                    .position(|registry| {
                        registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                    })
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("登记日缺少匹配股东名册".into())
                    })?;
                let snapshot = self.registries[registry_index]
                    .register(plan.plan_id.clone(), day)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .clone();
                self.dividends[index]
                    .register(snapshot, calendar)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                let registered_gross = crate::accounting::AccountingAmount::from_money(
                    self.dividends[index].total_gross().map_err(|error| {
                        SessionCorporateActionsError::Invalid(error.to_string())
                    })?,
                );
                let approved_gross = company_system
                    .dividend_plan_facts(&plan.issuer)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .into_iter()
                    .find(|fact| fact.plan_id == plan.plan_id)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(
                            "登记现金分红缺少Simple批准事实".into(),
                        )
                    })?
                    .total_gross;
                if registered_gross != approved_gross {
                    return Err(SessionCorporateActionsError::Invalid(
                        "登记日税前总额与已批准Simple分红金额不一致".into(),
                    ));
                }
            } else if day > plan.registered_on && self.dividends[index].registration().is_none() {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "分红计划 {} 错过登记日",
                    plan.plan_id
                )));
            }
            if day >= plan.payable_on {
                self.dividends[index]
                    .mark_payable(day)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
            if day < plan.payable_on
                || self.dividends[index].status()
                    == &crate::company::cash_dividend::CashDividendStatus::Paid
            {
                continue;
            }
            let payment_id = format!("dividend-payment:{}:{}", plan.plan_id, day);
            if self.dividends[index]
                .payments()
                .iter()
                .any(|payment| payment.payment_id() == payment_id)
            {
                continue;
            }
            let entitlements = self.dividends[index]
                .entitlements()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            let paid_holders: BTreeSet<_> = self.dividends[index]
                .successful_payments()
                .iter()
                .map(|(holder, _)| holder.clone())
                .collect();
            let mut outcomes = Vec::new();
            let mut new_external_receipts = Vec::new();
            let mut settled_gross = crate::money::Money::ZERO;
            for entitlement in entitlements
                .iter()
                .filter(|item| !paid_holders.contains(&item.holder))
            {
                match &entitlement.holder {
                    HolderId::Account(account) => {
                        let account_state = accounts.get_mut(account).ok_or_else(|| {
                            SessionCorporateActionsError::Invalid(format!(
                                "分红股东账户 {account:?} 不存在"
                            ))
                        })?;
                        match account_state.credit_cash(entitlement.gross) {
                            Ok(()) => {
                                settled_gross =
                                    settled_gross.add(entitlement.gross).map_err(|error| {
                                        SessionCorporateActionsError::Invalid(error.to_string())
                                    })?;
                                outcomes.push(
                                    crate::company::cash_dividend::HolderPaymentOutcome::Paid {
                                        holder: entitlement.holder.clone(),
                                        amount: entitlement.gross,
                                    },
                                );
                                self.account_gross_receipts
                                    .push(AccountDividendGrossReceipt {
                                        payment_id: payment_id.clone(),
                                        plan_id: plan.plan_id.clone(),
                                        account: *account,
                                        paid_on: day,
                                        gross: entitlement.gross,
                                        tax_status: if self.dividend_tax_books.iter().any(|book| {
                                            book.account() == *account
                                                && book.stock() == &plan.stock
                                        }) {
                                            DividendTaxStatus::IndividualPublicMarket
                                        } else {
                                            DividendTaxStatus::TreatmentNotConfigured
                                        },
                                    });
                            }
                            Err(error) => outcomes.push(
                                crate::company::cash_dividend::HolderPaymentOutcome::Failed {
                                    holder: entitlement.holder.clone(),
                                    reason: error.to_string(),
                                },
                            ),
                        }
                    }
                    HolderId::External(name) if !name.trim().is_empty() => {
                        settled_gross = settled_gross.add(entitlement.gross).map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                        outcomes.push(crate::company::cash_dividend::HolderPaymentOutcome::Paid {
                            holder: entitlement.holder.clone(),
                            amount: entitlement.gross,
                        });
                        new_external_receipts.push(ExternalDividendReceipt {
                            payment_id: payment_id.clone(),
                            plan_id: plan.plan_id.clone(),
                            holder: entitlement.holder.clone(),
                            paid_on: day,
                            gross: entitlement.gross,
                            tax_status: DividendTaxStatus::TreatmentNotConfigured,
                        });
                    }
                    HolderId::External(_) => {
                        return Err(SessionCorporateActionsError::Invalid(
                            "External holder 必须具名".into(),
                        ));
                    }
                    HolderId::IssuerTreasury => {
                        return Err(SessionCorporateActionsError::Invalid(
                            "IssuerTreasury 不得取得现金股利".into(),
                        ));
                    }
                }
            }
            self.dividends[index]
                .settle(payment_id.clone(), day, outcomes)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            self.external_receipts.extend(new_external_receipts);
            if settled_gross.cents() > 0 {
                let issuer = issuers.get(&plan.issuer).ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("分红付款的发行人身份丢失".into())
                })?;
                let settled = crate::accounting::AccountingAmount::from_money(settled_gross);
                company_system
                    .pay_dividend(&plan.issuer, &plan.plan_id, &payment_id, day, settled)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                if issuer.listed_stock.as_ref() != Some(&plan.stock) {
                    return Err(SessionCorporateActionsError::Invalid(
                        "分红付款发行人证券关系已改变".into(),
                    ));
                }
            }
        }
        self.settle_dividend_tax(accounts)?;
        Ok(())
    }

    /// 送转事件日结：公告 → R 日按登记快照冻结分配 → R+1 以 NonTradingTransfer
    /// 落账新股并同步投资者持仓、发行股数与 Simple 账面展示事实。
    ///
    /// 事件幂等：重复执行同一日结不重复入账；任何失败向外返回错误，由日终
    /// 候选事务整体回滚，不留下部分状态。
    pub(crate) fn process_stock_distributions_on_day_end(
        &mut self,
        day: crate::calendar::CivilDate,
        calendar: &crate::calendar::TradingCalendar,
        company_system: &mut crate::company::CompanySystem,
        accounts: &mut super::account_book::AccountBook,
    ) -> Result<(), SessionCorporateActionsError> {
        for index in 0..self.stock_distributions.len() {
            let plan = self.stock_distributions[index].plan().clone();
            if self.stock_distributions[index].status()
                == &crate::company::stock_distribution::StockDistributionStatus::Approved
            {
                if day > plan.announced_on {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "送转事件 {} 错过公告日",
                        plan.event_id
                    )));
                }
                if day == plan.announced_on {
                    self.stock_distributions[index]
                        .announce(day)
                        .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                }
            }
            if day == plan.registered_on {
                if self.stock_distributions[index].status()
                    != &crate::company::stock_distribution::StockDistributionStatus::Announced
                {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "送转事件 {} 登记日前未完成公告",
                        plan.event_id
                    )));
                }
                let registry_index = self
                    .registries
                    .iter()
                    .position(|registry| {
                        registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                    })
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(
                            "送转登记日缺少匹配股东名册".into(),
                        )
                    })?;
                let snapshot = self.registries[registry_index]
                    .register(plan.event_id.clone(), day)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .clone();
                self.stock_distributions[index]
                    .register(snapshot, calendar)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                let approved_shares = self.stock_distributions[index]
                    .receipt()
                    .map(|receipt| receipt.approved_total_new_shares)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(
                            "送转登记后缺少分配回执".into(),
                        )
                    })?;
                let finance_fact = company_system
                    .stock_distribution_facts(&plan.issuer)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                    .into_iter()
                    .find(|fact| fact.event_id == plan.event_id)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(
                            "送转登记缺少Simple声明事实".into(),
                        )
                    })?;
                if finance_fact.new_shares != approved_shares {
                    return Err(SessionCorporateActionsError::Invalid(
                        "送转登记日获批新增股数与Simple声明不一致".into(),
                    ));
                }
            } else if day > plan.registered_on
                && self.stock_distributions[index].registration().is_none()
            {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "送转事件 {} 错过登记日",
                    plan.event_id
                )));
            }
            if day != plan.ex_rights_on
                || self.stock_distributions[index].status()
                    != &crate::company::stock_distribution::StockDistributionStatus::Registered
            {
                continue;
            }
            let receipt = self.stock_distributions[index]
                .receipt()
                .cloned()
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("送转入账缺少冻结分配回执".into())
                })?;
            let credit_lots = holder_credit_lots(&receipt, day)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            let mut changes = Vec::with_capacity(credit_lots.len());
            for lot in &credit_lots {
                changes.push(DayNetChange {
                    holder: lot.holder.clone(),
                    change: i128::try_from(lot.qty).map_err(|_| {
                        SessionCorporateActionsError::Invalid("送转入账股数超出日结范围".into())
                    })?,
                    acquisition: Some(NetAcquisition {
                        lot_id: format!(
                            "stock-distribution:{}:{}",
                            plan.event_id,
                            holder_key(&lot.holder)
                        ),
                        source: AcquisitionSource::CorporateAction {
                            event: plan.event_id.clone(),
                        },
                        restriction: lot.restriction.clone(),
                    }),
                });
            }
            let registry_index = self
                .registries
                .iter()
                .position(|registry| {
                    registry.stock() == &plan.stock && registry.issuer() == &plan.issuer
                })
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("送转入账缺少匹配股东名册".into())
                })?;
            self.registries[registry_index]
                .close_day(ShareDayRequest {
                    event_id: format!("stock-distribution:{}", plan.event_id),
                    day,
                    scope: MovementScope::NonTradingTransfer {
                        basis: plan.approval_reference.clone(),
                    },
                    changes,
                })
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            for lot in &credit_lots {
                let HolderId::Account(account) = &lot.holder else {
                    continue;
                };
                let account = *account;
                let account_state = accounts.get_mut(&account).ok_or_else(|| {
                    SessionCorporateActionsError::Invalid(format!(
                        "送转股东账户 {account:?} 不存在"
                    ))
                })?;
                account_state
                    .credit_position_shares(plan.stock.clone(), lot.qty)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
            company_system
                .record_stock_distribution_credit(
                    &plan.issuer,
                    &plan.event_id,
                    day,
                    receipt.approved_total_new_shares,
                )
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            self.stock_distributions[index]
                .mark_credited(day)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        }
        Ok(())
    }

    pub(crate) fn close_registries_through(
        &mut self,
        through: crate::calendar::CivilDate,
        accounts: &BTreeMap<AccountId, BTreeMap<StockCode, u64>>,
        confirmations: &BTreeMap<
            AccountId,
            crate::experience::AppendOnlyHistory<super::PersonalTradeConfirmation>,
        >,
    ) -> Result<(), SessionCorporateActionsError> {
        for registry in &mut self.registries {
            if registry.settled_on() > through {
                return Err(SessionCorporateActionsError::Invalid(
                    "股东名册日期晚于日终日期".into(),
                ));
            }
            let mut day = registry.settled_on();
            while day < through {
                day = day
                    .next()
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                let mut net_by_account = BTreeMap::<AccountId, (i128, Vec<u64>)>::new();
                for (account, rows) in confirmations {
                    for row in rows
                        .iter()
                        .filter(|row| row.civil_date == day && row.code == *registry.stock())
                    {
                        let entry = net_by_account.entry(*account).or_default();
                        let quantity = i128::from(row.quantity_shares);
                        entry.0 = if row.side == crate::orderbook::Side::Buy {
                            entry.0.checked_add(quantity)
                        } else {
                            entry.0.checked_sub(quantity)
                        }
                        .ok_or_else(|| {
                            SessionCorporateActionsError::Invalid("当日成交净股数溢出".into())
                        })?;
                        entry.1.push(row.receipt_id);
                    }
                }
                let mut changes = Vec::new();
                for holding in registry.holdings() {
                    let HolderId::Account(account) = &holding.holder else {
                        changes.push(DayNetChange {
                            holder: holding.holder.clone(),
                            change: 0,
                            acquisition: None,
                        });
                        continue;
                    };
                    let account = *account;
                    let (change, receipt_ids) = net_by_account.remove(&account).unwrap_or_default();
                    let acquisition = if change > 0 {
                        let source = receipt_ids
                            .iter()
                            .map(u64::to_string)
                            .collect::<Vec<_>>()
                            .join(",");
                        Some(NetAcquisition {
                            lot_id: format!("market:{}:{}:{}", registry.stock().0, account.0, day),
                            source: AcquisitionSource::SecondaryMarket { settlement: source },
                            restriction: ShareRestriction::Unrestricted,
                        })
                    } else {
                        None
                    };
                    changes.push(DayNetChange {
                        holder: HolderId::Account(account),
                        change,
                        acquisition,
                    });
                }
                for (account, (change, receipt_ids)) in net_by_account {
                    if change <= 0 || !accounts.contains_key(&account) {
                        return Err(SessionCorporateActionsError::Invalid(
                            "无既有名册的证券成交账户只能通过真实净买入进入股东登记".into(),
                        ));
                    }
                    let source = receipt_ids
                        .iter()
                        .map(u64::to_string)
                        .collect::<Vec<_>>()
                        .join(",");
                    changes.push(DayNetChange {
                        holder: HolderId::Account(account),
                        change,
                        acquisition: Some(NetAcquisition {
                            lot_id: format!("market:{}:{}:{}", registry.stock().0, account.0, day),
                            source: AcquisitionSource::SecondaryMarket { settlement: source },
                            restriction: ShareRestriction::Unrestricted,
                        }),
                    });
                }
                registry
                    .close_day(ShareDayRequest {
                        event_id: format!("session-market:{}:{}", registry.stock().0, day),
                        day,
                        scope: MovementScope::PublicMarket,
                        changes,
                    })
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
            for holding in registry.holdings() {
                if let HolderId::Account(account) = &holding.holder {
                    let account = *account;
                    let expected = holding
                        .lots
                        .iter()
                        .try_fold(0_u64, |sum, lot| sum.checked_add(lot.qty))
                        .ok_or_else(|| {
                            SessionCorporateActionsError::Invalid("名册股份溢出".into())
                        })?;
                    let actual = accounts
                        .get(&account)
                        .and_then(|positions| positions.get(registry.stock()))
                        .copied()
                        .unwrap_or(0);
                    if expected != actual {
                        return Err(SessionCorporateActionsError::Invalid(format!(
                            "日终账户 {account:?} 实际持仓 {actual} 与股东名册 {expected} 不一致"
                        )));
                    }
                }
            }
        }
        self.sync_dividend_tax_days()?;
        Ok(())
    }

    fn sync_dividend_tax_days(&mut self) -> Result<(), SessionCorporateActionsError> {
        let receipts: Vec<(StockCode, crate::company::share_registry::ShareDayReceipt)> = self
            .registries
            .iter()
            .flat_map(|registry| {
                let stock = registry.stock().clone();
                registry
                    .receipts()
                    .iter()
                    .map(move |receipt| (stock.clone(), receipt.clone()))
            })
            .collect();
        let tax_book_keys: Vec<_> = self
            .dividend_tax_books
            .iter()
            .map(|book| (book.account(), book.stock().clone()))
            .collect();
        for (account, stock) in tax_book_keys {
            for (receipt_stock, receipt) in receipts.iter().filter(|(known, _)| known == &stock) {
                let event_id = format!(
                    "session-market:{}:{}:{}",
                    receipt_stock.0, account.0, receipt.request.day
                );
                let existing_day = self
                    .dividend_tax_books
                    .iter()
                    .find(|book| book.account() == account && book.stock() == receipt_stock)
                    .and_then(|book| book.receipt_by_event(&event_id))
                    .is_some();
                if existing_day {
                    continue;
                }
                let Some(change) = receipt
                    .request
                    .changes
                    .iter()
                    .find(|change| change.holder == HolderId::Account(account))
                else {
                    continue;
                };
                let acquisition = change.acquisition.as_ref().map(|acquisition| {
                    crate::company::cash_dividend_tax::DividendTaxLot {
                        id: format!("tax:{}", acquisition.lot_id),
                        qty: change
                            .change
                            .unsigned_abs()
                            .try_into()
                            .expect("positive change fits u64"),
                        acquired_on: receipt.request.day,
                        source: crate::company::cash_dividend_tax::convert_tax_source(
                            &acquisition.source,
                        ),
                        class: crate::company::cash_dividend_tax::convert_tax_class(
                            &acquisition.restriction,
                        ),
                    }
                });
                let book = self
                    .dividend_tax_books
                    .iter_mut()
                    .find(|book| book.account() == account && book.stock() == receipt_stock)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid(format!(
                            "账户 {account:?} 缺少 {receipt_stock:?} 的股息税账"
                        ))
                    })?;
                book.record_net_day(event_id, receipt.request.day, change.change, acquisition)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
        }
        Ok(())
    }

    pub(crate) fn validate(
        &self,
        accounts: &BTreeMap<AccountId, BTreeMap<StockCode, u64>>,
        company_system: &crate::company::CompanySystem,
        current_date: crate::calendar::CivilDate,
    ) -> Result<(), SessionCorporateActionsError> {
        let issuers = company_system.issuers();
        for tax_book in &self.dividend_tax_books {
            tax_book
                .validate()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if !accounts.contains_key(&tax_book.account()) {
                return Err(SessionCorporateActionsError::Invalid(
                    "股息税账引用了不存在的账户".into(),
                ));
            }
            if !self.registries.iter().any(|registry| {
                registry.stock() == tax_book.stock()
                    && registry
                        .holdings()
                        .iter()
                        .any(|holding| holding.holder == HolderId::Account(tax_book.account()))
            }) {
                return Err(SessionCorporateActionsError::Invalid(
                    "股息税账缺少对应的股东登记持有人".into(),
                ));
            }
        }
        let mut seen = BTreeSet::new();
        for registry in &self.registries {
            if !seen.insert(registry.stock().clone()) {
                return Err(SessionCorporateActionsError::Invalid(
                    "同一证券存在多个股东名册".into(),
                ));
            }
            Self::validate_registry(registry, accounts, issuers)?;
        }
        for book in &self.dividends {
            book.validate()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if book.plan().approved_on > current_date {
                return Err(SessionCorporateActionsError::Invalid(
                    "现金分红批准日期晚于当前会话日期".into(),
                ));
            }
            if !self.registries.iter().any(|registry| {
                registry.stock() == &book.plan().stock && registry.issuer() == &book.plan().issuer
            }) {
                return Err(SessionCorporateActionsError::Invalid(
                    "分红账簿缺少匹配的完整股东名册".into(),
                ));
            }
            let finance_plan = company_system
                .dividend_plan_facts(&book.plan().issuer)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?
                .into_iter()
                .find(|plan| plan.plan_id == book.plan().plan_id)
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("分红账簿缺少Simple批准事实".into())
                })?;
            if book.plan().approved_on != finance_plan.approved_on {
                return Err(SessionCorporateActionsError::Invalid(
                    "分红账簿批准日期与Simple事实不一致".into(),
                ));
            }
            if book.registration().is_none() {
                let registry = self
                    .registries
                    .iter()
                    .find(|registry| registry.stock() == &book.plan().stock)
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("分红计划缺少名册".into())
                    })?;
                let eligible = registry
                    .holdings()
                    .iter()
                    .filter(|holding| holding.holder != HolderId::IssuerTreasury)
                    .flat_map(|holding| holding.lots.iter())
                    .try_fold(0_u64, |sum, lot| sum.checked_add(lot.qty))
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("分红股份数量溢出".into())
                    })?;
                let gross_cents = i128::from(book.plan().gross_per_share.cents())
                    .checked_mul(i128::from(eligible))
                    .and_then(|cents| i64::try_from(cents).ok())
                    .ok_or_else(|| SessionCorporateActionsError::Invalid("分红总额溢出".into()))?;
                let gross = crate::accounting::AccountingAmount::from_money(
                    crate::money::Money::from_cents(gross_cents),
                );
                if gross != finance_plan.total_gross {
                    return Err(SessionCorporateActionsError::Invalid(
                        "未登记分红计划的名册总股数与Simple批准金额不一致".into(),
                    ));
                }
            }
        }
        company_system
            .validate_cash_dividend_books(&self.dividends)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        for book in &self.stock_distributions {
            book.validate()
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if book.plan().approved_on > current_date {
                return Err(SessionCorporateActionsError::Invalid(
                    "送转批准日期晚于当前会话日期".into(),
                ));
            }
            if !self.registries.iter().any(|registry| {
                registry.stock() == &book.plan().stock && registry.issuer() == &book.plan().issuer
            }) {
                return Err(SessionCorporateActionsError::Invalid(
                    "送转账簿缺少匹配的完整股东名册".into(),
                ));
            }
            if let Some(credited_on) = book.credited_on() {
                let registry = self
                    .registries
                    .iter()
                    .find(|registry| registry.stock() == &book.plan().stock)
                    .expect("registry presence was checked above");
                let expected_event = format!("stock-distribution:{}", book.plan().event_id);
                let matches = registry
                    .receipt_by_event(&expected_event)
                    .is_some_and(|receipt| {
                        receipt.request.day == credited_on
                            && matches!(
                                receipt.request.scope,
                                MovementScope::NonTradingTransfer { .. }
                            )
                            && receipt.request.changes.iter().try_fold(0_u64, |total, change| {
                                total.checked_add(
                                    u64::try_from(change.change).unwrap_or(0),
                                )
                            }) == Some(book.plan().approved_total_new_shares)
                    });
                if !matches {
                    return Err(SessionCorporateActionsError::Invalid(
                        "送转入账事实与股东名册非交易过户回执不一致".into(),
                    ));
                }
            }
        }
        company_system
            .validate_stock_distribution_books(&self.stock_distributions)
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        let mut group_keys = BTreeSet::new();
        for group in &self.applied_ex_reference_groups {
            if group.date > current_date
                || group.reference.ex_date != group.date
                || group.reference.reference_price.cents() <= 0
                || !group_keys.insert((group.date, group.stock.clone()))
                || (group.cash_plan_ids.is_empty() && group.stock_event_ids.is_empty())
            {
                return Err(SessionCorporateActionsError::Invalid(
                    "已应用除权除息组事实非法或重复".into(),
                ));
            }
            let mut expected_cash: Vec<_> = self
                .dividends
                .iter()
                .filter(|book| {
                    book.plan().stock == group.stock
                        && book.plan().ex_dividend_on == group.date
                        && book.registration().is_some()
                })
                .map(|book| book.plan().plan_id.clone())
                .collect();
            expected_cash.sort();
            let mut actual_cash = group.cash_plan_ids.clone();
            actual_cash.sort();
            let mut expected_stock: Vec<_> = self
                .stock_distributions
                .iter()
                .filter(|book| {
                    book.plan().stock == group.stock
                        && book.plan().ex_rights_on == group.date
                        && book.registration().is_some()
                })
                .map(|book| book.plan().event_id.clone())
                .collect();
            expected_stock.sort();
            let mut actual_stock = group.stock_event_ids.clone();
            actual_stock.sort();
            if expected_cash != actual_cash
                || actual_cash.windows(2).any(|pair| pair[0] == pair[1])
                || expected_stock != actual_stock
                || actual_stock.windows(2).any(|pair| pair[0] == pair[1])
            {
                return Err(SessionCorporateActionsError::Invalid(
                    "已应用除权除息组与登记分红方案或送转事件不一致".into(),
                ));
            }
        }
        let mut receipt_keys = BTreeSet::new();
        let mut account_receipt_keys = BTreeSet::new();
        for receipt in &self.account_gross_receipts {
            if receipt.paid_on > current_date {
                return Err(SessionCorporateActionsError::Invalid(
                    "账户到账日期晚于当前会话日期".into(),
                ));
            }
            if receipt.payment_id.trim().is_empty()
                || receipt.plan_id.trim().is_empty()
                || receipt.gross.cents() <= 0
                || !accounts.contains_key(&receipt.account)
                || !account_receipt_keys.insert((receipt.payment_id.clone(), receipt.account))
            {
                return Err(SessionCorporateActionsError::Invalid(
                    "账户 gross 到账凭证身份重复或事实非法".into(),
                ));
            }
            let book = self
                .dividends
                .iter()
                .find(|book| book.plan().plan_id == receipt.plan_id)
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("账户 gross 到账凭证缺少分红计划".into())
                })?;
            let tax_status = if self.dividend_tax_books.iter().any(|tax_book| {
                tax_book.account() == receipt.account && tax_book.stock() == &book.plan().stock
            }) {
                DividendTaxStatus::IndividualPublicMarket
            } else {
                DividendTaxStatus::TreatmentNotConfigured
            };
            if receipt.tax_status != tax_status {
                return Err(SessionCorporateActionsError::Invalid(
                    "账户股息税状态与显式税账配置不一致".into(),
                ));
            }
            let matches_payment = book.payments().iter().any(|payment| {
                payment.payment_id() == receipt.payment_id && payment.paid_on() == receipt.paid_on
                    && payment.outcomes().iter().any(|outcome| matches!(outcome,
                        crate::company::cash_dividend::HolderPaymentOutcome::Paid { holder: HolderId::Account(account), amount }
                        if account == &receipt.account && amount == &receipt.gross))
            });
            if !matches_payment {
                return Err(SessionCorporateActionsError::Invalid(
                    "账户 gross 到账凭证与分红成功付款回执不一致".into(),
                ));
            }
        }
        for receipt in &self.external_receipts {
            if receipt.paid_on > current_date {
                return Err(SessionCorporateActionsError::Invalid(
                    "External到账日期晚于当前会话日期".into(),
                ));
            }
            let HolderId::External(name) = &receipt.holder else {
                return Err(SessionCorporateActionsError::Invalid(
                    "外部收款凭证必须指向具名 External holder".into(),
                ));
            };
            if receipt.payment_id.trim().is_empty()
                || receipt.plan_id.trim().is_empty()
                || name.trim().is_empty()
                || receipt.gross.cents() <= 0
                || receipt.tax_status != DividendTaxStatus::TreatmentNotConfigured
                || !receipt_keys.insert((receipt.payment_id.clone(), receipt.holder.clone()))
            {
                return Err(SessionCorporateActionsError::Invalid(
                    "外部股东收款凭证身份重复或事实非法".into(),
                ));
            }
            let book = self
                .dividends
                .iter()
                .find(|book| book.plan().plan_id == receipt.plan_id)
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid("外部股东收款凭证缺少分红计划".into())
                })?;
            let matches_payment = book.payments().iter().any(|payment| {
                payment.payment_id() == receipt.payment_id
                    && payment.paid_on() == receipt.paid_on
                    && payment.outcomes().iter().any(|outcome| {
                        matches!(outcome,
                        crate::company::cash_dividend::HolderPaymentOutcome::Paid { holder, amount }
                        if holder == &receipt.holder && amount == &receipt.gross)
                    })
            });
            if !matches_payment {
                return Err(SessionCorporateActionsError::Invalid(
                    "外部股东收款凭证与分红成功付款回执不一致".into(),
                ));
            }
        }
        for book in &self.dividends {
            for payment in book.payments() {
                for outcome in payment.outcomes() {
                    if let crate::company::cash_dividend::HolderPaymentOutcome::Paid {
                        holder: HolderId::Account(account),
                        amount,
                    } = outcome
                    {
                        let tax_status = if self.dividend_tax_books.iter().any(|tax_book| {
                            tax_book.account() == *account && tax_book.stock() == &book.plan().stock
                        }) {
                            DividendTaxStatus::IndividualPublicMarket
                        } else {
                            DividendTaxStatus::TreatmentNotConfigured
                        };
                        if !self.account_gross_receipts.iter().any(|receipt| {
                            receipt.plan_id == book.plan().plan_id
                                && receipt.payment_id == payment.payment_id()
                                && receipt.paid_on == payment.paid_on()
                                && &receipt.account == account
                                && &receipt.gross == amount
                                && receipt.tax_status == tax_status
                        }) {
                            return Err(SessionCorporateActionsError::Invalid(
                                "已到账 Account holder 缺少 gross 到账凭证或 tax未配置标记".into(),
                            ));
                        }
                    }
                    if let crate::company::cash_dividend::HolderPaymentOutcome::Paid {
                        holder: HolderId::External(_),
                        amount,
                    } = outcome
                    {
                        if !self.external_receipts.iter().any(|receipt| {
                            receipt.plan_id == book.plan().plan_id
                                && receipt.payment_id == payment.payment_id()
                                && receipt.paid_on == payment.paid_on()
                                && &receipt.holder == outcome.holder()
                                && &receipt.gross == amount
                        }) {
                            return Err(SessionCorporateActionsError::Invalid(
                                "已付款的 External holder 缺少具名到账凭证".into(),
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn configure_registry(
        &mut self,
        registry: ShareRegistry,
        accounts: &BTreeMap<AccountId, BTreeMap<StockCode, u64>>,
        issuers: &crate::company::identity::IssuerRegistry,
    ) -> Result<(), SessionCorporateActionsError> {
        Self::validate_registry(&registry, accounts, issuers)?;
        if self
            .registries
            .iter()
            .any(|known| known.stock() == registry.stock())
        {
            return Err(SessionCorporateActionsError::Invalid(
                "证券已经配置股东名册".into(),
            ));
        }
        self.registries.push(registry);
        self.registries
            .sort_by(|left, right| left.stock().cmp(right.stock()));
        Ok(())
    }

    pub(crate) fn configure_cash_dividend_tax_book(
        &mut self,
        account: crate::orderbook::AccountId,
        stock: crate::account::StockCode,
        profile: crate::company::cash_dividend_tax::DividendTaxProfile,
    ) -> Result<(), SessionCorporateActionsError> {
        let registry = self
            .registries
            .iter()
            .find(|registry| registry.stock() == &stock)
            .ok_or_else(|| {
                SessionCorporateActionsError::Invalid(format!(
                    "配置股息税前必须先提供 {stock:?} 的完整股东名册"
                ))
            })?;
        let holding = registry
            .holdings()
            .iter()
            .find(|holding| holding.holder == HolderId::Account(account))
            .ok_or_else(|| {
                SessionCorporateActionsError::Invalid(format!(
                    "账户 {account:?} 不是 {stock:?} 的登记股东，不能配置股息税"
                ))
            })?;
        if self
            .dividend_tax_books
            .iter()
            .any(|book| book.account() == account && book.stock() == &stock)
        {
            return Err(SessionCorporateActionsError::Invalid(
                "同一账户证券已配置股息税身份".into(),
            ));
        }
        let lots = holding
            .lots
            .iter()
            .map(|lot| crate::company::cash_dividend_tax::DividendTaxLot {
                id: format!("tax:{}", lot.id),
                qty: lot.qty,
                acquired_on: lot.acquired_on,
                source: crate::company::cash_dividend_tax::convert_tax_source(&lot.source),
                class: crate::company::cash_dividend_tax::convert_tax_class(&lot.restriction),
            })
            .collect();
        let book = crate::company::cash_dividend_tax::CashDividendTaxBook::new(
            account,
            stock,
            profile,
            registry.settled_on(),
            lots,
        )
        .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        self.dividend_tax_books.push(book);
        self.dividend_tax_books.sort_by(|left, right| {
            (left.account(), left.stock()).cmp(&(right.account(), right.stock()))
        });
        Ok(())
    }

    fn validate_registry(
        registry: &ShareRegistry,
        accounts: &BTreeMap<AccountId, BTreeMap<StockCode, u64>>,
        issuers: &crate::company::identity::IssuerRegistry,
    ) -> Result<(), SessionCorporateActionsError> {
        registry
            .validate()
            .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        if issuers.issuer_of(registry.stock()) != Some(registry.issuer())
            || issuers
                .get(registry.issuer())
                .map(|issuer| issuer.issued_shares)
                != Some(registry.issued_shares())
        {
            return Err(SessionCorporateActionsError::Invalid(
                "证券、发行人及发行股数必须与公司身份完全一致".into(),
            ));
        }
        let mut seen_accounts = BTreeSet::new();
        let mut shares_by_account = BTreeMap::new();
        for holding in registry.holdings() {
            if let HolderId::Account(id) = &holding.holder {
                let id = *id;
                if !seen_accounts.insert(id) {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "账户 {id:?} 重复出现在股东名册"
                    )));
                }
                let shares = holding
                    .lots
                    .iter()
                    .try_fold(0_u64, |total, lot| total.checked_add(lot.qty))
                    .ok_or_else(|| {
                        SessionCorporateActionsError::Invalid("名册股份数量溢出".into())
                    })?;
                shares_by_account.insert(id, shares);
            }
        }
        for (account, positions) in accounts {
            let actual = positions.get(registry.stock()).copied().unwrap_or(0);
            if actual != shares_by_account.get(account).copied().unwrap_or(0) {
                return Err(SessionCorporateActionsError::Invalid(format!(
                    "账户 {account:?} 的实际持仓与名册不一致：账户 {actual} 股，名册 {} 股",
                    shares_by_account.get(account).copied().unwrap_or(0)
                )));
            }
        }
        if shares_by_account
            .keys()
            .any(|account| !accounts.contains_key(account))
        {
            return Err(SessionCorporateActionsError::Invalid(
                "名册引用了不存在的账户".into(),
            ));
        }
        Ok(())
    }

    fn settle_dividend_tax(
        &mut self,
        accounts: &mut super::account_book::AccountBook,
    ) -> Result<(), SessionCorporateActionsError> {
        let book_keys: Vec<_> = self
            .dividend_tax_books
            .iter()
            .map(|book| (book.account(), book.stock().clone()))
            .collect();
        for (account, stock) in book_keys {
            let tax_index = self
                .dividend_tax_books
                .iter()
                .position(|item| item.account() == account && *item.stock() == stock)
                .expect("tax book key came from the same collection");
            let collected_tax = self.dividend_tax_books[tax_index]
                .collections()
                .iter()
                .try_fold(crate::money::Money::ZERO, |total, receipt| {
                    total.add(receipt.collected)
                })
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            for dividend_book in &self.dividends {
                if dividend_book.plan().stock != stock {
                    continue;
                }
                let Some(snapshot) = dividend_book.registration() else {
                    continue;
                };
                if !snapshot
                    .holdings()
                    .iter()
                    .any(|holding| holding.holder == HolderId::Account(account))
                {
                    continue;
                }
                let index = tax_index;
                let event_id = format!("dividend-tax:{}", snapshot.event_id());
                let settled_on = self.dividend_tax_books[index].settled_on();
                if settled_on < snapshot.registered_on() {
                    continue;
                }
                if self.dividend_tax_books[index]
                    .dividend_event_by_id(&event_id)
                    .is_none()
                {
                    self.dividend_tax_books[index]
                        .register_dividend(
                            event_id,
                            snapshot.registered_on(),
                            crate::company::cash_dividend_tax::ExactDividendTaxAmount::new(
                                i128::from(
                                    dividend_book
                                        .entitlements()
                                        .map_err(|error| {
                                            SessionCorporateActionsError::Invalid(error.to_string())
                                        })?
                                        .iter()
                                        .find(|entitlement| {
                                            entitlement.holder == HolderId::Account(account)
                                        })
                                        .map(|entitlement| entitlement.gross.cents())
                                        .unwrap_or(0),
                                ),
                                snapshot
                                    .holdings()
                                    .iter()
                                    .find(|holding| holding.holder == HolderId::Account(account))
                                    .and_then(|holding| {
                                        holding.lots.iter().try_fold(0_u64, |total, lot| {
                                            total.checked_add(lot.qty)
                                        })
                                    })
                                    .unwrap_or(0),
                            )
                            .map_err(|error| {
                                SessionCorporateActionsError::Invalid(error.to_string())
                            })?,
                        )
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                }
            }

            let payment_keys: Vec<_> = self
                .dividends
                .iter()
                .flat_map(|book| {
                    book.payments().iter().map(move |payment| {
                        (book.plan().plan_id.clone(), payment.payment_id().to_owned())
                    })
                })
                .collect();
            for (plan_id, payment_id) in payment_keys {
                let Some(receipt) = self.account_gross_receipts.iter().find(|receipt| {
                    receipt.account == account
                        && receipt.plan_id == plan_id
                        && receipt.payment_id == payment_id
                }) else {
                    continue;
                };
                let index = tax_index;
                let registration_event_id = format!("dividend-tax:{}", plan_id);
                let settled_on = self.dividend_tax_books[index].settled_on();
                if settled_on < receipt.paid_on {
                    continue;
                }
                if self.dividend_tax_books[index]
                    .payment_by_id(&payment_id)
                    .is_none()
                    && self.dividend_tax_books[index]
                        .dividend_event_by_id(&registration_event_id)
                        .is_some()
                {
                    self.dividend_tax_books[index]
                        .record_payment(
                            &registration_event_id,
                            payment_id.clone(),
                            receipt.paid_on,
                            receipt.gross,
                            format!("account-gross:{payment_id}"),
                        )
                        .map_err(|error| {
                            SessionCorporateActionsError::Invalid(error.to_string())
                        })?;
                }
            }

            let index = tax_index;
            let settled_on = self.dividend_tax_books[index].settled_on();
            let available_cash = accounts
                .get(&account)
                .map(|account| account.cash())
                .ok_or_else(|| {
                    SessionCorporateActionsError::Invalid(format!("股息税账户 {account:?} 不存在"))
                })?;
            let collection_id =
                format!("dividend-tax-collect:{account:?}:{}:{settled_on}", stock.0);
            let base_available_cash = available_cash
                .add(collected_tax)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            let collection = self.dividend_tax_books[index]
                .collect_due(collection_id, settled_on, base_available_cash)
                .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            if collection.collected.cents() > 0 {
                accounts
                    .get_mut(&account)
                    .expect("the account existence was checked above")
                    .debit_cash(collection.collected)
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::StockCode,
        calendar::CivilDate,
        company::{
            share_registry::{AcquisitionSource, ShareHolding, ShareLot, ShareRestriction},
            CompanyId, CompanyKind, CompanySpec, IndustryId,
        },
    };

    fn d(year: i32, month: u8, day: u8) -> CivilDate {
        CivilDate::from_ymd(year, month, day).unwrap()
    }

    #[test]
    fn registry_requires_exact_issuer_security_and_real_account_positions() {
        let issuer = CompanyId("co".into());
        let code = StockCode("600000".into());
        let issuers = crate::company::identity::IssuerRegistry::new(vec![CompanySpec {
            id: issuer.clone(),
            name: "公司".into(),
            kind: CompanyKind::Industrial,
            industry: IndustryId("industry".into()),
            listed_stock: Some(code.clone()),
            issued_shares: 10,
            group_parent: None,
        }])
        .unwrap();
        let lot = ShareLot {
            id: "initial-account-1".into(),
            qty: 6,
            acquired_on: d(2026, 1, 1),
            source: AcquisitionSource::InitialAllocation {
                evidence: "explicit setup facts".into(),
            },
            restriction: ShareRestriction::Unrestricted,
        };
        let registry = ShareRegistry::new(
            code.clone(),
            issuer,
            10,
            d(2026, 1, 1),
            vec![
                ShareHolding {
                    holder: HolderId::Account(AccountId(1)),
                    lots: vec![lot],
                },
                ShareHolding {
                    holder: HolderId::External("named-holder".into()),
                    lots: vec![ShareLot {
                        id: "initial-external".into(),
                        qty: 4,
                        acquired_on: d(2026, 1, 1),
                        source: AcquisitionSource::InitialAllocation {
                            evidence: "explicit setup facts".into(),
                        },
                        restriction: ShareRestriction::Unrestricted,
                    }],
                },
            ],
        )
        .unwrap();
        let mut actions = SessionCorporateActions::default();
        assert!(actions
            .configure_registry(
                registry.clone(),
                &BTreeMap::from([(AccountId(1), BTreeMap::from([(code.clone(), 5)]))]),
                &issuers
            )
            .is_err());
        assert!(actions.registries.is_empty());
        actions
            .configure_registry(
                registry,
                &BTreeMap::from([(AccountId(1), BTreeMap::from([(code, 6)]))]),
                &issuers,
            )
            .unwrap();
        assert_eq!(actions.registries.len(), 1);
    }

    #[test]
    fn registry_day_end_advances_calendar_days_without_inventing_unlisted_holders() {
        let issuer = CompanyId("co".into());
        let code = StockCode("600000".into());
        let issuers = crate::company::identity::IssuerRegistry::new(vec![CompanySpec {
            id: issuer.clone(),
            name: "公司".into(),
            kind: CompanyKind::Industrial,
            industry: IndustryId("industry".into()),
            listed_stock: Some(code.clone()),
            issued_shares: 10,
            group_parent: None,
        }])
        .unwrap();
        let registry = ShareRegistry::new(
            code.clone(),
            issuer,
            10,
            d(2026, 10, 2),
            vec![
                ShareHolding {
                    holder: HolderId::Account(AccountId(1)),
                    lots: vec![ShareLot {
                        id: "known-lot".into(),
                        qty: 6,
                        acquired_on: d(2026, 10, 2),
                        source: AcquisitionSource::InitialAllocation {
                            evidence: "explicit setup fact".into(),
                        },
                        restriction: ShareRestriction::Unrestricted,
                    }],
                },
                ShareHolding {
                    holder: HolderId::External("named-holder".into()),
                    lots: vec![ShareLot {
                        id: "known-external-lot".into(),
                        qty: 4,
                        acquired_on: d(2026, 10, 2),
                        source: AcquisitionSource::InitialAllocation {
                            evidence: "explicit setup fact".into(),
                        },
                        restriction: ShareRestriction::Unrestricted,
                    }],
                },
            ],
        )
        .unwrap();
        let mut actions = SessionCorporateActions {
            registries: vec![registry],
            dividends: vec![],
            account_gross_receipts: vec![],
            external_receipts: vec![],
            dividend_tax_books: vec![],
            stock_distributions: vec![],
            applied_ex_reference_groups: vec![],
        };
        let accounts = BTreeMap::from([(AccountId(1), BTreeMap::from([(code, 6)]))]);
        actions
            .close_registries_through(d(2026, 10, 5), &accounts, &BTreeMap::new())
            .unwrap();
        assert_eq!(actions.registries[0].settled_on(), d(2026, 10, 5));
        assert_eq!(actions.registries[0].holdings().len(), 2);
        assert_eq!(
            actions.registries[0]
                .holdings()
                .iter()
                .map(|holding| holding.lots.iter().map(|lot| lot.qty).sum::<u64>())
                .sum::<u64>(),
            10
        );
        assert!(SessionCorporateActions::validate_registry(
            &actions.registries[0],
            &accounts,
            &issuers
        )
        .is_ok());
    }
}

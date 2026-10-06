use crate::company::share_registry::{
    AcquisitionSource, DayNetChange, HolderId, MovementScope, NetAcquisition, ShareDayRequest,
    ShareRegistry, ShareRestriction,
};
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
    #[ts(type = "import(\"../../save/schema/corporate-actions\").AccountDividendGrossReceipt[]")]
    pub account_gross_receipts: Vec<AccountDividendGrossReceipt>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").ExternalDividendReceipt[]")]
    pub external_receipts: Vec<ExternalDividendReceipt>,
    #[ts(type = "import(\"../../save/schema/corporate-actions\").AppliedCashExDividendGroup[]")]
    pub applied_ex_dividend_groups: Vec<AppliedCashExDividendGroup>,
}

#[derive(Clone, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(deny_unknown_fields)]
#[ts(export)]
pub struct AppliedCashExDividendGroup {
    pub date: crate::calendar::CivilDate,
    pub stock: StockCode,
    pub plan_ids: Vec<String>,
    pub reference: crate::company::ex_reference_price::ExReferencePrice,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum DividendTaxStatus {
    TreatmentNotConfigured,
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
                                        tax_status: DividendTaxStatus::TreatmentNotConfigured,
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
        Ok(())
    }

    pub(crate) fn validate(
        &self,
        accounts: &BTreeMap<AccountId, BTreeMap<StockCode, u64>>,
        company_system: &crate::company::CompanySystem,
        current_date: crate::calendar::CivilDate,
    ) -> Result<(), SessionCorporateActionsError> {
        let issuers = company_system.issuers();
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
        let mut group_keys = BTreeSet::new();
        for group in &self.applied_ex_dividend_groups {
            if group.date > current_date
                || group.reference.ex_date != group.date
                || group.reference.reference_price.cents() <= 0
                || !group_keys.insert((group.date, group.stock.clone()))
                || group.plan_ids.is_empty()
            {
                return Err(SessionCorporateActionsError::Invalid(
                    "已应用除息计划组事实非法或重复".into(),
                ));
            }
            let mut expected: Vec<_> = self
                .dividends
                .iter()
                .filter(|book| {
                    book.plan().stock == group.stock
                        && book.plan().ex_dividend_on == group.date
                        && book.registration().is_some()
                })
                .map(|book| book.plan().plan_id.clone())
                .collect();
            expected.sort();
            let mut actual = group.plan_ids.clone();
            actual.sort();
            if expected != actual || actual.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(SessionCorporateActionsError::Invalid(
                    "已应用除息计划组与登记分红方案不一致".into(),
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
                || receipt.tax_status != DividendTaxStatus::TreatmentNotConfigured
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
                        if !self.account_gross_receipts.iter().any(|receipt| {
                            receipt.plan_id == book.plan().plan_id
                                && receipt.payment_id == payment.payment_id()
                                && receipt.paid_on == payment.paid_on()
                                && &receipt.account == account
                                && &receipt.gross == amount
                                && receipt.tax_status == DividendTaxStatus::TreatmentNotConfigured
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        account::StockCode,
        calendar::CivilDate,
        company::{
            CompanyId, CompanyKind, CompanySpec, IndustryId,
            share_registry::{AcquisitionSource, ShareHolding, ShareLot, ShareRestriction},
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
        assert!(
            actions
                .configure_registry(
                    registry.clone(),
                    &BTreeMap::from([(AccountId(1), BTreeMap::from([(code.clone(), 5)]))]),
                    &issuers
                )
                .is_err()
        );
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
            applied_ex_dividend_groups: vec![],
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
        assert!(
            SessionCorporateActions::validate_registry(&actions.registries[0], &accounts, &issuers)
                .is_ok()
        );
    }
}

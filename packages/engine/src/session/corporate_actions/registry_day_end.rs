//! 股东名册与个人股息税账的独立日终候选计算；尚未接入生产日终链。

use super::*;
use crate::calendar::CivilDate;
use crate::company::cash_dividend_tax::CashDividendTaxBook;
use crate::company::share_registry::{
    AcquisitionSource, DayNetChange, MovementScope, NetAcquisition, ShareDayRequest,
    ShareRestriction,
};
use crate::session::PersonalTradeConfirmation;
use crate::stock::{StockCatalog, StockId};

/// 输入必须是普通撮合与收盘集合竞价全部完成后的最终持仓事实。
pub(crate) struct RegistryDayEndInput<'a> {
    pub day: CivilDate,
    pub catalog: &'a StockCatalog,
    pub accounts: &'a BTreeMap<AccountId, BTreeMap<StockId, u64>>,
    pub confirmations: &'a BTreeMap<AccountId, Vec<PersonalTradeConfirmation>>,
    pub issuer_repurchase_account: Option<AccountId>,
    /// 仅 AShareIndividual 模式传入个人身份集合；其他模式传 None。
    pub late_holder_tax_accounts: Option<&'a BTreeSet<AccountId>>,
}

/// 仅包含本相位计算出的候选事实；调用者决定是否安装。
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RegistryDayEndCandidate {
    pub registries: Vec<ShareRegistry>,
    pub dividend_tax_books: Vec<CashDividendTaxBook>,
}

impl SessionCorporateActions {
    /// 历史名册与税账须已通过整链校验；这里只新增 PublicMarket 日结事实。
    /// 不推进分红付款、回购 fill/cancel、除权或真实账户，尚无生产调用。
    pub(crate) fn registry_day_end_candidate(
        &self,
        input: RegistryDayEndInput<'_>,
    ) -> Result<RegistryDayEndCandidate, SessionCorporateActionsError> {
        let mut seen_receipts = BTreeSet::new();
        for (account, rows) in input.confirmations {
            if !input.accounts.contains_key(account) {
                return Err(invalid(format!("成交确认账户 {account:?} 不存在")));
            }
            for row in rows {
                if input.catalog.meta(row.stock).is_none() {
                    return Err(invalid(format!("成交确认未知证券 {:?}", row.stock)));
                }
                if row.civil_date > input.day {
                    return Err(invalid(format!(
                        "成交确认日期 {} 晚于日终 {}",
                        row.civil_date, input.day
                    )));
                }
                if row.quantity_shares == 0 {
                    return Err(invalid(format!("成交确认 {} 股数必须为正", row.receipt_id)));
                }
                if !seen_receipts.insert(row.receipt_id) {
                    return Err(invalid(format!("成交确认 {} 重复", row.receipt_id)));
                }
            }
        }
        let mut candidate = RegistryDayEndCandidate {
            registries: self.registries.clone(),
            dividend_tax_books: self.dividend_tax_books.clone(),
        };
        candidate.close_registries_through(
            input.day,
            input.accounts,
            input.confirmations,
            input.catalog,
            input.issuer_repurchase_account,
            input.late_holder_tax_accounts,
        )?;
        Ok(candidate)
    }
}

impl RegistryDayEndCandidate {
    /// 日终推进股东名册。`late_holder_tax_accounts` 是「后入股东自动开税账」
    /// 的资格集合（N2a，2026-10-08 决策）：仅 `AShareIndividual` 模式由 Session
    /// 侧传入「个人」身份账户全集，其他模式传 `None`（不开账）。首次经二级
    /// 市场净买入进入名册的账户按名册维护钩子同步开个人税账——这不是
    /// 「事后配置」：新入册账户此前不在名册中、无历史回执，税账从零重建
    /// FIFO 无事实丢失；入册前已持有期间的分红不追溯（该账户彼时不在
    /// 登记快照中，本就无分红事实）。
    fn close_registries_through(
        &mut self,
        through: crate::calendar::CivilDate,
        accounts: &BTreeMap<AccountId, BTreeMap<StockId, u64>>,
        confirmations: &BTreeMap<AccountId, Vec<PersonalTradeConfirmation>>,
        catalog: &StockCatalog,
        issuer_repurchase_account: Option<AccountId>,
        late_holder_tax_accounts: Option<&std::collections::BTreeSet<AccountId>>,
    ) -> Result<(), SessionCorporateActionsError> {
        // 本日经真实净买入首次入册的 (证券, 账户, 入册日)（N2a 后入股东）：
        // 名册推进循环结束后按资格集合同步开个人税账（见方法级注释的维护
        // 钩子语义），再进入税账日结同步。
        let mut late_entries: Vec<(StockCode, AccountId, crate::calendar::CivilDate)> = Vec::new();
        // 回购专户库存勾稽（Task174）不在本相位执行：当日回购成交此时尚未记入
        // 账簿（record_fill 在名册推进之后），lots/重放库存/账户持仓的三方对齐
        // 统一放在日终末尾的 validate 相位（那里 fills 已入账，且失败同样整日
        // 原子回滚）。

        let mut new_receipts = Vec::new();
        for registry in &mut self.registries {
            if registry.settled_on() > through {
                return Err(SessionCorporateActionsError::Invalid(
                    "股东名册日期晚于日终日期".into(),
                ));
            }
            let stock_id = resolve_stock_id(catalog, registry.stock())?;
            let original_receipts = registry.receipts().len();
            let mut day = registry.settled_on();
            while day < through {
                day = day
                    .next()
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                let mut net_by_account = BTreeMap::<AccountId, (i128, Vec<u64>)>::new();
                for (account, rows) in confirmations {
                    for row in rows
                        .iter()
                        .filter(|row| row.civil_date == day && row.stock == stock_id)
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
                // 回购账户与普通股东分账，归属不能取决于名册顺序或 Treasury 是否已入册。
                let (treasury_change, treasury_receipts) = issuer_repurchase_account
                    .and_then(|account| net_by_account.remove(&account))
                    .unwrap_or_default();
                if treasury_change < 0 {
                    return Err(SessionCorporateActionsError::Invalid(format!(
                        "回购账户当日净变动不能为负：证券 {}，日期 {day}，净股数 {treasury_change}",
                        registry.stock().0
                    )));
                }
                let mut changes = Vec::new();
                let mut treasury_registered = false;
                for holding in registry.holdings() {
                    let account = match &holding.holder {
                        HolderId::Account(account) => *account,
                        HolderId::IssuerTreasury => {
                            treasury_registered = true;
                            continue;
                        }
                        HolderId::External(_) => {
                            changes.push(DayNetChange {
                                holder: holding.holder.clone(),
                                change: 0,
                                acquisition: None,
                                consume: None,
                            });
                            continue;
                        }
                    };
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
                        consume: None,
                    });
                }
                if treasury_change > 0 || treasury_registered {
                    // 首次净买入沿用 close_day 的新持有人路径；零成交不虚构零股专户。
                    let acquisition = if treasury_change > 0 {
                        Some(NetAcquisition {
                            lot_id: treasury_market_lot_id(registry.stock(), day),
                            source: AcquisitionSource::SecondaryMarket {
                                settlement: treasury_receipts
                                    .iter()
                                    .map(u64::to_string)
                                    .collect::<Vec<_>>()
                                    .join(","),
                            },
                            restriction: ShareRestriction::Unrestricted,
                        })
                    } else {
                        None
                    };
                    changes.push(DayNetChange {
                        holder: HolderId::IssuerTreasury,
                        change: treasury_change,
                        acquisition,
                        consume: None,
                    });
                }
                for (account, (change, receipt_ids)) in net_by_account {
                    if change <= 0 || !accounts.contains_key(&account) {
                        return Err(SessionCorporateActionsError::Invalid(
                            "无既有名册的证券成交账户只能通过真实净买入进入股东登记".into(),
                        ));
                    }
                    late_entries.push((registry.stock().clone(), account, day));
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
                        consume: None,
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
            new_receipts.extend(
                registry.receipts()[original_receipts..]
                    .iter()
                    .cloned()
                    .map(|receipt| (registry.stock().clone(), receipt)),
            );
            reconcile_registry(
                registry,
                stock_id,
                through,
                accounts,
                issuer_repurchase_account,
            )?;
        }
        // 名册维护钩子（N2a，2026-10-08 决策「后续新进股东入册时同步补开个人
        // 税账」）：首次入册账户按资格开账；开账日取入册日的前一自然日（空
        // opening lots），紧随其后的税账同步把入册日的取得事实作为首条日结
        // 入账（税账首条日结必须紧邻开账日的次一自然日）。资格集合由 Session
        // 侧按三层税制模式过滤（仅 AShareIndividual 传「个人」身份账户全集，
        // Flat/Exempt 传 None 不开账）。
        for (stock, account, entry_day) in late_entries {
            if late_holder_tax_accounts.is_some_and(|eligible| eligible.contains(&account)) {
                let opened_on = entry_day
                    .prev()
                    .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
                self.register_late_holder_tax_book(account, stock, opened_on)?;
            }
        }
        self.sync_dividend_tax_days(&new_receipts)?;
        Ok(())
    }

    /// 后入股东的税账维护钩子（N2a）：为首次经二级市场净买入进入 `stock`
    /// 名册的 `account` 开 `IndividualPublicMarket` 个人税账。
    ///
    /// 与装配期 `configure_cash_dividend_tax_book` 的差异：不检查「名册已有
    /// 历史日结回执」守卫——该守卫防的是**事后**为早已持有者补账导致 FIFO
    /// 历史不可重建；本钩子只服务**首次入册**账户（调用方保证：账户来自
    /// `close_registries_through` 的日净额残差，即当日之前不在名册中、无任何
    /// 历史回执），税账从零重建 FIFO 无事实丢失。开账日 `opened_on` 取入册日
    /// 前一自然日且 opening lots 为空，入册日取得事实由紧随的
    /// `sync_dividend_tax_days` 作为首条日结入账。入册前已持有期间的分红不
    /// 追溯（该账户彼时不在登记快照中，本就无分红事实）。
    fn register_late_holder_tax_book(
        &mut self,
        account: AccountId,
        stock: StockCode,
        opened_on: crate::calendar::CivilDate,
    ) -> Result<(), SessionCorporateActionsError> {
        let registry = self
            .registries
            .iter()
            .find(|registry| registry.stock() == &stock)
            .ok_or_else(|| {
                SessionCorporateActionsError::Invalid(format!(
                    "后入股东 {account:?} 开税账缺少 {stock:?} 的股东名册"
                ))
            })?;
        if !registry
            .holdings()
            .iter()
            .any(|holding| holding.holder == HolderId::Account(account))
        {
            return Err(SessionCorporateActionsError::Invalid(format!(
                "后入股东 {account:?} 不在 {stock:?} 名册中，不能开个人税账"
            )));
        }
        if self
            .dividend_tax_books
            .iter()
            .any(|book| book.account() == account && book.stock() == &stock)
        {
            return Err(SessionCorporateActionsError::Invalid(format!(
                "账户 {account:?} 在 {stock:?} 已有个人税账，后入股东开账不可重复"
            )));
        }
        let book = crate::company::cash_dividend_tax::CashDividendTaxBook::new(
            account,
            stock,
            crate::company::cash_dividend_tax::DividendTaxProfile::IndividualPublicMarket,
            opened_on,
            Vec::new(),
        )
        .map_err(|error| SessionCorporateActionsError::Invalid(error.to_string()))?;
        self.dividend_tax_books.push(book);
        self.dividend_tax_books.sort_by(|left, right| {
            (left.account(), left.stock()).cmp(&(right.account(), right.stock()))
        });
        Ok(())
    }

    fn sync_dividend_tax_days(
        &mut self,
        receipts: &[(StockCode, crate::company::share_registry::ShareDayReceipt)],
    ) -> Result<(), SessionCorporateActionsError> {
        for book in &mut self.dividend_tax_books {
            let book_stock = book.stock().clone();
            for (stock, receipt) in receipts.iter().filter(|(stock, _)| stock == &book_stock) {
                let account = book.account();
                let Some(change) = receipt
                    .request
                    .changes
                    .iter()
                    .find(|change| change.holder == HolderId::Account(account))
                else {
                    continue;
                };
                let event_id = format!(
                    "session-market:{}:{}:{}",
                    stock.0, account.0, receipt.request.day
                );
                let acquisition = match &change.acquisition {
                    Some(acquisition) => Some(crate::company::cash_dividend_tax::DividendTaxLot {
                        id: format!("tax:{}", acquisition.lot_id),
                        qty: u64::try_from(change.change)
                            .map_err(|_| invalid("税账取得股数超出 u64"))?,
                        acquired_on: receipt.request.day,
                        source: crate::company::cash_dividend_tax::convert_tax_source(
                            &acquisition.source,
                        ),
                        class: crate::company::cash_dividend_tax::convert_tax_class(
                            &acquisition.restriction,
                        ),
                    }),
                    None => None,
                };
                book.record_net_day(event_id, receipt.request.day, change.change, acquisition)
                    .map_err(|error| invalid(error.to_string()))?;
            }
        }
        Ok(())
    }
}

fn invalid(reason: impl Into<String>) -> SessionCorporateActionsError {
    SessionCorporateActionsError::Invalid(reason.into())
}
fn resolve_stock_id(
    catalog: &StockCatalog,
    stock: &StockCode,
) -> Result<StockId, SessionCorporateActionsError> {
    catalog
        .id(&stock.0)
        .ok_or_else(|| invalid(format!("名册未知证券 {}", stock.0)))
}
fn treasury_market_lot_id(stock: &StockCode, day: CivilDate) -> String {
    format!("market:{}:issuer-treasury:{}", stock.0, day)
}

fn reconcile_registry(
    registry: &ShareRegistry,
    stock: StockId,
    date: CivilDate,
    accounts: &BTreeMap<AccountId, BTreeMap<StockId, u64>>,
    treasury_account: Option<AccountId>,
) -> Result<(), SessionCorporateActionsError> {
    let mut expected = BTreeMap::new();
    for holding in registry.holdings() {
        let account = match holding.holder {
            HolderId::Account(account) => {
                if Some(account) == treasury_account {
                    return Err(invalid("回购专户不能以 Account 身份入册"));
                }
                account
            }
            HolderId::IssuerTreasury => {
                treasury_account.ok_or_else(|| invalid("Treasury 名册缺少回购专户身份"))?
            }
            HolderId::External(_) => continue,
        };
        if !accounts.contains_key(&account) {
            return Err(invalid(format!("名册账户 {account:?} 不存在")));
        }
        let quantity = holding
            .lots
            .iter()
            .try_fold(0_u64, |sum, lot| sum.checked_add(lot.qty))
            .ok_or_else(|| invalid("名册股份溢出"))?;
        if expected.insert(account, quantity).is_some() {
            return Err(invalid("同一账户重复映射名册身份"));
        }
    }
    // 稀疏 positions 的缺席表示零股；账户本体缺席已在上方显式拒绝。
    for (account, positions) in accounts {
        let actual = positions.get(&stock).copied().unwrap_or(0);
        let expected = expected.get(account).copied().unwrap_or(0);
        if expected != actual {
            return Err(SessionCorporateActionsError::RegistryPositionMismatch {
                date,
                stock,
                account: *account,
                expected,
                actual,
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;

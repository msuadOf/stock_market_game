//! ordinary REG 与个人税日共用 civil shadow；不执行付款或公司股份 movement。

use super::*;
use crate::company::cash_dividend_tax::{
    day_boundary::TaxDayBoundaryFacts, CashDividendTaxBook, CashDividendTaxMode,
    DividendTaxProfile, ExactDividendTaxAmount,
};
use crate::company::share_registry::settlement_day::{
    ConfirmedShareMarketClose, ShareConfirmationRef, ShareMarketConfirmation, ShareMarketSource,
    ShareMarketSourceContext, SharePostAction, ShareSettlementDay,
};
use crate::company::share_registry::{
    AcquisitionSource, DayNetChange, HolderId, MovementScope, NetAcquisition, ShareDayRequest,
    ShareRestriction,
};

#[derive(Debug, thiserror::Error)]
#[error("ordinary 税日日结失败：日期 {day}，证券 {stock:?}：{source}")]
pub struct OrdinaryTaxDayEndError {
    day: crate::CivilDate,
    stock: Option<StockCode>,
    #[source]
    source: Box<dyn std::error::Error + Send + Sync>,
}

fn failure(
    day: crate::CivilDate,
    stock: Option<&StockCode>,
    source: impl std::error::Error + Send + Sync + 'static,
) -> SessionError {
    SessionError::OrdinaryTaxDayEnd(OrdinaryTaxDayEndError {
        day,
        stock: stock.cloned(),
        source: Box::new(source),
    })
}

fn invalid(detail: impl Into<String>) -> SessionCorporateActionsError {
    SessionCorporateActionsError::Invalid(detail.into())
}

impl GameSession {
    /// 仅在 civil shadow 上调用；交易日证明来自原 live World 的真实末拍探测。
    /// 同次 INFO/Admin 的固定成功尾负责安装 Companies，不在此提交第二次事务。
    pub(super) fn prepare_ordinary_tax_day_end(
        &mut self,
        day: crate::CivilDate,
        committed_market_close: bool,
    ) -> Result<(), SessionError> {
        let Some(actions) = self.state.companies.corporate_actions() else {
            return Ok(());
        };
        for book in &actions.dividend_tax_books {
            if !self.state.accounts.contains_key(&book.account()) {
                return Err(failure(
                    day,
                    Some(book.stock()),
                    invalid(format!("个人股息税账账户 {:?} 不存在", book.account())),
                ));
            }
        }
        if actions.registries.is_empty()
            && actions.dividends.is_empty()
            && actions.stock_distributions.is_empty()
            && actions.share_splits.is_empty()
            && actions.rights_offerings.is_empty()
        {
            return Ok(());
        }
        let accounts: BTreeMap<_, BTreeMap<_, _>> = self
            .state
            .accounts
            .iter()
            .map(|(id, account)| {
                (
                    *id,
                    account
                        .positions()
                        .iter()
                        .map(|(stock, position)| (*stock, u64::from(position.qty())))
                        .collect(),
                )
            })
            .collect();
        let treasury: Vec<_> = self
            .state
            .accounts
            .iter()
            .filter_map(|(id, account)| {
                (account.kind() == AccountKind::IssuerRepurchase).then_some(*id)
            })
            .collect();
        if treasury.len() > 1 || (treasury.len() == 1) != self.state.setup.issuer_repurchase_enabled
        {
            return Err(failure(day, None, invalid("回购专户与本局开关不一致")));
        }
        let treasury = treasury.first().copied();
        let mut confirmations = Vec::new();
        for (account, rows) in &self.state.personal_trade_confirmations {
            if !accounts.contains_key(account) {
                return Err(failure(
                    day,
                    None,
                    invalid(format!("最终确认账户 {account:?} 不存在")),
                ));
            }
            for row in rows {
                let code = self.state.stocks.code(row.stock).ok_or_else(|| {
                    failure(
                        day,
                        None,
                        invalid(format!(
                            "最终确认 {} 引用未知证券 {:?}",
                            row.receipt_id, row.stock
                        )),
                    )
                })?;
                if row.civil_date > day {
                    return Err(failure(
                        day,
                        None,
                        invalid(format!("最终确认 {} 晚于日结日期", row.receipt_id)),
                    ));
                }
                confirmations.push(ShareMarketConfirmation {
                    account: *account,
                    receipt_id: row.receipt_id,
                    stock: StockCode(code.into()),
                    day: row.civil_date,
                    side: row.side,
                    qty: u64::from(row.quantity_shares),
                });
            }
        }
        let calendar = self.state.civil_clock.calendar();
        let system = self
            .state
            .companies
            .company_system()
            .expect("公司行为容器只存在于 CompanySystem");
        let mut candidate = actions.clone();
        for book in &mut candidate.dividends {
            let plan = book.plan().clone();
            if book.status() == &crate::company::cash_dividend::CashDividendStatus::Approved {
                if plan.announced_on < day {
                    return Err(failure(
                        day,
                        Some(&plan.stock),
                        invalid(format!(
                            "分红方案 {} 错过公告日 {}",
                            plan.plan_id, plan.announced_on
                        )),
                    ));
                }
                if plan.announced_on == day {
                    book.announce(day)
                        .map_err(|source| failure(day, Some(&plan.stock), source))?;
                }
            }
            if book.registration().is_none() && plan.registered_on < day {
                return Err(failure(
                    day,
                    Some(&plan.stock),
                    invalid(format!(
                        "分红方案 {} 错过登记日 {}",
                        plan.plan_id, plan.registered_on
                    )),
                ));
            }
        }
        // 与市场成交使用同一个 shadow，公告或登记失败时不发布任何部分状态。
        for book in &mut candidate.stock_distributions {
            let plan = book.plan().clone();
            if book.status()
                == &crate::company::stock_distribution::StockDistributionStatus::Approved
            {
                if plan.announced_on < day {
                    return Err(failure(
                        day,
                        Some(&plan.stock),
                        invalid(format!("送转方案 {} 错过公告日", plan.event_id)),
                    ));
                }
                if plan.announced_on == day {
                    book.announce(day)
                        .map_err(|source| failure(day, Some(&plan.stock), source))?;
                }
            }
            if book.registration().is_none() && plan.registered_on < day {
                return Err(failure(
                    day,
                    Some(&plan.stock),
                    invalid(format!("送转方案 {} 错过登记日", plan.event_id)),
                ));
            }
        }
        for book in &mut candidate.share_splits {
            let plan = book.plan().clone();
            if book.status() == &crate::company::share_split::ShareSplitStatus::Approved {
                if plan.announced_on < day {
                    return Err(failure(
                        day,
                        Some(&plan.stock),
                        invalid(format!("拆缩股方案 {} 错过公告日", plan.event_id)),
                    ));
                }
                if plan.announced_on == day {
                    book.announce(day)
                        .map_err(|source| failure(day, Some(&plan.stock), source))?;
                }
            }
            if book.registration().is_none() && plan.registered_on < day {
                return Err(failure(
                    day,
                    Some(&plan.stock),
                    invalid(format!("拆缩股方案 {} 错过登记日", plan.event_id)),
                ));
            }
        }
        for book in &mut candidate.rights_offerings {
            let plan = book.plan().clone();
            if book.status() == &crate::company::rights_offering::RightsOfferingStatus::Approved {
                if plan.announced_on < day {
                    return Err(failure(
                        day,
                        Some(&plan.stock),
                        invalid(format!("配股方案 {} 错过公告日", plan.event_id)),
                    ));
                }
                if plan.announced_on == day {
                    book.announce(day)
                        .map_err(|source| failure(day, Some(&plan.stock), source))?;
                }
            }
            if book.registration().is_none() && plan.registered_on < day {
                return Err(failure(
                    day,
                    Some(&plan.stock),
                    invalid(format!("配股方案 {} 错过登记日", plan.event_id)),
                ));
            }
        }
        for index in 0..candidate.registries.len() {
            let registry = &candidate.registries[index];
            let stock = registry.stock().clone();
            let stock_id =
                self.state.stocks.id(&stock.0).ok_or_else(|| {
                    failure(day, Some(&stock), invalid("股东名册证券不在本局目录"))
                })?;
            let specification = self
                .state
                .setup
                .stocks
                .iter()
                .find(|spec| spec.code == stock)
                .ok_or_else(|| failure(day, Some(&stock), invalid("股东名册证券缺少冻结 setup")))?;
            let exchange = civil_clock::session_calendar_exchange(specification.exchange);
            let trading = calendar.is_trading_day(exchange, day)?;
            if trading && !committed_market_close {
                return Err(failure(
                    day,
                    Some(&stock),
                    invalid("交易日缺少真实 World 收盘完成证明"),
                ));
            }
            let rows: Vec<_> = confirmations
                .iter()
                .filter(|row| row.stock == stock && row.day == day)
                .collect();
            let existing_holders: BTreeSet<_> = registry
                .holdings()
                .iter()
                .map(|holding| &holding.holder)
                .collect();
            let mut net = BTreeMap::<HolderId, (i128, Vec<u64>)>::new();
            for holding in registry.holdings() {
                net.insert(holding.holder.clone(), (0, Vec::new()));
            }
            for row in &rows {
                let holder = if Some(row.account) == treasury {
                    HolderId::IssuerTreasury
                } else {
                    HolderId::Account(row.account)
                };
                let entry = net.entry(holder).or_default();
                let signed = if row.side == Side::Buy {
                    i128::from(row.qty)
                } else {
                    -i128::from(row.qty)
                };
                entry.0 = entry
                    .0
                    .checked_add(signed)
                    .ok_or_else(|| failure(day, Some(&stock), invalid("最终确认日净股份溢出")))?;
                entry.1.push(row.receipt_id);
            }
            let mut books: Vec<_> = candidate
                .dividend_tax_books
                .iter()
                .filter(|book| book.stock() == &stock)
                .cloned()
                .collect();
            if self.state.setup.dividend_tax_mode == Some(CashDividendTaxMode::AShareIndividual) {
                let tax_accounts: BTreeSet<_> = books.iter().map(|book| book.account()).collect();
                for holding in registry.holdings() {
                    let HolderId::Account(account) = holding.holder else {
                        continue;
                    };
                    if self.state.accounts.get(&account).is_some_and(|account| {
                        corporate_actions::taxpayer_identity_of_kind(account.kind())
                            == TaxpayerIdentity::Personal
                    }) && !tax_accounts.contains(&account)
                    {
                        return Err(failure(
                            day,
                            Some(&stock),
                            invalid(format!(
                                "已有个人持有人 {account:?} 缺少个人股息税账，禁止补建历史"
                            )),
                        ));
                    }
                }
            }
            let mut changes = Vec::with_capacity(net.len());
            for (holder, (change, mut receipt_ids)) in net {
                // 日净增按确认身份排序冻结来源，不依赖账户日志容器的迭代位置。
                receipt_ids.sort_unstable();
                if let HolderId::Account(account) = holder {
                    if !existing_holders.contains(&holder)
                        && change > 0
                        && self.state.setup.dividend_tax_mode
                            == Some(CashDividendTaxMode::AShareIndividual)
                        && self.state.accounts.get(&account).is_some_and(|account| {
                            corporate_actions::taxpayer_identity_of_kind(account.kind())
                                == TaxpayerIdentity::Personal
                        })
                        && !books.iter().any(|book| book.account() == account)
                    {
                        // 首次真实净买入入册的维护钩子；不补建既有持有人的历史税账。
                        books.push(
                            CashDividendTaxBook::new(
                                account,
                                stock.clone(),
                                DividendTaxProfile::IndividualPublicMarket,
                                registry.settled_on(),
                                Vec::new(),
                            )
                            .map_err(|source| failure(day, Some(&stock), source))?,
                        );
                    }
                }
                let acquisition = if change > 0 {
                    let suffix = match &holder {
                        HolderId::Account(account) => account.0.to_string(),
                        HolderId::IssuerTreasury => "issuer-treasury".into(),
                        HolderId::External(_) => {
                            return Err(failure(
                                day,
                                Some(&stock),
                                invalid("普通成交不能改变外部股东股份"),
                            ))
                        }
                    };
                    Some(NetAcquisition {
                        lot_id: format!("market:{}:{suffix}:{day}", stock.0),
                        source: AcquisitionSource::SecondaryMarket {
                            settlement: if holder == HolderId::IssuerTreasury {
                                format!("session-market:{}:{day}", stock.0)
                            } else {
                                receipt_ids
                                    .iter()
                                    .map(u64::to_string)
                                    .collect::<Vec<_>>()
                                    .join(",")
                            },
                        },
                        restriction: ShareRestriction::Unrestricted,
                    })
                } else {
                    None
                };
                changes.push(DayNetChange {
                    holder,
                    change,
                    acquisition,
                    consume: None,
                });
            }
            let source = if trading {
                ShareMarketSource::FinalConfirmations {
                    receipts: rows
                        .iter()
                        .map(|row| ShareConfirmationRef {
                            account: row.account,
                            receipt_id: row.receipt_id,
                        })
                        .collect(),
                }
            } else {
                ShareMarketSource::CalendarClosed {
                    exchange,
                    policy_digest: calendar.policy().content_digest().into(),
                }
            };
            let captures: Vec<_> = candidate
                .dividends
                .iter()
                .enumerate()
                .filter(|(_, book)| {
                    book.plan().stock == stock
                        && book.registration().is_none()
                        && book.plan().registered_on == day
                })
                .map(|(index, book)| (index, book.plan().clone()))
                .collect();
            let stock_captures: Vec<_> = candidate
                .stock_distributions
                .iter()
                .enumerate()
                .filter(|(_, book)| {
                    book.plan().stock == stock
                        && book.registration().is_none()
                        && book.plan().registered_on == day
                })
                .map(|(index, book)| (index, book.plan().event_id.clone()))
                .collect();
            let split_captures: Vec<_> = candidate
                .share_splits
                .iter()
                .enumerate()
                .filter(|(_, book)| {
                    book.plan().stock == stock
                        && book.registration().is_none()
                        && book.plan().registered_on == day
                })
                .map(|(index, book)| (index, book.plan().event_id.clone()))
                .collect();
            let rights_captures: Vec<_> = candidate
                .rights_offerings
                .iter()
                .enumerate()
                .filter(|(_, book)| {
                    book.plan().stock == stock
                        && book.registration().is_none()
                        && book.plan().registered_on == day
                })
                .map(|(index, book)| (index, book.plan().event_id.clone()))
                .collect();
            // Exempt 配股新股先作为本日唯一 REG 的收盘后 movement，
            // 后续 Admin 再以同一认购事实准备账户入股，不在此改真实账户。
            let mut issuance = Vec::new();
            let mut final_accounts = accounts.clone();
            for book in candidate.rights_offerings.iter().filter(|book| {
                book.plan().stock == stock
                    && book.status()
                        == &crate::company::rights_offering::RightsOfferingStatus::Closed
                    && book.plan().settlement_on == day
            }) {
                let plan = book.plan();
                if book
                    .determine_failure()
                    .map_err(|source| failure(day, Some(&stock), source))?
                {
                    continue;
                }
                if self.state.setup.dividend_tax_mode != Some(CashDividendTaxMode::Exempt)
                    || !books.is_empty()
                    || !candidate.applied_ex_reference_groups.iter().any(|group| {
                        group.stock == stock
                            && group.date == plan.ex_rights_on
                            && group.rights_event_ids == [plan.event_id.clone()]
                    })
                {
                    return Err(failure(
                        day,
                        Some(&stock),
                        invalid("成功配股缺少 Exempt 税制或真实除权锚"),
                    ));
                }
                let mut issuance_changes = Vec::new();
                for record in book
                    .subscriptions()
                    .iter()
                    .filter(|record| record.paid_shares > 0)
                {
                    let HolderId::Account(account) = &record.holder else {
                        return Err(failure(
                            day,
                            Some(&stock),
                            invalid("当前成功配股不支持外部股东或回购专户"),
                        ));
                    };
                    let positions = final_accounts.get_mut(account).ok_or_else(|| {
                        failure(day, Some(&stock), invalid("配股账户缺少真实市场持仓"))
                    })?;
                    let qty = positions.entry(stock_id).or_default();
                    *qty = qty
                        .checked_add(record.paid_shares)
                        .ok_or_else(|| failure(day, Some(&stock), invalid("配股账户股份溢出")))?;
                    issuance_changes.push(DayNetChange {
                        holder: record.holder.clone(),
                        change: i128::from(record.paid_shares),
                        acquisition: Some(NetAcquisition {
                            lot_id: format!(
                                "rights-offering:{}:account-{}",
                                plan.event_id, account.0
                            ),
                            source: AcquisitionSource::CorporateAction {
                                event: plan.event_id.clone(),
                            },
                            restriction: ShareRestriction::Unrestricted,
                        }),
                        consume: None,
                    });
                }
                if issuance_changes.is_empty() {
                    return Err(failure(day, Some(&stock), invalid("成功配股没有实缴股份")));
                }
                issuance.push(SharePostAction::Movement(ShareDayRequest {
                    event_id: format!("rights-offering:{}", plan.event_id),
                    day,
                    scope: MovementScope::NonTradingTransfer {
                        basis: plan.approval_reference.clone(),
                    },
                    changes: issuance_changes,
                }));
            }
            let mut cancellations = Vec::new();
            for book in candidate.issuer_repurchases.iter().filter(|book| {
                book.plan().stock == stock
                    && book.status()
                        == &crate::company::issuer_repurchase::IssuerRepurchaseStatus::Completed
                    && book.completed_on().is_some_and(|on| on < day)
            }) {
                use crate::company::share_registry::LotConsumption;
                let inventory = book
                    .current_inventory_shares(&[])
                    .map_err(|source| failure(day, Some(&stock), source))?;
                if inventory == 0 {
                    continue;
                }
                let mut consume = Vec::new();
                for fill in book.fills() {
                    let lots: Vec<_> = registry
                        .holdings()
                        .iter()
                        .filter(|holding| holding.holder == HolderId::IssuerTreasury)
                        .flat_map(|holding| &holding.lots)
                        .filter(|lot| {
                            lot.acquired_on == fill.day
                                && matches!(lot.source, AcquisitionSource::SecondaryMarket { .. })
                        })
                        .collect();
                    let [lot] = lots.as_slice() else {
                        return Err(failure(
                            day,
                            Some(&stock),
                            invalid("无法唯一归属本方案的 Treasury lot"),
                        ));
                    };
                    if lot.qty != fill.shares {
                        return Err(failure(
                            day,
                            Some(&stock),
                            invalid("Treasury lot 与本方案真实买入股数不等"),
                        ));
                    }
                    consume.push(LotConsumption {
                        lot_id: lot.id.clone(),
                        qty: lot.qty,
                    });
                }
                let owner = treasury
                    .ok_or_else(|| failure(day, Some(&stock), invalid("注销缺少回购专户")))?;
                let qty = final_accounts
                    .get_mut(&owner)
                    .and_then(|positions| positions.get_mut(&stock_id))
                    .ok_or_else(|| failure(day, Some(&stock), invalid("注销缺少回购专户持仓")))?;
                *qty = qty
                    .checked_sub(inventory)
                    .ok_or_else(|| failure(day, Some(&stock), invalid("注销股份超过专户持仓")))?;
                cancellations.push(SharePostAction::Movement(ShareDayRequest {
                    event_id: format!("issuer-repurchase-cancellation:{}", book.plan().event_id),
                    day,
                    scope: MovementScope::IssuerRepurchaseCancellation {
                        basis: book.plan().approval_reference.clone(),
                    },
                    changes: vec![DayNetChange {
                        holder: HolderId::IssuerTreasury,
                        change: -i128::from(inventory),
                        acquisition: None,
                        consume: Some(consume),
                    }],
                }));
            }
            let request = ShareSettlementDay {
                stock: stock.clone(),
                day,
                predecessor: registry
                    .settlement_frontier()
                    .map_err(|source| failure(day, Some(&stock), source))?,
                opening: registry.pending_opening().cloned().into_iter().collect(),
                actual_market_close: ConfirmedShareMarketClose {
                    unit: registry.current_unit(),
                    source,
                    request: ShareDayRequest {
                        event_id: format!("session-market:{}:{day}", stock.0),
                        day,
                        scope: MovementScope::PublicMarket,
                        changes,
                    },
                },
                post_market_actions: issuance
                    .into_iter()
                    .chain(
                        captures
                            .iter()
                            .map(|(_, plan)| SharePostAction::CaptureRegistration {
                                event_id: plan.plan_id.clone(),
                            })
                            .chain(
                                stock_captures
                                    .iter()
                                    .chain(&split_captures)
                                    .chain(&rights_captures)
                                    .map(|(_, event_id)| SharePostAction::CaptureRegistration {
                                        event_id: event_id.clone(),
                                    }),
                            ),
                    )
                    .chain(cancellations)
                    .collect(),
            };
            let sources = ShareMarketSourceContext {
                calendar,
                exchange,
                confirmations: &confirmations,
                treasury_account: treasury,
                // 非交易日由同一冻结日历证明；交易日已核原 World 的真实末拍。
                completed_through: Some(day),
            };
            let prepared = if self.state.setup.dividend_tax_mode
                == Some(CashDividendTaxMode::Exempt)
                && books.is_empty()
            {
                TaxDayBoundaryFacts::prepare_exempt(registry, &request, &sources)
            } else {
                TaxDayBoundaryFacts::prepare(registry, &books, &request, &sources)
            }
            .map_err(|source| failure(day, Some(&stock), source))?;
            corporate_actions::registry_day_end::reconcile_registry(
                prepared.registry(),
                stock_id,
                day,
                &final_accounts,
                treasury,
            )
            .map_err(|source| failure(day, Some(&stock), source))?;
            for (index, event_id) in stock_captures {
                let snapshot = prepared.registry().registration(&event_id).ok_or_else(|| {
                    failure(
                        day,
                        Some(&stock),
                        invalid(format!("送转方案 {event_id} 缺少登记快照")),
                    )
                })?;
                candidate.stock_distributions[index]
                    .register(snapshot.clone(), calendar)
                    .map_err(|source| failure(day, Some(&stock), source))?;
            }
            for (index, event_id) in split_captures {
                let snapshot = prepared.registry().registration(&event_id).ok_or_else(|| {
                    failure(
                        day,
                        Some(&stock),
                        invalid(format!("拆缩股方案 {event_id} 缺少登记快照")),
                    )
                })?;
                candidate.share_splits[index]
                    .register(snapshot.clone(), calendar)
                    .map_err(|source| failure(day, Some(&stock), source))?;
            }
            for (index, event_id) in rights_captures {
                let snapshot = prepared.registry().registration(&event_id).ok_or_else(|| {
                    failure(
                        day,
                        Some(&stock),
                        invalid(format!("配股方案 {event_id} 缺少登记快照")),
                    )
                })?;
                candidate.rights_offerings[index]
                    .entitle(snapshot.clone(), calendar)
                    .map_err(|source| failure(day, Some(&stock), source))?;
            }
            let mut registered_books = prepared.books().to_vec();
            for (dividend_index, plan) in captures {
                let snapshot =
                    prepared
                        .registry()
                        .registration(&plan.plan_id)
                        .ok_or_else(|| {
                            failure(
                                day,
                                Some(&stock),
                                invalid(format!(
                                    "分红方案 {} 缺少实际 typed capture",
                                    plan.plan_id
                                )),
                            )
                        })?;
                candidate.dividends[dividend_index]
                    .register(snapshot.clone(), calendar)
                    .map_err(|source| failure(day, Some(&stock), source))?;
                for entitlement in candidate.dividends[dividend_index]
                    .entitlements()
                    .map_err(|source| failure(day, Some(&stock), source))?
                {
                    let HolderId::Account(account) = entitlement.holder else {
                        continue;
                    };
                    let Some(book) = registered_books
                        .iter_mut()
                        .find(|book| book.account() == account)
                    else {
                        continue;
                    };
                    let market_book = prepared
                        .books()
                        .iter()
                        .find(|book| book.account() == account)
                        .expect("登记候选由同一市场候选克隆");
                    let attribution = prepared
                        .registrations()
                        .iter()
                        .find(|attribution| {
                            attribution.event_id() == plan.plan_id
                                && attribution.account() == account
                        })
                        .ok_or_else(|| {
                            failure(
                                day,
                                Some(&stock),
                                invalid(format!(
                                    "分红方案 {} 账户 {:?} 缺少真实税务登记归属",
                                    plan.plan_id, account
                                )),
                            )
                        })?;
                    // 多方案共用同一不可变市场尾；先登记的方案不改变后来方案的来源锚点。
                    if attribution.stock() != &stock
                        || attribution.registered_on() != day
                        || attribution.settled_receipts() != snapshot.settled_receipts()
                        || attribution.after_book_operation() != market_book.operation_seq()
                        || attribution.lots() != market_book.lots()
                        || book.lots() != market_book.lots()
                    {
                        return Err(failure(
                            day,
                            Some(&stock),
                            invalid(format!(
                                "分红方案 {} 账户 {:?} 的税务登记归属与实际市场尾不一致",
                                plan.plan_id, account
                            )),
                        ));
                    }
                    book.register_dividend(
                        format!("dividend-tax:{}", plan.plan_id),
                        day,
                        ExactDividendTaxAmount::new(i128::from(plan.gross_per_share.cents()), 1)
                            .map_err(|source| failure(day, Some(&stock), source))?,
                    )
                    .map_err(|source| failure(day, Some(&stock), source))?;
                }
            }
            candidate.registries[index] = prepared.registry().clone();
            candidate
                .dividend_tax_books
                .retain(|book| book.stock() != &stock);
            candidate.dividend_tax_books.extend(registered_books);
        }
        candidate.dividend_tax_books.sort_by(|left, right| {
            (left.account(), left.stock()).cmp(&(right.account(), right.stock()))
        });
        if let Some(book) = candidate
            .dividends
            .iter()
            .find(|book| book.plan().registered_on == day && book.registration().is_none())
        {
            return Err(failure(
                day,
                Some(&book.plan().stock),
                invalid(format!(
                    "分红方案 {} 缺少登记日的实际名册 capture",
                    book.plan().plan_id
                )),
            ));
        }
        for (event_id, stock, registered_on, missing) in candidate
            .stock_distributions
            .iter()
            .map(|book| {
                (
                    &book.plan().event_id,
                    &book.plan().stock,
                    book.plan().registered_on,
                    book.registration().is_none(),
                )
            })
            .chain(candidate.share_splits.iter().map(|book| {
                (
                    &book.plan().event_id,
                    &book.plan().stock,
                    book.plan().registered_on,
                    book.registration().is_none(),
                )
            }))
            .chain(candidate.rights_offerings.iter().map(|book| {
                (
                    &book.plan().event_id,
                    &book.plan().stock,
                    book.plan().registered_on,
                    book.registration().is_none(),
                )
            }))
        {
            if registered_on == day && missing {
                return Err(failure(
                    day,
                    Some(stock),
                    invalid(format!("资本方案 {event_id} 缺少登记日的实际名册 capture")),
                ));
            }
        }
        system
            .validate_stock_distribution_books(&candidate.stock_distributions)
            .map_err(|source| failure(day, None, source))?;
        system
            .validate_share_split_books(&candidate.share_splits)
            .map_err(|source| failure(day, None, source))?;
        system
            .validate_rights_offering_books(&candidate.rights_offerings)
            .map_err(|source| failure(day, None, source))?;
        // 发布前复用唯一公司规则核批准日期、金额、法律来源与既有付款映射。
        system
            .validate_cash_dividend_books(&candidate.dividends)
            .map_err(|source| failure(day, None, source))?;
        *self
            .state
            .companies
            .corporate_actions_mut()
            .expect("候选来自同一 CompanySystem") = candidate;
        Ok(())
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod payment_runtime_tests;

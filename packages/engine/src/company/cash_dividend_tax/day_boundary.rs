//! 普通市场完整税日来源；不从上市日或公司物理到账日推导税务日。
use super::*;
use crate::company::share_registry::settlement_day::{
    ShareMarketSourceContext, SharePostAction, SharePostActionRef, ShareSettlementDay,
    ShareUnitIdentity,
};
use crate::company::share_registry::{
    AcquisitionSource, DayNetChange, DisposedLot, HolderId, MovementScope, ShareDayRequest,
    ShareLot, ShareRegistry, ShareRegistryError, ShareRestriction,
};
use std::collections::{BTreeMap, VecDeque};

#[derive(Debug, thiserror::Error)]
pub(crate) enum TaxBoundaryError {
    #[error(transparent)]
    Registry(#[from] ShareRegistryError),
    #[error(transparent)]
    Tax(#[from] DividendTaxError),
    #[error("税务来源缺失：{0}")]
    MissingSource(String),
    #[error("税务登记归属缺失：account={account:?} stock={stock:?} day={day}: {detail}")]
    MissingAttribution {
        account: AccountId,
        stock: StockCode,
        day: CivilDate,
        detail: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaxRegistrationAttribution {
    event_id: String,
    registered_on: CivilDate,
    account: AccountId,
    stock: StockCode,
    after_book_operation: u64,
    settled_receipts: u64,
    lots: Vec<DividendTaxLot>,
}
impl TaxRegistrationAttribution {
    pub fn event_id(&self) -> &str {
        &self.event_id
    }
    pub fn registered_on(&self) -> CivilDate {
        self.registered_on
    }
    pub fn account(&self) -> AccountId {
        self.account
    }
    pub fn stock(&self) -> &StockCode {
        &self.stock
    }
    pub fn after_book_operation(&self) -> u64 {
        self.after_book_operation
    }
    pub fn settled_receipts(&self) -> u64 {
        self.settled_receipts
    }
    pub fn lots(&self) -> &[DividendTaxLot] {
        &self.lots
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaxDayBoundaryFacts {
    registry: ShareRegistry,
    books: Vec<CashDividendTaxBook>,
    registrations: Vec<TaxRegistrationAttribution>,
}
fn treasury_cancellation(request: &ShareDayRequest) -> bool {
    request
        .event_id
        .strip_prefix("issuer-repurchase-cancellation:")
        .is_some_and(|event| !event.trim().is_empty())
        && matches!(&request.scope, MovementScope::IssuerRepurchaseCancellation { basis } if !basis.trim().is_empty())
        && matches!(request.changes.as_slice(), [change]
            if change.holder == HolderId::IssuerTreasury && change.change < 0
                && change.acquisition.is_none() && change.consume.as_ref().is_some_and(|lots| !lots.is_empty()))
}

impl TaxDayBoundaryFacts {
    /// Exempt 不产生个人税账；仍由唯一 typed REG 校验真实市场来源与前驱。
    /// 仅允许明确配股发行的正向 movement，不能借此放宽其他公司行为。
    pub fn prepare_exempt(
        registry: &ShareRegistry,
        day: &ShareSettlementDay,
        sources: &ShareMarketSourceContext<'_>,
    ) -> Result<Self, TaxBoundaryError> {
        let expected_opening: Vec<_> = registry.pending_opening().cloned().into_iter().collect();
        if day.opening != expected_opening
            || day.actual_market_close.unit != registry.current_unit()
        {
            return Err(TaxBoundaryError::MissingSource(
                "Exempt 盘前 movement 或市场单位与已验证 REG 不一致".into(),
            ));
        }
        for action in &day.post_market_actions {
            match action {
                SharePostAction::CaptureRegistration { .. } => {}
                SharePostAction::Movement(request) if treasury_cancellation(request) => {}
                SharePostAction::Movement(request) => {
                    let Some(event) = request.event_id.strip_prefix("rights-offering:") else {
                        return Err(TaxBoundaryError::MissingSource(
                            "Exempt 仅支持配股发行 movement".into(),
                        ));
                    };
                    if event.trim().is_empty()
                        || !matches!(&request.scope, MovementScope::NonTradingTransfer { .. })
                        || request.changes.is_empty()
                        || request.changes.iter().any(|change| {
                            change.change <= 0 || change.consume.is_some()
                                || !matches!(&change.acquisition, Some(acquisition)
                                    if matches!(&acquisition.source, AcquisitionSource::CorporateAction { event: source } if source == event))
                        })
                    {
                        return Err(TaxBoundaryError::MissingSource("配股发行 movement 的身份或取得来源无效".into()));
                    }
                }
                SharePostAction::UnitMovement { .. } => {
                    return Err(TaxBoundaryError::MissingSource(
                        "Exempt 不支持单位换算".into(),
                    ))
                }
            }
        }
        let registry = registry.prepare_settlement_day(day, sources)?;
        Ok(Self {
            registry,
            books: Vec::new(),
            registrations: Vec::new(),
        })
    }

    /// 每证券只准备一份 REG；账户身份由 book 自身提供，不依赖输入位置。
    pub fn prepare(
        registry: &ShareRegistry,
        books: &[CashDividendTaxBook],
        day: &ShareSettlementDay,
        sources: &ShareMarketSourceContext<'_>,
    ) -> Result<Self, TaxBoundaryError> {
        let ordinary_history = registry.receipts().iter().all(|receipt| match &receipt.request.scope {
            MovementScope::SettlementDayMarket { binding } => {
                binding.opening.is_empty() && binding.market_unit == ShareUnitIdentity::Initial
                    && binding.post.iter().all(|action| match action {
                        SharePostActionRef::CaptureRegistration { .. } => true,
                        SharePostActionRef::Movement { event_id } => registry.receipt_by_event(event_id)
                            .is_some_and(|row| treasury_cancellation(&row.request)),
                        _ => false,
                    })
            }
            MovementScope::IssuerRepurchaseCancellation { .. } => treasury_cancellation(&receipt.request)
                && registry.receipts().iter().any(|market| matches!(&market.request.scope,
                    MovementScope::SettlementDayMarket { binding } if binding.post.iter().any(|action|
                        matches!(action, SharePostActionRef::Movement { event_id } if event_id == &receipt.request.event_id)))),
            _ => false,
        });
        if !ordinary_history
            || !day.opening.is_empty()
            || day.actual_market_close.unit != ShareUnitIdentity::Initial
            || day.post_market_actions.iter().any(|action| match action {
                SharePostAction::CaptureRegistration { .. } => false,
                SharePostAction::Movement(request) => !treasury_cancellation(request),
                _ => true,
            })
        {
            return Err(TaxBoundaryError::MissingSource(
                "ordinary 税日不支持其他历史或当日公司/单位 movement".into(),
            ));
        }
        // 由已有 REG 唯一实现核日历、水位、最终确认全集、前驱和物理净额。
        let candidate_registry = registry.prepare_settlement_day(day, sources)?;
        let before_holdings = index_holdings(registry.holdings());
        let after_holdings = index_holdings(candidate_registry.holdings());
        let history: Vec<_> = registry
            .receipts()
            .iter()
            .filter(|receipt| {
                matches!(
                    &receipt.request.scope,
                    MovementScope::SettlementDayMarket { .. }
                )
            })
            .map(|receipt| IndexedMarketDay::new(&receipt.request, &receipt.disposals))
            .collect();
        let market = IndexedMarketDay::new(&day.actual_market_close.request, &[]);
        let mut anchors = BTreeSet::from([registry.settled_on()]);
        for receipt in registry.receipts() {
            let MovementScope::SettlementDayMarket { binding } = &receipt.request.scope else {
                continue;
            };
            anchors.insert(receipt.request.day);
            anchors.insert(binding.predecessor.settled_on);
        }
        let mut captures = Vec::new();
        for action in &day.post_market_actions {
            let SharePostAction::CaptureRegistration { event_id } = action else {
                continue;
            };
            let snapshot = candidate_registry.registration(event_id).ok_or_else(|| {
                TaxBoundaryError::MissingSource(format!("capture={event_id} 缺少真实 REG snapshot"))
            })?;
            captures.push((snapshot, index_holdings(snapshot.holdings())));
        }
        let mut accounts = BTreeSet::new();
        let mut candidate_books = Vec::with_capacity(books.len());
        let mut registrations = Vec::new();
        for book in books {
            book.validate()?;
            if !accounts.insert(book.account()) {
                return Err(attribution_error(book, day.day, "同证券账户税账重复"));
            }
            if registry.stock() != book.stock()
                || registry.settled_on() != book.settled_on()
                || book.settled_on() != day.predecessor.settled_on
            {
                return Err(attribution_error(
                    book,
                    day.day,
                    "账户税账与 registry 的证券或完整期初日不一致",
                ));
            }
            if !book.redenominations.is_empty() || !book.stock_bonus_bases.is_empty() {
                return Err(TaxBoundaryError::MissingSource(
                    "ordinary 税日不支持公司/单位税账历史".into(),
                ));
            }
            let current_lots = verify_exact_lots(&before_holdings, book, day.day)?;
            verify_history(&history, &anchors, &current_lots, book, day.day)?;
            let (net_change, acquisition) = market.tax_facts(book.account())?;
            let mut candidate_book = book.clone();
            candidate_book.record_net_day(
                format!(
                    "session-market:{}:{}:{}",
                    book.stock().0,
                    book.account().0,
                    day.day
                ),
                day.day,
                net_change,
                acquisition,
            )?;
            verify_exact_lots(&after_holdings, &candidate_book, day.day)?;
            for (snapshot, holdings) in &captures {
                let lots = project_account_lots(holdings, book.account())?;
                if lots != candidate_book.lots() {
                    return Err(attribution_error(
                        book,
                        day.day,
                        "capture 完整 lot 与税日 frontier 不符",
                    ));
                }
                registrations.push(TaxRegistrationAttribution {
                    event_id: snapshot.event_id().into(),
                    registered_on: snapshot.registered_on(),
                    account: candidate_book.account(),
                    stock: candidate_book.stock().clone(),
                    after_book_operation: candidate_book.operation_seq,
                    settled_receipts: snapshot.settled_receipts(),
                    lots,
                });
            }
            candidate_books.push(candidate_book);
        }
        candidate_books.sort_by_key(|book| book.account());
        registrations.sort_by(|left, right| {
            (&left.event_id, left.account).cmp(&(&right.event_id, right.account))
        });
        Ok(Self {
            registry: candidate_registry,
            books: candidate_books,
            registrations,
        })
    }
    pub fn registry(&self) -> &ShareRegistry {
        &self.registry
    }
    pub fn books(&self) -> &[CashDividendTaxBook] {
        &self.books
    }
    pub fn registrations(&self) -> &[TaxRegistrationAttribution] {
        &self.registrations
    }
}
fn attribution_error(
    book: &CashDividendTaxBook,
    day: CivilDate,
    detail: impl Into<String>,
) -> TaxBoundaryError {
    TaxBoundaryError::MissingAttribution {
        account: book.account(),
        stock: book.stock().clone(),
        day,
        detail: detail.into(),
    }
}
fn project_lot(lot: &ShareLot) -> Result<DividendTaxLot, TaxBoundaryError> {
    if matches!(lot.source, AcquisitionSource::CorporateAction { .. })
        || lot.restriction != ShareRestriction::Unrestricted
    {
        return Err(TaxBoundaryError::MissingSource(
            "ordinary 税日不支持公司来源 lot 或未经核实的法定限售资格".into(),
        ));
    }
    Ok(DividendTaxLot {
        id: format!("tax:{}", lot.id),
        qty: lot.qty,
        acquired_on: lot.acquired_on,
        source: convert_tax_source(&lot.source),
        class: TaxShareClass::PublicMarket,
    })
}
fn index_holdings(
    holdings: &[crate::company::share_registry::ShareHolding],
) -> BTreeMap<AccountId, &[ShareLot]> {
    holdings
        .iter()
        .filter_map(|holding| match holding.holder {
            HolderId::Account(account) => Some((account, holding.lots.as_slice())),
            _ => None,
        })
        .collect()
}
fn project_account_lots(
    holdings: &BTreeMap<AccountId, &[ShareLot]>,
    account: AccountId,
) -> Result<Vec<DividendTaxLot>, TaxBoundaryError> {
    match holdings.get(&account) {
        Some(lots) => lots.iter().map(project_lot).collect(),
        None => Ok(Vec::new()),
    }
}
fn verify_exact_lots(
    holdings: &BTreeMap<AccountId, &[ShareLot]>,
    book: &CashDividendTaxBook,
    day: CivilDate,
) -> Result<Vec<DividendTaxLot>, TaxBoundaryError> {
    let expected = project_account_lots(holdings, book.account())?;
    if expected != book.lots() {
        return Err(attribution_error(
            book,
            day,
            format!(
                "完整 lot 归属不符：registry={expected:?} tax={:?}",
                book.lots()
            ),
        ));
    }
    Ok(expected)
}
/// 仅为同一批只读事实建账户索引，避免每本 book 重扫全部账户。
struct IndexedMarketDay<'a> {
    request: &'a ShareDayRequest,
    changes: BTreeMap<AccountId, &'a DayNetChange>,
    disposals: BTreeMap<AccountId, Vec<&'a DisposedLot>>,
}
impl<'a> IndexedMarketDay<'a> {
    fn new(request: &'a ShareDayRequest, disposals: &'a [DisposedLot]) -> Self {
        let changes = request
            .changes
            .iter()
            .filter_map(|change| match change.holder {
                HolderId::Account(account) => Some((account, change)),
                _ => None,
            })
            .collect();
        let mut indexed = BTreeMap::<_, Vec<_>>::new();
        for disposed in disposals {
            if let HolderId::Account(account) = disposed.holder {
                indexed.entry(account).or_default().push(disposed);
            }
        }
        Self {
            request,
            changes,
            disposals: indexed,
        }
    }
    fn tax_facts(
        &self,
        account: AccountId,
    ) -> Result<(i128, Option<DividendTaxLot>), TaxBoundaryError> {
        let Some(change) = self.changes.get(&account) else {
            return Ok((0, None));
        };
        let lot = if change.change > 0 {
            let acquisition = change.acquisition.as_ref().ok_or_else(|| {
                TaxBoundaryError::MissingSource("正净增缺失真实 acquisition".into())
            })?;
            if !matches!(
                acquisition.source,
                AcquisitionSource::SecondaryMarket { .. }
            ) {
                return Err(TaxBoundaryError::MissingSource(
                    "普通税日正净增必须来自实际 SecondaryMarket".into(),
                ));
            }
            Some(project_lot(&ShareLot {
                id: acquisition.lot_id.clone(),
                qty: u64::try_from(change.change)
                    .map_err(|_| TaxBoundaryError::MissingSource("实际税日净增超出u64".into()))?,
                acquired_on: self.request.day,
                source: acquisition.source.clone(),
                restriction: acquisition.restriction.clone(),
            })?)
        } else {
            None
        };
        Ok((change.change, lot))
    }
    fn tax_dispositions(
        &self,
        account: AccountId,
    ) -> Result<Vec<TaxDisposition>, TaxBoundaryError> {
        match self.disposals.get(&account) {
            Some(disposals) => disposals
                .iter()
                .map(|disposed| {
                    Ok(TaxDisposition {
                        lot: project_lot(&disposed.lot)?,
                        disposed_on: disposed.disposed_on,
                    })
                })
                .collect(),
            None => Ok(Vec::new()),
        }
    }
}
/// 完整尾部不能证明逐日税历史；每本 book 从自身已完成的 opening frontier 核验。
fn verify_history(
    history: &[IndexedMarketDay<'_>],
    anchors: &BTreeSet<CivilDate>,
    current_lots: &[DividendTaxLot],
    book: &CashDividendTaxBook,
    day: CivilDate,
) -> Result<(), TaxBoundaryError> {
    if !anchors.contains(&book.opened_on) {
        return Err(attribution_error(
            book,
            day,
            "税账 opening frontier 无真实 REG 已完成日对应",
        ));
    }
    let suffix = &history[history.partition_point(|source| source.request.day <= book.opened_on)..];
    if suffix.len() != book.days.len() {
        return Err(attribution_error(
            book,
            day,
            "税日与 opening frontier 后的 REG 自然日未一一对应",
        ));
    }
    for (receipt, tax) in suffix.iter().zip(&book.days) {
        let (net, acquisition) = receipt.tax_facts(book.account())?;
        if receipt.request.day != tax.day
            || net != tax.net_change
            || acquisition != tax.acquisition
            || receipt.tax_dispositions(book.account())? != tax.dispositions
        {
            return Err(attribution_error(
                book,
                tax.day,
                format!(
                    "历史税日与真实 REG 净额/取得/处置不符：market={} tax={}",
                    receipt.request.event_id, tax.event_id
                ),
            ));
        }
    }
    // 仅逆推来源元数据与数量来核 opening；不创建税处置、不重算税额。
    let mut opening: VecDeque<_> = current_lots.iter().cloned().collect();
    for receipt in suffix.iter().rev() {
        let (_, acquisition) = receipt.tax_facts(book.account())?;
        if let Some(acquisition) = acquisition {
            if opening.pop_back().as_ref() != Some(&acquisition) {
                return Err(attribution_error(
                    book,
                    receipt.request.day,
                    "历史取得 lot 完整来源或数量不符",
                ));
            }
        }
        for disposed in receipt.tax_dispositions(book.account())?.into_iter().rev() {
            if let Some(front) = opening.front_mut().filter(|lot| lot.id == disposed.lot.id) {
                let mut metadata = disposed.lot.clone();
                metadata.qty = front.qty;
                if *front != metadata {
                    return Err(attribution_error(
                        book,
                        receipt.request.day,
                        "历史 FIFO 处置来源与存活 lot 不符",
                    ));
                }
                front.qty = front.qty.checked_add(disposed.lot.qty).ok_or_else(|| {
                    attribution_error(book, receipt.request.day, "opening lot 数量逆推溢出")
                })?;
            } else {
                opening.push_front(disposed.lot);
            }
        }
    }
    if !opening.iter().eq(&book.opening_lots) {
        return Err(attribution_error(
            book,
            book.opened_on,
            "opening frontier 的完整 lot 归属不符",
        ));
    }
    Ok(())
}
#[cfg(test)]
#[path = "day_boundary_tests.rs"]
mod tests;

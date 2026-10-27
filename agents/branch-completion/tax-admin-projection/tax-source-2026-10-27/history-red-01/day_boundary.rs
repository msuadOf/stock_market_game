//! 普通市场完整税日来源；不从上市日或公司物理到账日推导税务日。
use super::*;
use crate::company::share_registry::settlement_day::{
    ShareMarketSourceContext, ShareSettlementDay,
};
use crate::company::share_registry::{ShareRegistry, ShareRegistryError};

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
    pub event_id: String,
    pub registered_on: CivilDate,
    pub account: AccountId,
    pub stock: StockCode,
    pub after_book_operation: u64,
    pub settled_receipts: u64,
    pub lots: Vec<DividendTaxLot>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TaxDayBoundaryFacts {
    registry: ShareRegistry,
    book: CashDividendTaxBook,
    registrations: Vec<TaxRegistrationAttribution>,
}
impl TaxDayBoundaryFacts {
    pub fn prepare(
        registry: &ShareRegistry,
        book: &CashDividendTaxBook,
        day: &ShareSettlementDay,
        sources: &ShareMarketSourceContext<'_>,
    ) -> Result<Self, TaxBoundaryError> {
        use crate::company::share_registry::settlement_day::{
            SharePostAction, SharePostActionRef, ShareUnitIdentity,
        };
        use crate::company::share_registry::{AcquisitionSource, HolderId, MovementScope};
        book.validate()?;
        // 普通入口不承诺任何公司变动或单位换算的税日；历史也须满足同一范围。
        let ordinary_history =
            registry
                .receipts()
                .iter()
                .all(|receipt| match &receipt.request.scope {
                    MovementScope::SettlementDayMarket { binding } => {
                        binding.opening.is_empty()
                            && binding.market_unit == ShareUnitIdentity::Initial
                            && binding.post.iter().all(|action| {
                                matches!(action, SharePostActionRef::CaptureRegistration { .. })
                            })
                    }
                    _ => false,
                });
        if !ordinary_history
            || !day.opening.is_empty()
            || day.actual_market_close.unit != ShareUnitIdentity::Initial
            || day
                .post_market_actions
                .iter()
                .any(|action| !matches!(action, SharePostAction::CaptureRegistration { .. }))
            || !book.redenominations.is_empty()
            || !book.stock_bonus_bases.is_empty()
        {
            return Err(TaxBoundaryError::MissingSource(
                "ordinary 税日不支持历史或当日公司/单位 movement，不能从物理日期推断税务日".into(),
            ));
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
        verify_exact_lots(registry.holdings(), book, day.day)?;
        // 由已有 REG 唯一实现核日历、水位、最终确认全集、前驱和物理净额。
        let candidate_registry = registry.prepare_settlement_day(day, sources)?;
        let change = day
            .actual_market_close
            .request
            .changes
            .iter()
            .find(|change| change.holder == HolderId::Account(book.account()));
        let (net_change, acquisition) = if let Some(change) = change {
            let acquisition = if change.change > 0 {
                let source = change.acquisition.as_ref().ok_or_else(|| {
                    TaxBoundaryError::MissingSource("正净增缺失真实 acquisition".into())
                })?;
                if !matches!(source.source, AcquisitionSource::SecondaryMarket { .. }) {
                    return Err(TaxBoundaryError::MissingSource(
                        "普通税日正净增必须来自实际 SecondaryMarket".into(),
                    ));
                }
                let class = public_market_class(&source.restriction)?;
                Some(DividendTaxLot {
                    id: format!("tax:{}", source.lot_id),
                    qty: u64::try_from(change.change).map_err(|_| {
                        TaxBoundaryError::MissingSource("实际税日净增超出 u64".into())
                    })?,
                    acquired_on: day.day,
                    source: convert_tax_source(&source.source),
                    class,
                })
            } else {
                None
            };
            (change.change, acquisition)
        } else {
            (0, None)
        };
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
        verify_exact_lots(candidate_registry.holdings(), &candidate_book, day.day)?;
        let mut registrations = Vec::new();
        for action in &day.post_market_actions {
            let SharePostAction::CaptureRegistration { event_id } = action else {
                unreachable!("ordinary 相位已校验");
            };
            let snapshot = candidate_registry.registration(event_id).ok_or_else(|| {
                TaxBoundaryError::MissingSource(format!("capture={event_id} 缺少真实 REG snapshot"))
            })?;
            verify_exact_lots(snapshot.holdings(), &candidate_book, day.day)?;
            registrations.push(TaxRegistrationAttribution {
                event_id: event_id.clone(),
                registered_on: snapshot.registered_on(),
                account: candidate_book.account(),
                stock: candidate_book.stock().clone(),
                after_book_operation: candidate_book.operation_seq,
                settled_receipts: snapshot.settled_receipts(),
                lots: candidate_book.lots().to_vec(),
            });
        }
        Ok(Self {
            registry: candidate_registry,
            book: candidate_book,
            registrations,
        })
    }
    pub fn registry(&self) -> &ShareRegistry {
        &self.registry
    }
    pub fn book(&self) -> &CashDividendTaxBook {
        &self.book
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
fn public_market_class(
    restriction: &crate::company::share_registry::ShareRestriction,
) -> Result<TaxShareClass, TaxBoundaryError> {
    match restriction {
        crate::company::share_registry::ShareRestriction::Unrestricted => {
            Ok(TaxShareClass::PublicMarket)
        }
        _ => Err(TaxBoundaryError::MissingSource(
            "ordinary 税日未取得法定限售税务资格，不能从物理 restriction 推断".into(),
        )),
    }
}
fn verify_exact_lots(
    holdings: &[crate::company::share_registry::ShareHolding],
    book: &CashDividendTaxBook,
    day: CivilDate,
) -> Result<(), TaxBoundaryError> {
    use crate::company::share_registry::{AcquisitionSource, HolderId};
    let mut expected = Vec::new();
    if let Some(holding) = holdings
        .iter()
        .find(|holding| holding.holder == HolderId::Account(book.account()))
    {
        for lot in &holding.lots {
            if matches!(lot.source, AcquisitionSource::CorporateAction { .. }) {
                return Err(TaxBoundaryError::MissingSource(
                    "ordinary 税日不支持公司来源 lot".into(),
                ));
            }
            expected.push(DividendTaxLot {
                id: format!("tax:{}", lot.id),
                qty: lot.qty,
                acquired_on: lot.acquired_on,
                source: convert_tax_source(&lot.source),
                class: public_market_class(&lot.restriction)?,
            });
        }
    }
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
    Ok(())
}
#[cfg(test)]
#[path = "day_boundary_tests.rs"]
mod tests;

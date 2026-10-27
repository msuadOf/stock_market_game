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
    books: Vec<CashDividendTaxBook>,
    registrations: Vec<TaxRegistrationAttribution>,
}
impl TaxDayBoundaryFacts {
    pub fn prepare(
        registry: &ShareRegistry,
        books: &[CashDividendTaxBook],
        day: &ShareSettlementDay,
        sources: &ShareMarketSourceContext<'_>,
    ) -> Result<Self, TaxBoundaryError> {
        let _ = (registry, books, day, sources);
        Err(TaxBoundaryError::MissingSource("批来源与历史归属尚未实现".into()))
    }
    pub fn registry(&self) -> &ShareRegistry {
        &self.registry
    }
    pub fn books(&self) -> &[CashDividendTaxBook] { &self.books }
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

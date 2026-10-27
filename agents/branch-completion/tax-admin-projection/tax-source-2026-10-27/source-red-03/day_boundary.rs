//! 普通市场完整税日来源；不从上市日或公司物理到账日推导税务日。
use super::*;
use crate::company::share_registry::{ShareRegistry, ShareRegistryError};
use crate::company::share_registry::settlement_day::{ShareSettlementDay, ShareMarketSourceContext};

#[derive(Debug, thiserror::Error)]
pub(crate) enum TaxBoundaryError {
    #[error(transparent)]
    Registry(#[from] ShareRegistryError),
    #[error(transparent)]
    Tax(#[from] DividendTaxError),
    #[error("税务来源缺失：{0}")]
    MissingSource(String),
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
    pub fn prepare(_registry: &ShareRegistry, _book: &CashDividendTaxBook, _day: &ShareSettlementDay, _sources: &ShareMarketSourceContext<'_>) -> Result<Self, TaxBoundaryError> {
        Err(TaxBoundaryError::MissingSource("真实 ordinary 来源尚未实现".into()))
    }
    pub fn registry(&self) -> &ShareRegistry { &self.registry }
    pub fn book(&self) -> &CashDividendTaxBook { &self.book }
    pub fn registrations(&self) -> &[TaxRegistrationAttribution] { &self.registrations }
}
#[cfg(test)]
#[path = "day_boundary_tests.rs"]
mod tests;

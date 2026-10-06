mod config;
mod environment;
pub mod finance;
mod finance_config;
pub mod growth;
pub mod period;
pub mod preferences;
mod state;
pub use config::{SimpleCompanyConfig, SimpleConfig, SimpleEnvironmentConfig};
pub use finance::{SimpleFinanceConfig, SimpleFinanceError, SimpleFinanceState, SimpleSummaryRule};
pub use preferences::{
    SimpleCashDividendPreference, SimpleCompanyPreferences, SimplePreferenceLedger,
    SimplePreferenceProposalKind, SimplePreferenceRejection, SimpleStockDistributionPreference,
};
pub(crate) use state::SimpleFundamentals;
#[cfg(test)]
mod preferences_tests;
#[cfg(test)]
mod tests;

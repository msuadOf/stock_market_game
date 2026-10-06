mod config;
mod environment;
pub mod finance;
mod finance_config;
pub mod growth;
pub mod period;
mod state;
pub use config::{SimpleCompanyConfig, SimpleConfig, SimpleEnvironmentConfig};
pub use finance::{
    DistributableProfit, DividendDeclaration, DividendLegalFacts, DividendPaymentFact,
    DividendPaymentReceipt, DividendPlanFact, DividendPlanReceipt, SimpleFinanceConfig,
    SimpleFinanceError, SimpleFinanceState, SimpleSummaryRule,
};
pub(crate) use state::SimpleFundamentals;
#[cfg(test)]
mod tests;

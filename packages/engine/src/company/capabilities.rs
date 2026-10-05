#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct CompanyCapabilities {
    pub revenue: bool,
    pub net_income: bool,
    pub equity: bool,
    pub cash_flow: bool,
    pub full_financial_statements: bool,
    pub cash_settlement: bool,
    pub unsupported_reason: String,
}

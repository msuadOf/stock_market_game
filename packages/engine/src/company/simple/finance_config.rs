use crate::accounting::{JournalLine, TaxPolicy};

#[derive(Clone, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct SimpleFinanceConfig {
    #[ts(type = "{ account: string; side: 'Debit' | 'Credit'; amount: string }[]")]
    pub opening_lines: Vec<JournalLine>,
    #[ts(
        type = "{ version: number; vat: { output_rate_bp: number; input_rate_bp: number; deductible_share_bp: number }; income_tax: { rate_bp: number; loss_carryforward_years: number } }"
    )]
    pub tax_policy: TaxPolicy,
    pub summary_rule: SimpleSummaryRule,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum SimpleSummaryRule {
    ReceivableRevenuePayableExpenses,
}

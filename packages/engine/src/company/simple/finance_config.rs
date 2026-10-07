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
    /// 账面展示参数（2026-10-08 用户决策）：严格持久化字段，无 serde 默认；
    /// 旧档缺该字段被显式拒绝（无兼容原则）。
    pub book_display: SimpleBookDisplayConfig,
}

#[derive(Clone, Copy, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
pub enum SimpleSummaryRule {
    ReceivableRevenuePayableExpenses,
}

/// Simple 账面展示参数（2026-10-08 用户决策原文：「公司账面现金/投资额展示字段：
/// 现金=累计留存收益（净利润−累计分红）、投资额=累计收入×固定比例（默认 30%，
/// 新局可编辑）；均为展示值不建模真实资金流（账面/真实分离铁律不变）」）。
#[derive(Clone, Copy, Eq, PartialEq, Debug, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[ts(export)]
#[serde(deny_unknown_fields)]
pub struct SimpleBookDisplayConfig {
    /// 投资额 = 累计收入 × 该比例（bp，0..=10000；默认 3000 = 30%）。
    pub investment_of_revenue_bp: i32,
}

impl SimpleBookDisplayConfig {
    /// 用户决策的默认比例：30%。
    pub const DEFAULT: Self = Self {
        investment_of_revenue_bp: 3_000,
    };

    pub fn validate(&self) -> Result<(), String> {
        if !(0..=10_000).contains(&self.investment_of_revenue_bp) {
            return Err(format!(
                "投资额展示比例必须为 0..10000 bp，当前 {} bp",
                self.investment_of_revenue_bp
            ));
        }
        Ok(())
    }
}

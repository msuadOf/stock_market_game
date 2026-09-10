//! 默认虚构公司集合（数据表）：5 家上市公司映射默认 5 股票（全部工商语义）
//! + 4 家未上市独立测试实体（四种 `CompanyKind` 各一，含一家集团子公司）。
//!
//! 全部经营配置均为**虚构游戏假设**（待校准，不声称真实行业参数）；不依据
//! 初始股价反推资产——实收资本 = 面值 1 元 × 总股本，与市价无关。2 年经营
//! 前史在任务 14 生成；本集合由任务 26 统一接入新游戏会话。默认 5 股票的
//! 交易类别/股本/交易所不因公司映射而改变。

use crate::account::StockCode;
use crate::accounting::{AccountingAmount, LedgerAccountId, PostingSide};
use crate::calendar::CivilDate;
use crate::company::contracts::{CreditLine, OperatingBudget};
use crate::company::counterparty::{CounterpartyId, CounterpartyKind, ExternalCounterparty};
use crate::company::error::CompanyError;
use crate::company::opening::{CompanyOpening, OpeningLine};
use crate::company::spec::{CompanyId, CompanyKind, CompanySpec, IndustryId};
use crate::company::CompanyConfig;

/// 通用开局科目代码（企业会计准则通用科目编号；图表 = generic v1）。
mod acct {
    pub const BANK: &str = "1002"; // 银行存款（现金类）
    pub const AR: &str = "1122"; // 应收账款
    pub const FIXED_ASSETS: &str = "1601"; // 固定资产
    pub const ST_DEBT: &str = "2001"; // 短期借款
    pub const CAPITAL: &str = "4001"; // 实收资本（面值 1 元 × 股本）
}

/// 元 → `AccountingAmount`（分）。
fn amount(yuan_value: i128) -> AccountingAmount {
    AccountingAmount::from_cents(yuan_value.checked_mul(100).expect("fixture yuan fits i128"))
}

fn line(code: &str, side: PostingSide, yuan_value: i128) -> OpeningLine {
    OpeningLine {
        account: LedgerAccountId(code.to_string()),
        side,
        amount: amount(yuan_value),
    }
}

/// 开局数字（元）：借 = 现金 + 应收 + 固定资产；贷 = 实收资本 + 短期借款。
/// 应收/借款为零时不设行（行金额恒正）。
struct OpeningFigures {
    cash: i128,
    receivables: i128,
    fixed_assets: i128,
    paid_in_capital: i128,
    short_term_debt: i128,
}

impl OpeningFigures {
    fn into_opening(self, as_of: CivilDate) -> CompanyOpening {
        let mut lines = vec![
            line(acct::BANK, PostingSide::Debit, self.cash),
            line(acct::FIXED_ASSETS, PostingSide::Debit, self.fixed_assets),
            line(acct::CAPITAL, PostingSide::Credit, self.paid_in_capital),
        ];
        if self.receivables > 0 {
            lines.insert(1, line(acct::AR, PostingSide::Debit, self.receivables));
        }
        if self.short_term_debt > 0 {
            lines.push(line(
                acct::ST_DEBT,
                PostingSide::Credit,
                self.short_term_debt,
            ));
        }
        CompanyOpening::generic_chart(as_of, lines)
    }
}

/// 一家公司的默认数据行。
struct CompanyRow {
    id: &'static str,
    name: &'static str,
    industry: &'static str,
    kind: CompanyKind,
    listed_stock: Option<&'static str>,
    issued_shares: u64,
    group_parent: Option<&'static str>,
    figures: OpeningFigures,
    /// 开局贷款人及授信上限（元）；`None` = 无借款无授信。
    lender: Option<(&'static str, i128)>,
    /// 贷款人之外的商业对手方。
    extra_counterparties: &'static [(&'static str, CounterpartyKind, &'static str)],
}

impl CompanyRow {
    fn into_config(self, as_of: CivilDate) -> Result<CompanyConfig, CompanyError> {
        let spec = CompanySpec {
            id: CompanyId(self.id.to_string()),
            name: self.name.to_string(),
            industry: IndustryId(self.industry.to_string()),
            kind: self.kind,
            listed_stock: self.listed_stock.map(|code| StockCode(code.to_string())),
            issued_shares: self.issued_shares,
            group_parent: self.group_parent.map(|id| CompanyId(id.to_string())),
        };
        let mut counterparties = Vec::new();
        if let Some((lender_id, _)) = self.lender {
            counterparties.push(ExternalCounterparty {
                id: CounterpartyId(lender_id.to_string()),
                kind: CounterpartyKind::Lender,
                name: "虚构合作银行".to_string(),
            });
        }
        for (id, kind, name) in self.extra_counterparties {
            counterparties.push(ExternalCounterparty {
                id: CounterpartyId((*id).to_string()),
                kind: *kind,
                name: (*name).to_string(),
            });
        }
        let credit_lines = self
            .lender
            .map(|(lender_id, limit)| {
                vec![CreditLine {
                    lender: CounterpartyId(lender_id.to_string()),
                    limit: amount(limit),
                }]
            })
            .unwrap_or_default();
        Ok(CompanyConfig {
            spec,
            opening: self.figures.into_opening(as_of),
            counterparties,
            budget: OperatingBudget::new(amount(1_000_000), credit_lines)?,
        })
    }
}

/// 默认虚构公司集合。5 家上市工商公司的 `issued_shares` 与默认 5 股票的
/// `total_shares` 逐字段一致（apps/web/src/config/defaults.ts；测试钉住）。
pub fn default_companies(as_of: CivilDate) -> Result<Vec<CompanyConfig>, CompanyError> {
    let rows = vec![
        // 稳健实业（600101，装备制造）：现金 60 亿 + 应收 5 亿 + 固定资产
        // 44.28571429 亿 = 实收资本 89.28571429 亿 + 短期借款 20 亿。
        CompanyRow {
            id: "C-600101",
            name: "稳健实业集团有限公司",
            industry: "machinery",
            kind: CompanyKind::Industrial,
            listed_stock: Some("600101"),
            issued_shares: 8_928_571_429,
            group_parent: None,
            figures: OpeningFigures {
                cash: 6_000_000_000,
                receivables: 500_000_000,
                fixed_assets: 4_428_571_429,
                paid_in_capital: 8_928_571_429,
                short_term_debt: 2_000_000_000,
            },
            lender: Some(("EXT-LDR-600101", 3_000_000_000)),
            extra_counterparties: &[(
                "EXT-CUST-600101",
                CounterpartyKind::Customer,
                "虚构装备客户",
            )],
        },
        // 芯片科技（002156，半导体）。
        CompanyRow {
            id: "C-002156",
            name: "芯片科技股份有限公司",
            industry: "semiconductor",
            kind: CompanyKind::Industrial,
            listed_stock: Some("002156"),
            issued_shares: 2_925_045_704,
            group_parent: None,
            figures: OpeningFigures {
                cash: 2_200_000_000,
                receivables: 300_000_000,
                fixed_assets: 1_225_045_704,
                paid_in_capital: 2_925_045_704,
                short_term_debt: 800_000_000,
            },
            lender: Some(("EXT-LDR-002156", 1_200_000_000)),
            extra_counterparties: &[],
        },
        // 短线题材（300260，文化传媒；创业板）。
        CompanyRow {
            id: "C-300260",
            name: "短线题材文化传媒股份有限公司",
            industry: "culture-media",
            kind: CompanyKind::Industrial,
            listed_stock: Some("300260"),
            issued_shares: 815_217_391,
            group_parent: None,
            figures: OpeningFigures {
                cash: 600_000_000,
                receivables: 115_217_391,
                fixed_assets: 400_000_000,
                paid_in_capital: 815_217_391,
                short_term_debt: 300_000_000,
            },
            lender: Some(("EXT-LDR-300260", 500_000_000)),
            extra_counterparties: &[],
        },
        // 人气妖股（600610，商贸零售）。
        CompanyRow {
            id: "C-600610",
            name: "人气妖股商贸股份有限公司",
            industry: "retail-trade",
            kind: CompanyKind::Industrial,
            listed_stock: Some("600610"),
            issued_shares: 1_059_602_649,
            group_parent: None,
            figures: OpeningFigures {
                cash: 900_000_000,
                receivables: 159_602_649,
                fixed_assets: 500_000_000,
                paid_in_capital: 1_059_602_649,
                short_term_debt: 500_000_000,
            },
            lender: Some(("EXT-LDR-600610", 800_000_000)),
            extra_counterparties: &[],
        },
        // ST 低价股（000812，钢铁）。
        CompanyRow {
            id: "C-000812",
            name: "低价钢铁股份有限公司",
            industry: "steel",
            kind: CompanyKind::Industrial,
            listed_stock: Some("000812"),
            issued_shares: 1_052_631_579,
            group_parent: None,
            figures: OpeningFigures {
                cash: 1_000_000_000,
                receivables: 52_631_579,
                fixed_assets: 1_500_000_000,
                paid_in_capital: 1_052_631_579,
                short_term_debt: 1_500_000_000,
            },
            lender: Some(("EXT-LDR-000812", 2_500_000_000)),
            extra_counterparties: &[],
        },
        // 未上市独立测试实体：工商（C-600101 的集团子公司）。
        CompanyRow {
            id: "C-TEST-IND",
            name: "测试工商实体（集团子公司）",
            industry: "machinery",
            kind: CompanyKind::Industrial,
            listed_stock: None,
            issued_shares: 100_000_000,
            group_parent: Some("C-600101"),
            figures: OpeningFigures {
                cash: 40_000_000,
                receivables: 5_000_000,
                fixed_assets: 65_000_000,
                paid_in_capital: 100_000_000,
                short_term_debt: 10_000_000,
            },
            lender: Some(("EXT-LDR-TEST-IND", 50_000_000)),
            extra_counterparties: &[
                (
                    "EXT-CUST-TEST-IND",
                    CounterpartyKind::Customer,
                    "虚构测试客户",
                ),
                (
                    "EXT-SUPP-TEST-IND",
                    CounterpartyKind::Supplier,
                    "虚构测试供应商",
                ),
            ],
        },
        // 未上市独立测试实体：银行。
        CompanyRow {
            id: "C-TEST-BANK",
            name: "测试银行实体",
            industry: "banking",
            kind: CompanyKind::Bank,
            listed_stock: None,
            issued_shares: 5_000_000_000,
            group_parent: None,
            figures: OpeningFigures {
                cash: 6_500_000_000,
                receivables: 100_000_000,
                fixed_assets: 400_000_000,
                paid_in_capital: 5_000_000_000,
                short_term_debt: 2_000_000_000,
            },
            lender: Some(("EXT-LDR-TEST-BANK", 3_000_000_000)),
            extra_counterparties: &[],
        },
        // 未上市独立测试实体：保险（无开局借款）。
        CompanyRow {
            id: "C-TEST-INS",
            name: "测试保险实体",
            industry: "insurance",
            kind: CompanyKind::Insurance,
            listed_stock: None,
            issued_shares: 10_000_000_000,
            group_parent: None,
            figures: OpeningFigures {
                cash: 8_000_000_000,
                receivables: 0,
                fixed_assets: 2_000_000_000,
                paid_in_capital: 10_000_000_000,
                short_term_debt: 0,
            },
            lender: None,
            extra_counterparties: &[],
        },
        // 未上市独立测试实体：地产。
        CompanyRow {
            id: "C-TEST-RE",
            name: "测试地产实体",
            industry: "real-estate-development",
            kind: CompanyKind::RealEstate,
            listed_stock: None,
            issued_shares: 2_000_000_000,
            group_parent: None,
            figures: OpeningFigures {
                cash: 1_200_000_000,
                receivables: 100_000_000,
                fixed_assets: 1_700_000_000,
                paid_in_capital: 2_000_000_000,
                short_term_debt: 1_000_000_000,
            },
            lender: Some(("EXT-LDR-TEST-RE", 2_000_000_000)),
            extra_counterparties: &[],
        },
    ];
    rows.into_iter().map(|row| row.into_config(as_of)).collect()
}

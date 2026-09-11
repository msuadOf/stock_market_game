//! 新游戏公司装配（任务 26）：把 [`SessionSetup`] 的股票清单装配为公司域
//! 全套开局状态——[`CompanyRegistry`]（发行人映射校验）+ 经营前史 + 公开库
//! （经 [`assemble_seeded_prehistory`]，任务 15）。
//!
//! 装配策略（文档化游戏假设，见 issues 登记）：
//! - 默认 5 股票（600101/002156/300260/600610/000812）复用
//!   [`crate::company::default_companies`] 数据表的开局数字；
//! - 其余股票按「实收资本 = 面值 1 元 × 总股本」推导通用工商公司
//!   （现金 30% + 固定资产 80% = 资本 + 短期借款 10%，确定性无随机）；
//! - 经营流参数按实收资本等比缩放（年化收入目标 ≈ 4 × 资本、税前利润率
//!   ≈ 12.5% 的**高周转简化**——使 PE 档位估值与常见股价同一量级；真实
//!   行业参数校准属后续任务，不声称现实口径）；
//! - 税务政策为显式版本化游戏假设（13%/13%/25%/5 年，与行业测试夹具
//!   同值；税法原文取证仍 blocked，见任务 2 登记）。
//!
//! 公司域种子与策略/会话 RNG 分流（K4）：`seed ^ COMPANY_SEED_TAG` 后经
//! `OperatingRng::derive` 二次派生，绝不与 self.rng 消费序列重叠。

use super::*;

use crate::accounting::{
    AccountingAmount, FixedAssetCode, InventoryItemCode, JournalLine, LedgerAccountId, PostingSide,
    TaxPolicy,
};
use crate::calendar::CivilDate;
use crate::company::events::ShockParams;
use crate::company::industrial::{
    industrial_chart_v2, IndustrialBooks, IndustrialConfig, OpeningAssetItem, OpeningDebtTerms,
};
use crate::company::operations::{
    CompanyOperationsConfig, FlowParams, IndustrialFlowParams, OperatingCompanyConfig,
};
use crate::company::{
    default_companies, CompanyConfig, CompanyId, CompanyKind, CompanyRegistry, CompanySpec,
    CounterpartyId, CounterpartyKind, CreditLine, ExternalCounterparty, IndustryId, OpeningLine,
    OperatingBudget,
};
use crate::information::{assemble_seeded_prehistory, SeededPrehistory};

/// 公司域种子标签（FNV-1a("company-operations")）：与策略/注意力/决策种子的
/// 派生模式互不相同，保证公司流与 NPC 流独立。
const COMPANY_SEED_TAG: u64 = 0x1a4d_706e_79f1_57c5;

/// 单只上市公司的装配产物：注册表配置（任务 7 域）+ 经营配置（任务 14 域）。
struct ListedCompanyConfigs {
    registry: CompanyConfig,
    operating: OperatingCompanyConfig,
}

/// 新游戏公司装配产物。
pub(crate) struct CompanyAssembly {
    pub registry: CompanyRegistry,
    pub prehistory: SeededPrehistory,
}

/// 装配新游戏的公司域全套开局状态。任一公司构造/映射校验失败 ⇒ 整体 Err
/// （无部分状态），由 [`SessionError::InvalidSetup`] 携带上下文显式上抛。
pub(super) fn assemble_companies(
    setup: &SessionSetup,
    seed: u64,
) -> Result<CompanyAssembly, SessionError> {
    // 前史首日 = 开局年 − 2 的 1 月 1 日；账套 as_of = 前史首日前一天
    // （generate_history 的调用方契约）。
    let as_of = CivilDate::from_ymd(setup.start_date.year() - 3, 12, 31)
        .map_err(|error| SessionError::InvalidSetup(format!("company as_of invalid: {error}")))?;
    let mut registry_configs = Vec::with_capacity(setup.stocks.len());
    let mut operating_configs = Vec::with_capacity(setup.stocks.len());
    let defaults = default_companies(as_of).map_err(company_error("default companies"))?;
    for stock in &setup.stocks {
        // 默认表命中条件 = 精确发行人匹配（代码 + 股本逐字相等）：股本不同的
        // setup 用表内开局数字会造成量纲错配（每股账面值失真数百倍），必须
        // 走通用推导。未命中 ⇒ 按总股本推导通用工商公司。
        let figures = defaults
            .iter()
            .find(|config| {
                config.spec.listed_stock.as_ref() == Some(&stock.code)
                    && config.spec.issued_shares == stock.total_shares
            })
            .map(Figures::from_default_row)
            .unwrap_or_else(|| Figures::from_total_shares(stock.total_shares));
        let listed = assemble_listed_company(stock, figures, as_of)?;
        registry_configs.push(listed.registry);
        operating_configs.push(listed.operating);
    }
    let registry = CompanyRegistry::new(registry_configs).map_err(company_error("registry"))?;
    let issuer_pairs: Vec<(StockCode, u64)> = setup
        .stocks
        .iter()
        .map(|stock| (stock.code.clone(), stock.total_shares))
        .collect();
    registry
        .validate_issuer_mapping(&issuer_pairs)
        .map_err(|error| SessionError::InvalidSetup(format!("issuer mapping invalid: {error}")))?;
    let operations_config = CompanyOperationsConfig {
        seed: seed ^ COMPANY_SEED_TAG,
        shock_params: ShockParams::default_v1(),
        companies: operating_configs,
    };
    let prehistory =
        assemble_seeded_prehistory(operations_config, setup.start_date).map_err(|error| {
            SessionError::InvalidSetup(format!("seeded prehistory failed: {error}"))
        })?;
    Ok(CompanyAssembly {
        registry,
        prehistory,
    })
}

fn company_error(stage: &'static str) -> impl Fn(crate::company::CompanyError) -> SessionError {
    move |error| SessionError::InvalidSetup(format!("{stage} failed: {error}"))
}

/// 开局数字（元）：借 = 现金 + 应收 + 固定资产；贷 = 实收资本 + 短期借款。
struct Figures {
    cash: i128,
    receivables: i128,
    fixed_assets: i128,
    paid_in_capital: i128,
    short_term_debt: i128,
}

impl Figures {
    /// 通用推导（游戏假设）：资本 = 面值 1 元 × 总股本；现金 30% + 固定资产
    /// 80% = 资本 + 短期借款 10%。各科目至少 1 元（极小股本取整归零会让开局
    /// 凭证出现非正行金额）。确定性纯算术，无随机。
    fn from_total_shares(total_shares: u64) -> Self {
        let capital = i128::from(total_shares).max(1);
        Figures {
            cash: (capital * 30 / 100).max(1),
            receivables: 0,
            fixed_assets: (capital * 80 / 100).max(1),
            paid_in_capital: capital,
            short_term_debt: (capital * 10 / 100).max(1),
        }
    }

    /// 从默认表行读取开局数字（元）。默认表行是 generic v1 开局凭证；这里
    /// 只取数字，行业账套（v2）由本模块统一重建。
    fn from_default_row(config: &CompanyConfig) -> Self {
        let mut cash = 0_i128;
        let mut receivables = 0_i128;
        let mut fixed_assets = 0_i128;
        let mut paid_in_capital = 0_i128;
        let mut short_term_debt = 0_i128;
        let to_yuan = |amount: &AccountingAmount| amount.cents() / 100;
        for line in &config.opening.lines {
            let yuan = to_yuan(&line.amount);
            match (line.account.0.as_str(), line.side) {
                ("1002", PostingSide::Debit) => cash += yuan,
                ("1122", PostingSide::Debit) => receivables += yuan,
                ("1601", PostingSide::Debit) => fixed_assets += yuan,
                ("4001", PostingSide::Credit) => paid_in_capital += yuan,
                ("2001", PostingSide::Credit) => short_term_debt += yuan,
                _ => {}
            }
        }
        Figures {
            cash,
            receivables,
            fixed_assets,
            paid_in_capital,
            short_term_debt,
        }
    }
}

fn yuan(value: i128) -> AccountingAmount {
    AccountingAmount::from_cents(
        value
            .checked_mul(100)
            .expect("listed company fixture yuan fits i128"),
    )
}

/// 注册表域开局行（generic v1 科目表）。
fn opening_line(code: &str, side: PostingSide, value: i128) -> OpeningLine {
    OpeningLine {
        account: LedgerAccountId(code.to_string()),
        side,
        amount: yuan(value),
    }
}

/// 工业账套域开局行（v2 科目表）。
fn journal_line(code: &str, side: PostingSide, value: i128) -> JournalLine {
    JournalLine {
        account: LedgerAccountId(code.to_string()),
        side,
        amount: yuan(value),
    }
}

/// 上市工商公司的显式版本化税务政策（游戏假设；税法原文取证 blocked，
/// 数值与行业测试夹具一致——现行大陆增值税标准税率 13%、企业所得税 25%）。
fn listed_tax_policy() -> TaxPolicy {
    TaxPolicy {
        version: 1,
        vat: crate::accounting::VatPolicy {
            output_rate_bp: 1_300,
            input_rate_bp: 1_300,
            deductible_share_bp: 10_000,
        },
        income_tax: crate::accounting::IncomeTaxPolicy {
            rate_bp: 2_500,
            loss_carryforward_years: 5,
        },
    }
}

/// 经营流参数（游戏假设，按实收资本缩放的「高周转简化」+ 按股票代码的
/// 确定性个体差异）：年化收入目标 ≈ 4 × 资本 × 代码派生倍率（1..=6）、
/// 税前利润率 ≈ 12.5%（±原料成本差异），使 PE 档位（8–24×）估值与常见
/// 初始股价处于同一量级且天然多空分歧。校准属后续任务；本函数确定性无随机。
fn listed_flow_params(code: &str, capital_yuan: i128) -> IndustrialFlowParams {
    const UNIT_PRICE_EXCL_VAT_CENTS: i128 = 4_000;
    const CONVERSION_CENTS_PER_UNIT: i128 = 800;
    const ADMIN_CENTS_PER_UNIT: i128 = 100;
    const TRADING_DAYS_PER_YEAR: i128 = 250;
    const REVENUE_TARGET_MULTIPLE: i128 = 4;
    // 代码派生确定性差异（FNV-1a，无随机）：收入倍率 1..=6、原料成本
    // 2400..=2800 分/件——同资本的不同公司财务规模自然分化。
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in code.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    let revenue_multiple = 1 + (hash % 6) as i128;
    let raw_unit_cost = 2_400 + ((hash >> 16) % 5) as i128 * 100;
    // demand = capital×4×倍率 / (unit_price_yuan × 250)，至少 100 件/日。
    let unit_price_yuan = UNIT_PRICE_EXCL_VAT_CENTS / 100;
    let demand = (capital_yuan * REVENUE_TARGET_MULTIPLE * revenue_multiple
        / (unit_price_yuan * TRADING_DAYS_PER_YEAR))
        .max(100);
    IndustrialFlowParams {
        customer: CounterpartyId("EXT-CUST-LISTED".to_string()),
        supplier: CounterpartyId("EXT-SUPP-LISTED".to_string()),
        raw_item: InventoryItemCode("RAW-1".to_string()),
        finished_item: InventoryItemCode("FG-1".to_string()),
        raw_account: LedgerAccountId("1403".to_string()),
        finished_account: LedgerAccountId("1405".to_string()),
        base_daily_demand_units: demand,
        unit_price_excl_vat: AccountingAmount::from_cents(UNIT_PRICE_EXCL_VAT_CENTS),
        receivable_credit_days: 5,
        raw_replenish_target_units: demand * 5,
        raw_unit_cost_excl_vat: AccountingAmount::from_cents(raw_unit_cost),
        daily_production_units: demand,
        daily_conversion_cost: AccountingAmount::from_cents(demand * CONVERSION_CENTS_PER_UNIT),
        daily_admin_expense: AccountingAmount::from_cents(demand * ADMIN_CENTS_PER_UNIT),
        bad_debt_base_bp: 500,
        asset_impairment_fraction_bp: 1_000,
    }
}

fn counterparty(id: &str, kind: CounterpartyKind) -> ExternalCounterparty {
    ExternalCounterparty {
        id: CounterpartyId(id.to_string()),
        kind,
        name: "虚构上市公司对手方".to_string(),
    }
}

fn assemble_listed_company(
    stock: &StockSpec,
    figures: Figures,
    as_of: CivilDate,
) -> Result<ListedCompanyConfigs, SessionError> {
    let company_id = CompanyId(format!("C-{}", stock.code.0));
    let spec = CompanySpec {
        id: company_id.clone(),
        name: format!("虚构上市公司{}", stock.code.0),
        industry: IndustryId("listed-industrial".to_string()),
        kind: CompanyKind::Industrial,
        listed_stock: Some(stock.code.clone()),
        issued_shares: stock.total_shares,
        group_parent: None,
    };
    spec.validate().map_err(company_error("company spec"))?;
    let lender_id = CounterpartyId(format!("EXT-LDR-{}", stock.code.0));
    // 注册表配置（任务 7 域：generic v1 科目表）。
    let mut registry_lines = vec![
        opening_line("1002", PostingSide::Debit, figures.cash),
        opening_line("1601", PostingSide::Debit, figures.fixed_assets),
        opening_line("4001", PostingSide::Credit, figures.paid_in_capital),
    ];
    if figures.receivables > 0 {
        registry_lines.insert(
            1,
            opening_line("1122", PostingSide::Debit, figures.receivables),
        );
    }
    if figures.short_term_debt > 0 {
        registry_lines.push(opening_line(
            "2001",
            PostingSide::Credit,
            figures.short_term_debt,
        ));
    }
    let registry_counterparties = vec![
        counterparty(
            &format!("EXT-CUST-{}", stock.code.0),
            CounterpartyKind::Customer,
        ),
        counterparty(
            &format!("EXT-SUPP-{}", stock.code.0),
            CounterpartyKind::Supplier,
        ),
        ExternalCounterparty {
            id: lender_id.clone(),
            kind: CounterpartyKind::Lender,
            name: "虚构合作银行".to_string(),
        },
    ];
    // 授信下限 1 元（极小股本公司的 20% 取整可能为 0；授信面必须为正）。
    let credit_limit = yuan((figures.paid_in_capital * 20 / 100).max(1));
    let registry = CompanyConfig {
        spec: spec.clone(),
        opening: crate::company::CompanyOpening::generic_chart(as_of, registry_lines),
        counterparties: registry_counterparties,
        budget: OperatingBudget::new(
            AccountingAmount::ZERO,
            vec![CreditLine {
                lender: lender_id.clone(),
                limit: credit_limit,
            }],
        )
        .map_err(company_error("operating budget"))?,
    };

    // 经营配置（任务 14 域：工业 v2 科目表 + 子账种子 + 流参数）。
    // 开局行只包含金额为正的科目（过账守卫拒绝非正行金额；通用推导的应收为 0）。
    let debt_maturity = CivilDate::from_ymd(as_of.year() + 2, 12, 31)
        .map_err(|error| SessionError::InvalidSetup(format!("debt maturity invalid: {error}")))?;
    let opening_debt = (figures.short_term_debt > 0).then(|| OpeningDebtTerms {
        lender: lender_id.clone(),
        principal: yuan(figures.short_term_debt),
        annual_rate_bp: 365,
        maturity_date: debt_maturity,
    });
    let mut industrial_lines = vec![
        journal_line("1002", PostingSide::Debit, figures.cash),
        journal_line("1601", PostingSide::Debit, figures.fixed_assets),
        journal_line("2001", PostingSide::Credit, figures.short_term_debt),
        journal_line("4001", PostingSide::Credit, figures.paid_in_capital),
    ];
    if figures.receivables > 0 {
        industrial_lines.insert(
            1,
            journal_line("1122", PostingSide::Debit, figures.receivables),
        );
    }
    let industrial = IndustrialConfig {
        chart: industrial_chart_v2(),
        as_of,
        opening_lines: industrial_lines,
        opening_inventory: Vec::new(),
        opening_assets: (figures.fixed_assets > 0)
            .then(|| OpeningAssetItem {
                code: FixedAssetCode("FA-1".to_string()),
                cost: yuan(figures.fixed_assets),
                salvage_value: AccountingAmount::ZERO,
                life_months: 120,
            })
            .into_iter()
            .collect(),
        opening_debt,
        counterparties: vec![
            counterparty("EXT-CUST-LISTED", CounterpartyKind::Customer),
            counterparty("EXT-SUPP-LISTED", CounterpartyKind::Supplier),
            ExternalCounterparty {
                id: lender_id,
                kind: CounterpartyKind::Lender,
                name: "虚构合作银行".to_string(),
            },
        ],
        budget: OperatingBudget::new(
            AccountingAmount::ZERO,
            vec![CreditLine {
                lender: CounterpartyId(format!("EXT-LDR-{}", stock.code.0)),
                limit: credit_limit,
            }],
        )
        .map_err(company_error("operating budget"))?,
        tax_policy: listed_tax_policy(),
    };
    let books = IndustrialBooks::new(industrial)
        .map_err(|error| SessionError::InvalidSetup(format!("industrial books failed: {error}")))?;
    let operating = OperatingCompanyConfig {
        spec,
        books: crate::company::operations::IndustryBooks::Industrial(books),
        flow: FlowParams::Industrial(listed_flow_params(&stock.code.0, figures.paid_in_capital)),
    };
    Ok(ListedCompanyConfigs {
        registry,
        operating,
    })
}

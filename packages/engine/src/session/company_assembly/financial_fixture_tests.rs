use super::*;

use crate::accounting::{
    AccountingAmount, FixedAssetCode, InventoryItemCode, JournalLine, LedgerAccountId, PostingSide,
    TaxPolicy,
};
use crate::calendar::CivilDate;
use crate::company::events::ShockParams;
use crate::company::industrial::{
    industrial_account_chart, IndustrialBooks, IndustrialConfig, OpeningAssetItem, OpeningDebtTerms,
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


struct ListedCompanyConfigs { registry: CompanyConfig, operating: OperatingCompanyConfig }

pub(crate) fn hash_financial_fixture() -> (CompanyRegistry, SeededPrehistory) {
    let start = CivilDate::from_iso("2030-01-01").unwrap();
    let as_of = CivilDate::from_iso("2027-12-31").unwrap();
    let stock = StockSpec {
        code: StockCode("600888".into()),
        exchange: StockExchange::Shanghai,
        initial_price: Money::from_cents(1_000),
        category: SecurityCategory::MainBoard,
        limit_pct: 0.10,
        tick: Money::from_cents(1),
        total_shares: 10_000_000,
        float_shares: 0,
    };
    let listed = assemble_listed_company(&stock, OpeningFigures::from_total_shares(stock.total_shares), as_of).unwrap();
    let registry = CompanyRegistry::new(vec![listed.registry]).unwrap();
    let mut shock_params = ShockParams::current_default_parameters();
    shock_params.market_candidate_bp = 0;
    shock_params.industry_candidate_bp = 0;
    shock_params.company_candidate_bp = 0;
    let history = assemble_seeded_prehistory(CompanyOperationsConfig {
        seed: 42,
        shock_params,
        companies: vec![listed.operating],
    }, start, crate::information::ReportFrequency::Quarterly).unwrap();
    (registry, history)
}

pub(crate) fn confirmed_announcement_fixture(
    session: &GameSession,
    shock: crate::company::ActiveShock,
) -> (crate::information::PublicLibrary, DayEndDisclosures, CivilDayEndReport) {
    let date = session.civil_date();
    let stock = &session.state.setup.stocks[0];
    let as_of = CivilDate::from_ymd(date.year() - 3, 12, 31).unwrap();
    let listed = assemble_listed_company(stock, OpeningFigures::from_total_shares(stock.total_shares), as_of).unwrap();
    let company = listed.operating.spec.id.clone();
    let mut params = ShockParams::current_default_parameters();
    params.market_candidate_bp = 0;
    params.industry_candidate_bp = 0;
    params.company_candidate_bp = 0;
    let mut history = assemble_seeded_prehistory(CompanyOperationsConfig {
        seed: 42,
        shock_params: params,
        companies: vec![listed.operating],
    }, date, session.state.setup.report_frequency).unwrap();
    history.ops.apply_company_shock(&company, shock).unwrap();
    let mut clock = CivilClock::new_for_stocks(date, [(stock.code.clone(), session_calendar_exchange(stock.exchange))]).unwrap();
    let mut wiring = CompanyOperationsClockWiring::new();
    wiring.install(&mut clock, &history.ops).unwrap();
    let report = clock.end_day(date).unwrap();
    wiring.run_day_end(&report, &mut clock, &mut history.ops).unwrap();
    let mut library = session.state.library.as_ref().clone();
    let mut dispatch = DisclosureDispatch::new(library.latest_published_instant());
    let disclosures = dispatch.run_day_end(DayEndDisclosureCtx {
        report_frequency: session.state.setup.report_frequency,
        groups: &[],
        report: &report,
        ops: &history.ops,
        closing: &mut history.closing,
        library: &mut library,
    }).unwrap();
    assert!(!disclosures.announcements_published.is_empty());
    (library, disclosures, report)
}

fn company_error(stage: &'static str) -> impl Fn(crate::company::CompanyError) -> SessionError {
    move |error| SessionError::InvalidSetup(format!("{stage} failed: {error}"))
}

/// 开局数字（元）：借 = 现金 + 应收 + 固定资产；贷 = 实收资本 + 短期借款。
struct OpeningFigures {
    cash: i128,
    receivables: i128,
    fixed_assets: i128,
    paid_in_capital: i128,
    short_term_debt: i128,
}

impl OpeningFigures {
    /// 精确命中默认发行人（代码 + 股本），其余开局事实由总股本推导。
    fn for_stock(stock: &StockSpec, defaults: &[CompanyConfig]) -> Self {
        defaults
            .iter()
            .find(|config| {
                config.spec.listed_stock.as_ref() == Some(&stock.code)
                    && config.spec.issued_shares == stock.total_shares
            })
            .map(Self::from_default_row)
            .unwrap_or_else(|| Self::from_total_shares(stock.total_shares))
    }

    /// 通用推导（游戏假设）：资本 = 面值 1 元 × 总股本；现金按 30%、借款按
    /// 10% 取整且至少 1 元，资金来源扣除现金后的实际投入形成固定资产。
    /// 整百股时固定资产为资本的 80%；零碎股不独立截断资产用途而损失资金。
    fn from_total_shares(total_shares: u64) -> Self {
        let capital = i128::from(total_shares).max(1);
        let cash = (capital * 30 / 100).max(1);
        let short_term_debt = (capital * 10 / 100).max(1);
        OpeningFigures {
            cash,
            receivables: 0,
            fixed_assets: capital + short_term_debt - cash,
            paid_in_capital: capital,
            short_term_debt,
        }
    }

    /// 从默认表行读取开局数字（元）；默认行使用通用科目表（AccountChart::version()=1）。此处
    /// 只取数字，工业账套（AccountChart::version()=2）由本模块统一重建。
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
        OpeningFigures {
            cash,
            receivables,
            fixed_assets,
            paid_in_capital,
            short_term_debt,
        }
    }

    /// 注册表域的通用科目表投影（AccountChart::version()=1）；保留条件债务行与应收插入顺序。
    fn registry_opening_lines(&self) -> Vec<OpeningLine> {
        let mut lines = vec![
            opening_line("1002", PostingSide::Debit, self.cash),
            opening_line("1601", PostingSide::Debit, self.fixed_assets),
            opening_line("4001", PostingSide::Credit, self.paid_in_capital),
        ];
        if self.receivables > 0 {
            lines.insert(
                1,
                opening_line("1122", PostingSide::Debit, self.receivables),
            );
        }
        if self.short_term_debt > 0 {
            lines.push(opening_line(
                "2001",
                PostingSide::Credit,
                self.short_term_debt,
            ));
        }
        lines
    }

    /// 经营域的工业科目表投影（AccountChart::version()=2）；债务行为无条件行，沿用开局账套守卫。
    fn industrial_opening_lines(&self) -> Vec<JournalLine> {
        let mut lines = vec![
            journal_line("1002", PostingSide::Debit, self.cash),
            journal_line("1601", PostingSide::Debit, self.fixed_assets),
            journal_line("2001", PostingSide::Credit, self.short_term_debt),
            journal_line("4001", PostingSide::Credit, self.paid_in_capital),
        ];
        if self.receivables > 0 {
            lines.insert(
                1,
                journal_line("1122", PostingSide::Debit, self.receivables),
            );
        }
        lines
    }
}

fn yuan(value: i128) -> AccountingAmount {
    AccountingAmount::from_cents(
        value
            .checked_mul(100)
            .expect("listed company fixture yuan fits i128"),
    )
}

/// 注册表域开局行（通用科目表，AccountChart::version()=1）。
fn opening_line(code: &str, side: PostingSide, value: i128) -> OpeningLine {
    OpeningLine {
        account: LedgerAccountId(code.to_string()),
        side,
        amount: yuan(value),
    }
}

/// 工业账套域开局行（AccountChart::version()=2）。
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
    figures: OpeningFigures,
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
    // 注册表配置使用通用科目表（AccountChart::version()=1）。
    let registry_lines = figures.registry_opening_lines();
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

    // 经营配置包含工业科目表（AccountChart::version()=2）、子账种子与经营流参数。
    // 开局行只包含金额为正的科目（过账守卫拒绝非正行金额；通用推导的应收为 0）。
    let debt_maturity = CivilDate::from_ymd(as_of.year() + 2, 12, 31)
        .map_err(|error| SessionError::InvalidSetup(format!("debt maturity invalid: {error}")))?;
    let opening_debt = (figures.short_term_debt > 0).then(|| OpeningDebtTerms {
        lender: lender_id.clone(),
        principal: yuan(figures.short_term_debt),
        annual_rate_bp: 365,
        maturity_date: debt_maturity,
    });
    let industrial_lines = figures.industrial_opening_lines();
    let industrial = IndustrialConfig {
        chart: industrial_account_chart(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn odd_share_company_opening_allocates_actual_funding_without_rounding_loss() {
        let as_of = CivilDate::from_ymd(2027, 12, 31).unwrap();
        let mut stock = crate::session::npc_working_quote_tests::quote_setup(0)
            .stocks
            .remove(0);
        stock.total_shares = 101;
        stock.float_shares = 101;
        let listed = assemble_listed_company(
            &stock,
            OpeningFigures::from_total_shares(stock.total_shares),
            as_of,
        )
        .expect("合法小股本公司的开局资金配置必须精确平账");
        let registry = CompanyRegistry::new(vec![listed.registry]).unwrap();
        let registry_books = registry
            .get(&CompanyId(format!("C-{}", stock.code.0)))
            .unwrap()
            .books();
        let operating_books = listed.operating.books.books();
        for books in [registry_books, operating_books] {
            let trial = books.ledger().trial_balance().unwrap();
            assert_eq!(trial.total_debits, yuan(111));
            assert_eq!(trial.total_credits, yuan(111));
            assert_eq!(books.ledger().cash_total().unwrap(), yuan(30));
            assert_eq!(books.ledger().liabilities_total().unwrap(), yuan(10));
            assert_eq!(books.ledger().equity_rolling().unwrap(), yuan(101));
            assert_eq!(books.journal().entry_count(), 1);
        }
    }

    #[test]
    fn opening_figures_preserve_both_account_orders_and_yuan_units() {
        for (receivables, debt) in [(0, 0), (7, 0), (0, 11), (7, 11)] {
            let figures = OpeningFigures {
                cash: 30,
                receivables,
                fixed_assets: 80,
                paid_in_capital: 100,
                short_term_debt: debt,
            };
            let registry = figures.registry_opening_lines();
            let industrial = figures.industrial_opening_lines();
            let mut expected_registry = vec![
                ("1002", PostingSide::Debit, 3_000),
                ("1601", PostingSide::Debit, 8_000),
                ("4001", PostingSide::Credit, 10_000),
            ];
            let mut expected_industrial = vec![
                ("1002", PostingSide::Debit, 3_000),
                ("1601", PostingSide::Debit, 8_000),
                ("2001", PostingSide::Credit, debt * 100),
                ("4001", PostingSide::Credit, 10_000),
            ];
            if receivables > 0 {
                expected_registry.insert(1, ("1122", PostingSide::Debit, receivables * 100));
                expected_industrial.insert(1, ("1122", PostingSide::Debit, receivables * 100));
            }
            if debt > 0 {
                expected_registry.push(("2001", PostingSide::Credit, debt * 100));
            }
            assert_eq!(
                registry
                    .iter()
                    .map(|line| (line.account.0.as_str(), line.side, line.amount.cents()))
                    .collect::<Vec<_>>(),
                expected_registry
            );
            assert_eq!(
                industrial
                    .iter()
                    .map(|line| (line.account.0.as_str(), line.side, line.amount.cents()))
                    .collect::<Vec<_>>(),
                expected_industrial
            );
        }
    }

    #[test]
    fn opening_figures_default_selection_requires_stock_code_and_share_count() {
        let defaults = default_companies(CivilDate::from_ymd(2023, 12, 31).unwrap()).unwrap();
        let row = defaults
            .iter()
            .find(|row| {
                row.spec
                    .listed_stock
                    .as_ref()
                    .is_some_and(|code| code.0 == "600101")
            })
            .unwrap();
        let mut stock = crate::session::npc_working_quote_tests::quote_setup(0)
            .stocks
            .remove(0);
        stock.code = row.spec.listed_stock.clone().unwrap();
        stock.total_shares = row.spec.issued_shares;
        let exact = OpeningFigures::for_stock(&stock, &defaults);
        assert_eq!(exact.cash, 6_000_000_000);
        assert_eq!(exact.paid_in_capital, i128::from(row.spec.issued_shares));
        stock.total_shares = 10_000_000;
        let different_shares = OpeningFigures::for_stock(&stock, &defaults);
        assert_eq!(different_shares.cash, 3_000_000);
        stock.code = StockCode("600888".to_string());
        stock.total_shares = row.spec.issued_shares;
        let different_code = OpeningFigures::for_stock(&stock, &defaults);
        assert_eq!(
            different_code.cash,
            i128::from(row.spec.issued_shares) * 30 / 100
        );
    }

    #[test]
    fn opening_figures_default_rows_preserve_explicit_amounts() {
        let as_of = CivilDate::from_ymd(2023, 12, 31).unwrap();
        let defaults = default_companies(as_of).unwrap();
        for row in defaults {
            let projected = OpeningFigures::from_default_row(&row).registry_opening_lines();
            assert_eq!(
                projected
                    .iter()
                    .map(|line| (&line.account, line.side, line.amount.cents()))
                    .collect::<Vec<_>>(),
                row.opening
                    .lines
                    .iter()
                    .map(|line| (&line.account, line.side, line.amount.cents()))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn small_and_maximum_share_company_openings_preserve_registered_funding() {
        let as_of = CivilDate::from_ymd(2027, 12, 31).unwrap();
        for total_shares in [1, 2, 9, 10, 99, 100, 109, u64::MAX] {
            let mut stock = crate::session::npc_working_quote_tests::quote_setup(0)
                .stocks
                .remove(0);
            stock.total_shares = total_shares;
            stock.float_shares = 0;
            let figures = OpeningFigures::from_total_shares(total_shares);
            let capital = i128::from(total_shares);
            let cash = (capital * 30 / 100).max(1);
            let debt = (capital * 10 / 100).max(1);
            assert_eq!(figures.paid_in_capital, capital);
            assert_eq!(figures.cash, cash);
            assert_eq!(figures.short_term_debt, debt);
            assert_eq!(figures.fixed_assets, capital + debt - cash);
            let listed = assemble_listed_company(&stock, figures, as_of).unwrap();
            let registry = CompanyRegistry::new(vec![listed.registry]).unwrap();
            let company = registry
                .get(&CompanyId(format!("C-{}", stock.code.0)))
                .unwrap();
            for books in [company.books(), listed.operating.books.books()] {
                let trial = books.ledger().trial_balance().unwrap();
                assert_eq!(trial.total_debits, yuan(capital + debt));
                assert_eq!(trial.total_credits, yuan(capital + debt));
                assert_eq!(books.ledger().cash_total().unwrap(), yuan(cash));
                assert_eq!(books.ledger().liabilities_total().unwrap(), yuan(debt));
                assert_eq!(books.ledger().equity_rolling().unwrap(), yuan(capital));
            }
        }
    }

    #[test]
    fn zero_share_company_still_rejects_invalid_issuer_specification() {
        let as_of = CivilDate::from_ymd(2027, 12, 31).unwrap();
        let mut stock = crate::session::npc_working_quote_tests::quote_setup(0)
            .stocks
            .remove(0);
        stock.total_shares = 0;
        stock.float_shares = 0;
        let result = assemble_listed_company(&stock, OpeningFigures::from_total_shares(0), as_of);
        assert!(matches!(result, Err(SessionError::InvalidSetup(message))
            if message.starts_with("company spec failed:") && message.contains("zero issued shares")));
    }
}

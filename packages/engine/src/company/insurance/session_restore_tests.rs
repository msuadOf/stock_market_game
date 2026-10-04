use super::*;
use crate::account::StockCode;
use crate::calendar::CivilDate;
use crate::company::events::ShockParams;
use crate::company::operations::{
    CompanyOperationsConfig, FlowParams, IndustryBooks, InsuranceFlowParams, OperatingCompanyConfig,
};
use crate::company::{CompanyId, CompanyKind, CompanySpec, CounterpartyKind, IndustryId};
use crate::money::Money;
use crate::session::{
    FloatAllocation, GameSession, NpcSetup, SecurityCategory, SessionError, SessionSetup,
    StockExchange, StockSpec,
};
use crate::strategy::{HotParams, InstParams, RetailParams, StrategyParams};

#[test]
fn full_session_restore_rejects_corrupt_insurance_group_in_memory() {
    let amount = AccountingAmount::from_cents;
    let date = |value| CivilDate::from_iso(value).unwrap();
    let company_id = CompanyId("RESTORE-INS".into());
    let stock = StockCode("600101".into());
    let policyholder = CounterpartyId("POL".into());
    let mut shocks = ShockParams::current_default_parameters();
    shocks.market_candidate_bp = 0;
    shocks.industry_candidate_bp = 0;
    shocks.company_candidate_bp = 0;
    let books = InsuranceBooks::new(InsuranceConfig {
        chart: insurance_account_chart(),
        as_of: date("2027-12-31"),
        opening_lines: vec![
            line(
                chart::acct::CASH,
                crate::accounting::PostingSide::Debit,
                amount(200_000),
            ),
            line(
                chart::acct::CAPITAL,
                crate::accounting::PostingSide::Credit,
                amount(200_000),
            ),
        ],
        counterparties: vec![crate::company::ExternalCounterparty {
            id: policyholder.clone(),
            kind: CounterpartyKind::Customer,
            name: "测试投保人".into(),
        }],
        discount: DiscountAssumption {
            version: 1,
            rate_bp: 400,
        },
    })
    .unwrap();
    let mut session = GameSession::new(
        SessionSetup {
            stocks: vec![StockSpec {
                code: stock.clone(),
                exchange: StockExchange::Shanghai,
                initial_price: Money::from_cents(1_000),
                category: SecurityCategory::MainBoard,
                limit_pct: 0.10,
                tick: Money::from_cents(1),
                total_shares: 100,
                float_shares: 0,
            }],
            company_operations: Some(CompanyOperationsConfig {
                seed: 1,
                shock_params: shocks,
                companies: vec![OperatingCompanyConfig {
                    spec: CompanySpec {
                        id: company_id.clone(),
                        name: "测试保险公司".into(),
                        industry: IndustryId("insurance".into()),
                        kind: CompanyKind::Insurance,
                        listed_stock: Some(stock),
                        issued_shares: 100,
                        group_parent: None,
                    },
                    books: IndustryBooks::Insurance(books),
                    flow: FlowParams::Insurance(InsuranceFlowParams {
                        policyholder: policyholder.clone(),
                        daily_groups_base: 0,
                        premium: amount(100_000),
                        expected_claims: amount(80_000),
                        risk_adjustment: amount(5_000),
                        coverage_days: 30,
                        claim_every_days: 10,
                        claim_size: amount(1_000),
                    }),
                }],
            }),
            groups: Vec::new(),
            npcs: NpcSetup {
                retail_count: 0,
                inst_count: 0,
                hot_count: 0,
                retail_cash_median: Money::from_cents(10_000_000),
            },
            config: crate::GameConfig::proposed_defaults(),
            strategy_params: StrategyParams {
                retail: RetailParams {
                    arrival_rate: 0.0,
                    order_size_mean: 100,
                    chase_prob: 0.0,
                },
                inst: InstParams {
                    margin: 0.05,
                    order_size: 100,
                },
                hot: HotParams {
                    lookback: 3,
                    trend_threshold: 0.02,
                    order_size: 100,
                },
            },
            ticks_per_day: 6,
            auction_ticks: 0,
            closing_auction_ticks: 0,
            history_len: 5,
            t1_enabled: true,
            float_allocation: FloatAllocation::random(),
            start_date: date("2030-01-05"),
            simulation_policy_id: crate::SIMULATION_POLICY_ID.into(),
        },
        1,
    )
    .unwrap();
    session.end_civil_day().unwrap();
    let mut saved = session.save().unwrap();
    let group_id = ContractId("RESTORE-GROUP".into());
    let company = saved
        .company_operations
        .companies
        .get_mut(&company_id)
        .unwrap();
    let IndustryBooks::Insurance(books) = &mut company.books else {
        panic!("测试公司必须拥有 InsuranceBooks");
    };
    books
        .establish_group(
            InsuranceProductKind::TermProtection,
            group_id.clone(),
            &policyholder,
            amount(100_000),
            amount(80_000),
            amount(5_000),
            date("2030-01-06"),
            date("2030-02-05"),
        )
        .unwrap();
    let restored = GameSession::restore(&saved).unwrap();
    assert_eq!(
        restored.save().unwrap().company_operations,
        saved.company_operations
    );
    let company = saved
        .company_operations
        .companies
        .get_mut(&company_id)
        .unwrap();
    let IndustryBooks::Insurance(books) = &mut company.books else {
        panic!("测试公司必须拥有 InsuranceBooks");
    };
    groups::restore_tests::set_released_revenue(
        books.groups.get_mut(&group_id).unwrap(),
        AccountingAmount::MAX,
    );
    let error = match GameSession::restore(&saved) {
        Err(SessionError::InvalidSave(detail)) => detail,
        _ => panic!("损坏保险子账必须被完整 restore 拒绝"),
    };
    assert!(error.contains("RESTORE-INS"), "{error}");
    assert!(error.contains("RESTORE-GROUP"), "{error}");
    assert!(error.contains("GMM"), "{error}");
}

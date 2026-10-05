use super::*;
use crate::calendar::CivilDate;
use crate::company::events::ShockParams;
use crate::company::operations::{
    CompanyOperations, CompanyOperationsConfig, FlowParams, IndustryBooks, InsuranceFlowParams,
    OperatingCompanyConfig,
};
use crate::company::{CompanyId, CompanyKind, CompanySpec, CounterpartyKind, IndustryId};

#[test]
fn independent_operations_restore_rejects_corrupt_insurance_group_with_company_context() {
    let amount = AccountingAmount::from_cents;
    let date = |value| CivilDate::from_iso(value).unwrap();
    let company_id = CompanyId("RESTORE-INS".into());
    let policyholder = CounterpartyId("POL".into());
    let mut shocks = ShockParams::current_default_parameters();
    shocks.market_candidate_bp = 0;
    shocks.industry_candidate_bp = 0;
    shocks.company_candidate_bp = 0;
    let books = InsuranceBooks::new(InsuranceConfig {
        chart: insurance_account_chart(),
        as_of: date("2030-01-05"),
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
        income_tax_policy: crate::accounting::IncomeTaxPolicy {
            rate_bp: 2_500,
            loss_carryforward_years: 5,
        },
    })
    .unwrap();
    let mut saved = CompanyOperations::new(
        CompanyOperationsConfig {
            seed: 1,
            shock_params: shocks,
            companies: vec![OperatingCompanyConfig {
                spec: CompanySpec {
                    id: company_id.clone(),
                    name: "测试保险公司".into(),
                    industry: IndustryId("insurance".into()),
                    kind: CompanyKind::Insurance,
                    listed_stock: None,
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
        },
        date("2030-01-06"),
    )
    .unwrap();
    saved.advance_civil_day(date("2030-01-06")).unwrap();
    let group_id = ContractId("RESTORE-GROUP".into());
    let company = saved.companies.get_mut(&company_id).unwrap();
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
            date("2030-01-07"),
            date("2030-02-06"),
        )
        .unwrap();
    let restored: CompanyOperations =
        serde_json::from_value(serde_json::to_value(&saved).unwrap()).unwrap();
    restored.validate_restored().unwrap();
    assert_eq!(restored, saved);
    let before = serde_json::to_value(&restored).unwrap();
    let company = saved.companies.get_mut(&company_id).unwrap();
    let IndustryBooks::Insurance(books) = &mut company.books else {
        panic!("测试公司必须拥有 InsuranceBooks");
    };
    groups::restore_tests::set_released_revenue(
        books.groups.get_mut(&group_id).unwrap(),
        AccountingAmount::MAX,
    );
    let error = saved
        .validate_restored()
        .expect_err("损坏保险子账必须被独立经营状态恢复拒绝")
        .to_string();
    assert!(error.contains("RESTORE-INS"), "{error}");
    assert!(error.contains("RESTORE-GROUP"), "{error}");
    assert!(error.contains("GMM"), "{error}");
    assert_eq!(serde_json::to_value(restored).unwrap(), before);
}

use super::period::{
    AnnualTrendConfig, PeriodGenerationParameters, PeriodNoiseConfig, PeriodVariableExpenseRule,
    SettlementCycle,
};
use super::*;
use crate::{
    account::StockCode,
    accounting::{
        AccountingAmount, IncomeTaxPolicy, JournalLine, LedgerAccountId, PostingSide, TaxPolicy,
        VatPolicy,
    },
    calendar::CivilDate,
    company::{
        CompanyId, CompanyKind, CompanySpec, CompanySystem, CompanySystemError, IndustryId,
        api::CompanyCommand, config::CompanySystemConfig,
    },
};

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).unwrap()
}
fn amount(cents: i128) -> AccountingAmount {
    AccountingAmount::from_cents(cents)
}
fn spec() -> CompanySpec {
    CompanySpec {
        id: CompanyId("A".into()),
        name: "虚拟公司".into(),
        industry: IndustryId("虚拟行业".into()),
        kind: CompanyKind::Industrial,
        listed_stock: Some(StockCode("600101".into())),
        issued_shares: 1000,
        group_parent: None,
    }
}
fn config() -> SimpleConfig {
    SimpleConfig {
        environment: SimpleEnvironmentConfig {
            initial_change_bp: 0,
            persistence_bp: 0,
            noise: zero_noise(),
        },
        companies: vec![SimpleCompanyConfig {
            company: spec().id,
            kind: CompanyKind::Industrial,
            generation: PeriodGenerationParameters {
                initial_revenue: amount(100_000),
                initial_fixed_expense: amount(110_000),
                revenue_trend: AnnualTrendConfig::Fixed {
                    annual_growth_bp: 1268,
                },
                demand_sensitivity_bp: 0,
                revenue_noise: zero_noise(),
                fixed_expense_trend: AnnualTrendConfig::Fixed {
                    annual_growth_bp: 0,
                },
                fixed_expense_noise: zero_noise(),
                variable_expense: PeriodVariableExpenseRule::RevenueRatio {
                    ratio_bp: 0,
                    noise: zero_noise(),
                },
            },
            finance: SimpleFinanceConfig {
                opening_lines: vec![
                    JournalLine {
                        account: LedgerAccountId("1122".into()),
                        side: PostingSide::Debit,
                        amount: amount(500_000),
                    },
                    JournalLine {
                        account: LedgerAccountId("4001".into()),
                        side: PostingSide::Credit,
                        amount: amount(500_000),
                    },
                ],
                tax_policy: TaxPolicy {
                    version: 1,
                    vat: VatPolicy {
                        output_rate_bp: 1300,
                        input_rate_bp: 1300,
                        deductible_share_bp: 10_000,
                    },
                    income_tax: IncomeTaxPolicy {
                        rate_bp: 2500,
                        loss_carryforward_years: 5,
                    },
                },
                summary_rule: SimpleSummaryRule::ReceivableRevenuePayableExpenses,
            },
            preferences: super::preferences::SimpleCompanyPreferences::none(),
        }],
        prehistory_periods: 2,
        settlement_cycle: SettlementCycle::Monthly,
    }
}
fn zero_noise() -> PeriodNoiseConfig {
    PeriodNoiseConfig {
        monthly_bp: 0,
        quarterly_bp: 0,
        half_year_bp: 0,
        annual_bp: 0,
    }
}

#[test]
fn restored_finance_baseline_must_match_fundamentals_history_start() {
    let mut cfg = config();
    cfg.prehistory_periods = 0;
    let state = create(cfg.clone());
    let mut earlier = SimpleFinanceState::create(
        spec().id,
        CompanyKind::Industrial,
        &cfg.companies[0].finance,
        date("2029-11-30"),
    )
    .unwrap();
    earlier
        .apply_period(
            date("2029-12-01"),
            date("2029-12-31"),
            &crate::company::api::PeriodAmounts {
                revenue: amount(0),
                fixed_expense: amount(0),
                variable_expense: amount(0),
            },
        )
        .unwrap();
    earlier.validate().unwrap();
    let mut saved = serde_json::to_value(state).unwrap();
    saved["implementation"]["state"]["companies"]["A"]["finance"] =
        serde_json::to_value(earlier).unwrap();
    assert!(serde_json::from_value::<CompanySystem>(saved).is_err());
}
fn create(config: SimpleConfig) -> CompanySystem {
    CompanySystem::create(
        vec![spec()],
        CompanySystemConfig::Simple(config),
        date("2030-01-01"),
        19,
    )
    .unwrap()
}

#[test]
fn restored_issuer_kind_must_match_finance_kind() {
    let state = create(config());
    let mut saved = serde_json::to_value(state).unwrap();
    saved["issuers"]["A"]["kind"] = serde_json::json!("Bank");
    assert!(serde_json::from_value::<CompanySystem>(saved).is_err());
}

#[test]
fn explicit_config_kind_is_required_and_must_match_issuer() {
    let mut saved = serde_json::to_value(config()).unwrap();
    saved["companies"][0]
        .as_object_mut()
        .unwrap()
        .remove("kind");
    assert!(serde_json::from_value::<SimpleConfig>(saved).is_err());
    let mut cfg = config();
    cfg.companies[0].kind = CompanyKind::Bank;
    assert!(
        CompanySystem::create(
            vec![spec()],
            CompanySystemConfig::Simple(cfg),
            date("2030-01-01"),
            19,
        )
        .is_err()
    );
}

#[test]
fn simple_creates_dated_summary_prehistory_with_expense_derived_loss() {
    let state = create(config());
    assert_eq!(state.advanced_through(), date("2029-12-31"));
    assert_eq!(state.history().len(), 2);
    assert_eq!(state.history()[0].period_start, date("2029-11-01"));
    assert_eq!(state.history()[1].period_end, date("2029-12-31"));
    assert_eq!(state.history()[1].amounts.revenue, amount(102_010));
    assert_eq!(state.history()[1].amounts.fixed_expense, amount(110_000));
    let finance = state.finance(&spec().id).unwrap();
    let loss_before_tax = amount(-16_990);
    let deferred_tax_asset = amount(4_248);
    assert_eq!(
        finance
            .books()
            .ledger()
            .account_balance(&LedgerAccountId("1811".into()))
            .net_debit()
            .unwrap(),
        deferred_tax_asset
    );
    assert_eq!(
        finance.books().ledger().equity_rolling().unwrap(),
        amount(500_000)
            .add(loss_before_tax)
            .unwrap()
            .add(deferred_tax_asset)
            .unwrap()
    );
    let caps = state.capabilities(&spec().id).unwrap();
    assert!(caps.full_financial_statements && caps.cash_flow);
    assert!(!caps.cash_settlement);
}

#[test]
fn simple_month_end_generates_facts_without_owning_publication_schedule() {
    let mut state = create(config());
    for day in 1..31 {
        assert!(
            state
                .advance_day(CivilDate::from_ymd(2030, 1, day).unwrap())
                .unwrap()
                .is_empty()
        );
    }
    let generated = state.advance_day(date("2030-01-31")).unwrap();
    assert_eq!(generated.len(), 1);
    assert_eq!(generated[0].period_start, date("2030-01-01"));
    assert_eq!(generated[0].period_end, date("2030-01-31"));
    let before = state.export_state();
    assert!(state.advance_day(date("2030-01-31")).is_err());
    assert_eq!(state, before);
    assert!(state.advance_day(date("2030-02-02")).is_err());
    assert_eq!(state, before);
}

#[test]
fn simple_zero_revenue_needs_explicit_restart_without_investor_funding() {
    let mut cfg = config();
    cfg.prehistory_periods = 0;
    cfg.companies[0].generation.initial_revenue = amount(0);
    let mut state = create(cfg);
    for day in 1..=31 {
        state
            .advance_day(CivilDate::from_ymd(2030, 1, day).unwrap())
            .unwrap();
    }
    assert_eq!(state.history()[0].amounts.revenue, amount(0));
    assert!(
        state
            .submit_command(CompanyCommand::RestartRevenue {
                company: spec().id,
                revenue: amount(1000),
                source: "".into()
            })
            .is_err()
    );
    state
        .submit_command(CompanyCommand::RestartRevenue {
            company: spec().id,
            revenue: amount(1000),
            source: "虚拟复业场景输入".into(),
        })
        .unwrap();
    for day in 1..=28 {
        state
            .advance_day(CivilDate::from_ymd(2030, 2, day).unwrap())
            .unwrap();
    }
    assert_eq!(state.history()[1].amounts.revenue, amount(1000));
}

#[test]
fn simple_restore_keeps_rng_and_next_month_result_and_rejects_missing_fields() {
    let mut cfg = config();
    cfg.environment.noise.monthly_bp = 50;
    cfg.companies[0].generation.revenue_noise.monthly_bp = 25;
    cfg.companies[0].generation.fixed_expense_noise.monthly_bp = 10;
    let mut state = create(cfg);
    let value = serde_json::to_value(&state).unwrap();
    let mut restored: CompanySystem = serde_json::from_value(value.clone()).unwrap();
    for day in 1..=31 {
        let next = CivilDate::from_ymd(2030, 1, day).unwrap();
        assert_eq!(
            state.advance_day(next).unwrap(),
            restored.advance_day(next).unwrap()
        );
    }
    assert_eq!(state, restored);
    let mut missing = value;
    missing["implementation"]["state"]["config"]["companies"][0]["generation"]
        .as_object_mut()
        .unwrap()
        .remove("initial_fixed_expense");
    assert!(serde_json::from_value::<CompanySystem>(missing).is_err());
}

#[test]
fn simple_invalid_growth_and_simulation_are_not_fallbacks() {
    let mut cfg = config();
    cfg.companies[0].generation.revenue_trend = AnnualTrendConfig::Fixed {
        annual_growth_bp: -10_001,
    };
    assert!(
        CompanySystem::create(
            vec![spec()],
            CompanySystemConfig::Simple(cfg),
            date("2030-01-01"),
            1
        )
        .is_err()
    );
    assert!(matches!(
        CompanySystem::create(
            vec![spec()],
            CompanySystemConfig::Simulation,
            date("2030-01-01"),
            1
        ),
        Err(CompanySystemError::Unsupported(_))
    ));
}

#[test]
fn simple_failed_month_end_keeps_all_rng_and_indicators_unchanged() {
    let mut cfg = config();
    cfg.prehistory_periods = 0;
    cfg.companies[0].generation.initial_revenue = AccountingAmount::MAX;
    let mut state = create(cfg);
    for day in 1..31 {
        state
            .advance_day(CivilDate::from_ymd(2030, 1, day).unwrap())
            .unwrap();
    }
    let before = state.export_state();
    assert!(state.advance_day(date("2030-01-31")).is_err());
    assert_eq!(state, before);
}

#[test]
fn simple_restore_rejects_future_duplicate_history_wrong_owner_and_half_restart() {
    let original = serde_json::to_value(create(config())).unwrap();
    for mutation in 0..6 {
        let mut value = original.clone();
        let saved = &mut value["implementation"]["state"];
        match mutation {
            0 => saved["history"][0]["company"] = serde_json::json!("UNKNOWN"),
            1 => saved["history"][0]["period_end"] = serde_json::json!("2030-11-30"),
            2 => {
                let entry = saved["history"][0].clone();
                saved["history"].as_array_mut().unwrap().push(entry);
            }
            3 => {
                saved["companies"]["A"]["generation"]["amounts"]["revenue"] =
                    serde_json::json!("5.00")
            }
            4 => {
                saved["history"][0]["explanation"]["restart_source"] =
                    serde_json::json!("没有对应复业收入")
            }
            5 => {
                saved["companies"]["A"]
                    .as_object_mut()
                    .unwrap()
                    .remove("pending_restart");
            }
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<CompanySystem>(value).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn simple_hash_cache_is_not_business_state_and_invalidates_on_change() {
    let mut state = create(config());
    let uncached = state.clone();
    let first = state.hash_projection().unwrap();
    assert_eq!(state, uncached);
    assert_eq!(first, state.clone().hash_projection().unwrap());
    let restored: CompanySystem =
        serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();
    assert_eq!(first, restored.hash_projection().unwrap());
    state.advance_day(date("2030-01-01")).unwrap();
    assert_ne!(first, state.hash_projection().unwrap());
    let updated = state.hash_projection().unwrap();
    assert!(state.advance_day(date("2030-01-01")).is_err());
    assert_eq!(updated, state.hash_projection().unwrap());
}

#[test]
fn simple_one_failed_company_does_not_commit_other_parallel_company() {
    let mut first = spec();
    let mut second = spec();
    first.id = CompanyId("FIRST".into());
    second.id = CompanyId("SECOND".into());
    second.listed_stock = Some(StockCode("600102".into()));
    let mut cfg = config();
    cfg.prehistory_periods = 0;
    cfg.companies[0].company = first.id.clone();
    cfg.companies[0].generation.revenue_noise.monthly_bp = 50;
    let mut failing = cfg.companies[0].clone();
    failing.company = second.id.clone();
    failing.generation.initial_revenue = AccountingAmount::MAX;
    failing.generation.revenue_noise.monthly_bp = 0;
    cfg.companies.push(failing);
    let mut state = CompanySystem::create(
        vec![first, second],
        CompanySystemConfig::Simple(cfg),
        date("2030-01-01"),
        19,
    )
    .unwrap();
    for day in 1..31 {
        state
            .advance_day(CivilDate::from_ymd(2030, 1, day).unwrap())
            .unwrap();
    }
    let before = state.clone();
    assert!(state.advance_day(date("2030-01-31")).is_err());
    assert_eq!(state, before);
}

#[test]
fn simple_restore_rejects_unknown_nested_issuer_and_rng_fields() {
    let mut cfg = config();
    cfg.prehistory_periods = 0;
    let original = serde_json::to_value(create(cfg)).unwrap();
    for path in ["issuer", "environment", "company"] {
        let mut value = original.clone();
        match path {
            "issuer" => value["issuers"]["A"]["unsupported_book"] = serde_json::json!({}),
            "environment" => {
                value["implementation"]["state"]["environment_rng"]["unknown"] =
                    serde_json::json!(true)
            }
            "company" => {
                value["implementation"]["state"]["companies"]["A"]["generation"]["rng"]["unknown"] =
                    serde_json::json!(true)
            }
            _ => unreachable!(),
        }
        assert!(
            serde_json::from_value::<CompanySystem>(value).is_err(),
            "nested {path}"
        );
    }
}

#[test]
fn simple_restore_rejects_duplicate_map_identities() {
    let mut cfg = config();
    cfg.prehistory_periods = 0;
    let value = serde_json::to_value(create(cfg)).unwrap();
    for (path, object) in [
        ("issuers", &value["issuers"]),
        ("companies", &value["implementation"]["state"]["companies"]),
    ] {
        let entry = serde_json::to_string(&object["A"]).unwrap();
        let unique = format!("\"{path}\":{{\"A\":{entry}}}");
        let duplicate = format!("\"{path}\":{{\"A\":{entry},\"A\":{entry}}}");
        let serialized = serde_json::to_string(&value).unwrap();
        assert!(serialized.contains(&unique));
        assert!(
            serde_json::from_str::<CompanySystem>(&serialized.replace(&unique, &duplicate))
                .is_err(),
            "duplicate {path}"
        );
    }
}

#[test]
fn simple_restore_without_history_rejects_initial_baseline_drift() {
    let mut cfg = config();
    cfg.prehistory_periods = 0;
    let mut value = serde_json::to_value(create(cfg)).unwrap();
    value["implementation"]["state"]["companies"]["A"]["generation"]["amounts"]["revenue"] =
        serde_json::json!("999.00");
    assert!(serde_json::from_value::<CompanySystem>(value).is_err());
}

#[test]
fn restore_rejects_explanation_cycle_mismatching_actual_natural_window() {
    let mut cfg = config();
    cfg.companies[0].generation.revenue_trend = AnnualTrendConfig::Fixed {
        annual_growth_bp: 0,
    };
    let mut value = serde_json::to_value(create(cfg)).unwrap();
    let history = value["implementation"]["state"]["history"]
        .as_array_mut()
        .unwrap();
    for entry in history {
        entry["explanation"]["cycle"] = serde_json::json!("Annual");
        entry["explanation"]["revenue_segments"][0]["months"] = serde_json::json!(12);
        entry["explanation"]["fixed_expense_segments"][0]["months"] = serde_json::json!(12);
    }
    assert!(serde_json::from_value::<CompanySystem>(value).is_err());
}

#[test]
fn quarterly_start_inside_natural_cycle_waits_until_its_real_end() {
    let mut cfg = config();
    cfg.settlement_cycle = SettlementCycle::Quarterly;
    cfg.prehistory_periods = 0;
    let mut state = CompanySystem::create(
        vec![spec()],
        CompanySystemConfig::Simple(cfg),
        date("2030-01-07"),
        19,
    )
    .unwrap();
    assert_eq!(state.advanced_through(), date("2030-01-06"));
    assert_eq!(
        state.finance(&spec().id).unwrap().opening_date(),
        date("2029-12-31")
    );
    let mut cursor = date("2030-01-07");
    while cursor < date("2030-03-31") {
        assert!(state.advance_day(cursor).unwrap().is_empty());
        cursor = cursor.next().unwrap();
    }
    assert!(state.history().is_empty());
    let generated = state.advance_day(cursor).unwrap();
    assert_eq!(generated[0].period_start, date("2030-01-01"));
    assert_eq!(generated[0].period_end, date("2030-03-31"));
    assert_eq!(generated[0].explanation.cycle, SettlementCycle::Quarterly);
    assert_eq!(
        state
            .report_availability(
                &spec().id,
                crate::accounting::AccountingPeriod::from_ymd(2030, 3).unwrap(),
                crate::accounting::reports::ReportKind::Monthly
            )
            .unwrap(),
        crate::company::api::CompanyReportAvailability::PeriodNotRepresented
    );
}

#[test]
fn restore_requires_explicit_nullable_issuer_fields_without_default_completion() {
    let mut cfg = config();
    cfg.prehistory_periods = 0;
    let original = serde_json::to_value(create(cfg)).unwrap();
    for field in ["listed_stock", "group_parent"] {
        let mut value = original.clone();
        value["issuers"]["A"].as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<CompanySystem>(value).is_err(),
            "missing {field}"
        );
    }
}

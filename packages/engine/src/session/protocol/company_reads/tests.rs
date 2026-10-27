use super::*;
use crate::company::cash_dividend_tax::{
    CashDividendTaxBook, CashDividendTaxMode, DividendTaxProfile,
};
use crate::session::npc_working_quote_tests::quote_setup;
include!("../../../../test-support/simple_company.rs");

fn emit_wire_fixture(result: &CompanyReadResult) {
    println!(
        "COMPANY_READ_FIXTURE {}",
        serde_json::json!({ "owner": AccountId(0), "value": result })
    );
}

fn company_session(mode: CashDividendTaxMode) -> ProtocolSession {
    let mut setup = quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.history_len = 1;
    setup.company_system = Some(simple_company_fixture!(crate; ["600888"]));
    setup.dividend_tax_mode = Some(mode);
    setup.flat_withholding_bp = (mode == CashDividendTaxMode::FlatWithholding).then_some(1000);
    setup.par_value_per_share = Some(crate::Money::from_cents(100));
    setup.auto_corporate_foundation = true;
    setup.report_frequency = Some(crate::information::ReportFrequency::Quarterly);
    ProtocolSession::new(setup, 7).unwrap()
}

#[test]
fn company_views_preserve_nonempty_existing_domain_results_and_save_bytes() {
    let session = company_session(CashDividendTaxMode::AShareIndividual);
    assert!(session.runtime_driver_for_test().is_none());
    let before = serde_json::to_value(session.game().save().unwrap()).unwrap();
    for (query, expected) in [
        (
            CompanyReadQuery::AccountDividendTaxStatus,
            serde_json::to_value(
                session
                    .game()
                    .account_dividend_tax_status(AccountId(0))
                    .unwrap(),
            )
            .unwrap(),
        ),
        (
            CompanyReadQuery::CompanyCapabilities {
                company: CompanyId(0),
            },
            serde_json::to_value(
                session
                    .game()
                    .company_capabilities(&CompanyId(0), AccountId(0))
                    .unwrap(),
            )
            .unwrap(),
        ),
        (
            CompanyReadQuery::CompanyPeriodExplanation {
                company: CompanyId(0),
                period_end: CivilDate::from_iso("2029-12-31").unwrap(),
            },
            serde_json::to_value(
                session
                    .game()
                    .company_period_explanation(
                        &CompanyId(0),
                        CivilDate::from_iso("2029-12-31").unwrap(),
                    )
                    .unwrap(),
            )
            .unwrap(),
        ),
    ] {
        let result = session.company_read_query(AccountId(0), query).unwrap();
        emit_wire_fixture(&result);
        let wire = serde_json::to_value(result).unwrap();
        assert_eq!(wire["data"], expected);
    }
    assert_eq!(
        before,
        serde_json::to_value(session.game().save().unwrap()).unwrap()
    );
    assert!(session.runtime_driver_for_test().is_none());
}

#[test]
fn global_outstanding_is_filtered_to_the_authenticated_owner() {
    let mut session = company_session(CashDividendTaxMode::AShareIndividual);
    // 隔离验证读面过滤，不以装配的领域事实冒充日终结算 producer。
    let today = session.civil_date();
    let actions = session
        .game_mut()
        .state
        .companies
        .corporate_actions_mut()
        .unwrap();
    for owner in [AccountId(0), AccountId(99)] {
        actions.dividend_tax_books.push(
            CashDividendTaxBook::new(
                owner,
                crate::StockCode("600888".into()),
                DividendTaxProfile::IndividualPublicMarket,
                today,
                Vec::new(),
            )
            .unwrap(),
        );
    }
    let result = session
        .company_read_query(AccountId(0), CompanyReadQuery::DividendTaxOutstandingViews)
        .unwrap();
    emit_wire_fixture(&result);
    let CompanyReadResult::DividendTaxOutstandingViews(views) = result else {
        panic!("结果类型错误")
    };
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].account, AccountId(0));
}

#[test]
fn unhealthy_sessions_reject_before_reading_company_facts() {
    let mut session = company_session(CashDividendTaxMode::AShareIndividual);
    session.game_mut().poison = Some(StepFatal::InvariantViolation {
        location: "只读边界测试".into(),
        description: "测试已毒化会话".into(),
    });
    assert!(matches!(
        session.company_read_query(AccountId(0), CompanyReadQuery::AccountDividendTaxStatus),
        Err(CompanyReadError::Unhealthy(_))
    ));
}

#[test]
fn actual_owner_query_returns_the_existing_domain_view_without_state_changes() {
    let mut setup = quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.history_len = 1;
    let session = ProtocolSession::new(setup, 7).unwrap();
    let before = serde_json::to_value(session.game().save().unwrap()).unwrap();
    // 无公司线也是明确的领域拒绝，而不是空视图或宿主 Unsupported stub。
    let error = session
        .company_read_query(AccountId(0), CompanyReadQuery::AccountDividendTaxStatus)
        .unwrap_err();
    assert!(matches!(error, CompanyReadError::Corporate(_)), "{error:?}");
    let failure = serde_json::to_value(error.public_failure()).unwrap();
    assert!(failure["data"]["reason"]
        .as_str()
        .unwrap()
        .contains("未配置公司行为状态"));
    assert_eq!(
        before,
        serde_json::to_value(session.game().save().unwrap()).unwrap()
    );
}

#[test]
fn unknown_owner_is_rejected_before_any_domain_query() {
    let session = ProtocolSession::new(quote_setup(0), 7).unwrap();
    assert!(matches!(
        session.company_read_query(AccountId(u64::MAX), CompanyReadQuery::OwnerRightsOfferings),
        Err(CompanyReadError::UnknownOwner(AccountId(u64::MAX)))
    ));
}

#[test]
fn company_identity_uses_the_actual_catalog_namespace() {
    let session = ProtocolSession::new(quote_setup(0), 7).unwrap();
    assert!(matches!(
        session.company_read_query(
            AccountId(0),
            CompanyReadQuery::CompanyCapabilities {
                company: CompanyId(u32::MAX)
            }
        ),
        Err(CompanyReadError::UnknownCompany(CompanyId(u32::MAX)))
    ));
}

#[test]
fn requests_have_no_client_selected_owner_and_reject_unknown_fields() {
    for input in [
        r#"{"kind":"AccountDividendTaxStatus","account":1}"#,
        r#"{"kind":"CompanyCapabilities","data":{"company":0,"account":1}}"#,
    ] {
        assert!(serde_json::from_str::<CompanyReadQuery>(input).is_err());
    }
}

#[test]
fn flat_receipts_return_nonempty_owner_facts_without_other_accounts() {
    let mut session = company_session(CashDividendTaxMode::FlatWithholding);
    let other = session
        .join_market(
            crate::session::OpaqueSubjectId::new("flat-reader-other".into()).unwrap(),
            true,
        )
        .unwrap()
        .account_id;
    let paid_on = CivilDate::from_iso("2029-12-31").unwrap();
    // 只读隔离 fixture：直接装配冻结回执，不冒充 Session 付款 producer。
    let actions = session
        .game_mut()
        .state
        .companies
        .corporate_actions_mut()
        .unwrap();
    for account in [AccountId(0), AccountId(99), other] {
        actions
            .flat_withholding_receipts
            .push(FlatWithholdingReceipt {
                payment_id: "read-payment".into(),
                plan_id: "read-plan".into(),
                account,
                paid_on,
                gross: crate::Money::from_cents(if account == other { 200 } else { 100 }),
                rate_bp: 1000,
                withheld: crate::Money::from_cents(if account == other { 20 } else { 10 }),
            });
    }
    let before = serde_json::to_value(session.game().save().unwrap()).unwrap();
    let result = session
        .company_read_query(AccountId(0), CompanyReadQuery::OwnerFlatWithholdingReceipts)
        .unwrap();
    emit_wire_fixture(&result);
    let CompanyReadResult::OwnerFlatWithholdingReceipts(receipts) = result else {
        panic!("结果类型错误")
    };
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].account, AccountId(0));
    assert_eq!(
        receipts,
        session
            .game()
            .owner_flat_withholding_receipts(AccountId(0))
            .unwrap()
    );
    let result = session
        .company_read_query(other, CompanyReadQuery::OwnerFlatWithholdingReceipts)
        .unwrap();
    let CompanyReadResult::OwnerFlatWithholdingReceipts(other_receipts) = result else {
        panic!("结果类型错误")
    };
    assert_eq!(other_receipts.len(), 1);
    assert_eq!(other_receipts[0].account, other);
    assert_eq!(other_receipts[0].gross.cents(), 200);
    assert_ne!(other_receipts, receipts);
    assert_eq!(
        before,
        serde_json::to_value(session.game().save().unwrap()).unwrap()
    );
}

#[test]
fn rights_offerings_return_nonempty_owner_entitlement_without_other_holders() {
    use crate::company::rights_offering::*;
    use crate::company::share_registry::{
        AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
    };
    let date = |text| CivilDate::from_iso(text).unwrap();
    let mut session = company_session(CashDividendTaxMode::FlatWithholding);
    let other = session
        .join_market(
            crate::session::OpaqueSubjectId::new("rights-reader-other".into()).unwrap(),
            true,
        )
        .unwrap()
        .account_id;
    let plan = RightsOfferingEventPlan {
        event_id: "read-rights".into(),
        approval_reference: "只读 fixture 的明确批准".into(),
        issuer: CompanyId(0),
        stock: crate::StockCode("600888".into()),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: date("2029-12-20"),
        announced_on: date("2029-12-21"),
        registered_on: date("2029-12-24"),
        payment_start_on: date("2029-12-25"),
        payment_deadline_on: date("2029-12-31"),
        ex_rights_on: date("2030-01-02"),
        settlement_on: date("2030-01-03"),
        listing_on: date("2030-01-03"),
        price_per_share: crate::Money::from_cents(800),
        mode: RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 100_000,
        },
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    };
    let holdings = [AccountId(0), other]
        .into_iter()
        .map(|account| ShareHolding {
            holder: HolderId::Account(account),
            lots: vec![ShareLot {
                id: format!("opening-{}", account.0),
                qty: if account == AccountId(0) { 100 } else { 200 },
                acquired_on: date("2029-01-02"),
                source: AcquisitionSource::InitialAllocation {
                    evidence: "只读隔离 fixture".into(),
                },
                restriction: ShareRestriction::Unrestricted,
            }],
        })
        .collect();
    let mut registry = ShareRegistry::new(
        plan.stock.clone(),
        plan.issuer,
        300,
        plan.registered_on,
        holdings,
    )
    .unwrap();
    let snapshot = registry
        .register(plan.event_id.clone(), plan.registered_on)
        .unwrap()
        .clone();
    let mut book = RightsOfferingBook::new(plan).unwrap();
    book.announce(book.plan().announced_on).unwrap();
    book.entitle(
        snapshot,
        &crate::calendar::TradingCalendar::current_default_calendar().unwrap(),
    )
    .unwrap();
    // 仅验证领域账簿的宿主投影，不把局部装配当成生产 Session 认购／结算。
    session
        .game_mut()
        .state
        .companies
        .corporate_actions_mut()
        .unwrap()
        .rights_offerings
        .push(book);
    let expected_owner = session.game().owner_rights_offerings(AccountId(0)).unwrap();
    let other_owner = session.game().owner_rights_offerings(other).unwrap();
    assert_ne!(expected_owner, other_owner);
    assert_eq!(
        other_owner[0]
            .owner_entitlement
            .as_ref()
            .unwrap()
            .rights_shares,
        20
    );
    let before = serde_json::to_value(session.game().save().unwrap()).unwrap();
    let result = session
        .company_read_query(AccountId(0), CompanyReadQuery::OwnerRightsOfferings)
        .unwrap();
    emit_wire_fixture(&result);
    let CompanyReadResult::OwnerRightsOfferings(views) = result else {
        panic!("结果类型错误")
    };
    assert_eq!(views.len(), 1);
    assert_eq!(
        views[0].owner_entitlement.as_ref().unwrap().rights_shares,
        10
    );
    assert_eq!(
        views,
        session.game().owner_rights_offerings(AccountId(0)).unwrap()
    );
    assert_eq!(
        before,
        serde_json::to_value(session.game().save().unwrap()).unwrap()
    );
}

#[test]
fn public_failures_keep_specific_reasons_without_global_other_owner_facts() {
    let internal = CompanyReadError::DividendTaxOutstanding {
        source: SessionCorporateActionsError::Invalid("账户99的证券600999应税12345分失败".into()),
    };
    assert!(std::error::Error::source(&internal).is_some());
    let public = serde_json::to_string(&internal.public_failure()).unwrap();
    assert!(public.contains("未划收股息税汇总读取失败"));
    assert!(!public.contains("99"));
    assert!(!public.contains("600999"));
    assert!(!public.contains("12345"));
    let session = company_session(CashDividendTaxMode::AShareIndividual);
    let error = session
        .company_read_query(AccountId(0), CompanyReadQuery::OwnerFlatWithholdingReceipts)
        .unwrap_err();
    let public = serde_json::to_value(error.public_failure()).unwrap();
    assert!(public["data"]["reason"]
        .as_str()
        .unwrap()
        .contains("非 FlatWithholding"));
    let error = session
        .company_read_query(
            AccountId(0),
            CompanyReadQuery::CompanyPeriodExplanation {
                company: CompanyId(0),
                period_end: CivilDate::from_iso("2030-01-31").unwrap(),
            },
        )
        .unwrap_err();
    let public = serde_json::to_value(error.public_failure()).unwrap();
    assert_eq!(public["data"]["classification"], "BusinessCondition");
    assert!(public["data"]["reason"]
        .as_str()
        .unwrap()
        .contains("尚未结算"));
}

#[test]
fn company_query_uses_catalog_company_id_even_when_listed_stock_id_differs() {
    let mut session = company_session(CashDividendTaxMode::FlatWithholding);
    let listed = session
        .game()
        .company_system()
        .unwrap()
        .issuers()
        .get(&CompanyId(0))
        .unwrap()
        .clone();
    let mut unlisted = listed.clone();
    unlisted.code = "A-UNLISTED".into();
    unlisted.name = "未上市只读 fixture".into();
    unlisted.listed_stock = None;
    let crate::company::config::CompanySystemConfig::Simple(mut config) =
        simple_company_fixture!(crate; ["UNLISTED", "600888"])
    else {
        unreachable!()
    };
    config.companies[1].generation.initial_revenue =
        crate::accounting::AccountingAmount::from_cents(200_000_000);
    let system = crate::company::CompanySystem::create(
        vec![unlisted, listed],
        crate::company::config::CompanySystemConfig::Simple(config),
        session.civil_date(),
        7,
    )
    .unwrap();
    *session.game_mut().state.companies.system_arc_mut().unwrap() = std::sync::Arc::new(system);
    *session
        .game_mut()
        .state
        .companies
        .corporate_actions_mut()
        .unwrap() = crate::session::SessionCorporateActions::default();
    let entry = session
        .company_catalog_view()
        .unwrap()
        .into_iter()
        .find(|entry| entry.listed_stock.is_some())
        .unwrap();
    assert_eq!(entry.id, CompanyId(1));
    assert_eq!(entry.listed_stock, Some(crate::StockId(0)));
    let period_end = CivilDate::from_iso("2029-12-31").unwrap();
    let expected = session
        .game()
        .company_period_explanation(&entry.id, period_end)
        .unwrap();
    assert_ne!(
        expected,
        session
            .game()
            .company_period_explanation(&CompanyId(0), period_end)
            .unwrap()
    );
    let before = serde_json::to_value(session.game().save().unwrap()).unwrap();
    let result = session
        .company_read_query(
            AccountId(0),
            CompanyReadQuery::CompanyPeriodExplanation {
                company: entry.id,
                period_end,
            },
        )
        .unwrap();
    let CompanyReadResult::CompanyPeriodExplanation(actual) = result else {
        panic!("结果类型错误")
    };
    assert_eq!(actual, expected);
    assert_eq!(
        before,
        serde_json::to_value(session.game().save().unwrap()).unwrap()
    );
}

use super::*;
use crate::company::cash_dividend_tax::{
    CashDividendTaxBook, CashDividendTaxMode, DividendTaxProfile,
};
use crate::session::npc_working_quote_tests::quote_setup;
include!("../../../../test-support/simple_company.rs");

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
        let wire =
            serde_json::to_value(session.company_read_query(AccountId(0), query).unwrap()).unwrap();
        assert_eq!(wire["data"], expected);
    }
    assert_eq!(
        before,
        serde_json::to_value(session.game().save().unwrap()).unwrap()
    );
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

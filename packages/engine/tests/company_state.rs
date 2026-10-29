//! 直接编译真实互斥容器源码，域对象使用 engine 的正式公开类型。

pub use engine::*;

#[path = "../src/session/company_state.rs"]
mod company_state;

mod accessor_contract {
    use super::company_state::{LegacyCompanyState, SessionCompanies, SystemCompanyState};
    use super::*;
    use engine::accounting::closing::ClosingEngine;
    use engine::company::config::CompanySystemConfig;
    use engine::company::events::ShockParams;
    use engine::company::operations::{CompanyOperations, CompanyOperationsConfig};
    use engine::company::simple::period::{PeriodNoiseConfig, SettlementCycle};
    use engine::company::simple::{SimpleConfig, SimpleEnvironmentConfig};
    use engine::company::{CompanyCatalog, CompanyRegistry, CompanySystem};
    use engine::session::SessionCorporateActions;
    use std::sync::Arc;

    fn date() -> CivilDate {
        CivilDate::from_iso("2030-06-03").unwrap()
    }

    fn system_state() -> SessionCompanies {
        let system = CompanySystem::create(
            vec![],
            CompanySystemConfig::Simple(SimpleConfig {
                environment: SimpleEnvironmentConfig {
                    initial_change_bp: 0,
                    persistence_bp: 0,
                    noise: PeriodNoiseConfig {
                        monthly_bp: 0,
                        quarterly_bp: 0,
                        half_year_bp: 0,
                        annual_bp: 0,
                    },
                },
                companies: vec![],
                prehistory_periods: 1,
                settlement_cycle: SettlementCycle::Monthly,
            }),
            date(),
            19,
        )
        .unwrap();
        system.validate_restored().unwrap();
        SessionCompanies::CompanySystem(Box::new(SystemCompanyState {
            system: Arc::new(system),
            corporate_actions: SessionCorporateActions::default(),
        }))
    }

    fn legacy_state() -> SessionCompanies {
        let operations = CompanyOperations::new(
            CompanyOperationsConfig {
                seed: 19,
                shock_params: ShockParams::current_default_parameters(),
                catalog: CompanyCatalog::from_codes(&[]).unwrap(),
                companies: vec![],
            },
            date(),
        )
        .unwrap();
        SessionCompanies::LegacyOperations(LegacyCompanyState {
            registry: Arc::new(CompanyRegistry::new(vec![]).unwrap()),
            operations: Arc::new(operations),
            closing: Arc::new(ClosingEngine::new()),
            groups: vec![],
        })
    }

    #[test]
    fn legacy_accessors_mutate_only_the_selected_domain_and_save_it() {
        let mut state = legacy_state();
        assert!(state.legacy().unwrap().registry.is_empty());
        assert!(state.system_arc().is_none());
        assert!(state.system_arc_mut().is_none());
        assert!(state.corporate_actions_mut().is_none());
        let next = date().next().unwrap();
        Arc::make_mut(&mut state.legacy_mut().unwrap().operations)
            .advance_civil_day(date())
            .unwrap();
        let mut saved = state.save();
        assert!(saved.system().is_none());
        assert!(saved.actions().is_none());
        assert!(saved.issuer_ids().is_empty());
        assert_eq!(saved.legacy_for_test().0.next_expected_date(), next);
        saved
            .legacy_for_test_mut()
            .0
            .advance_civil_day(next)
            .unwrap();
        assert_eq!(
            saved.legacy_for_test().0.next_expected_date(),
            next.next().unwrap()
        );
        assert_eq!(
            state.legacy().unwrap().operations.next_expected_date(),
            next
        );
    }

    #[test]
    fn system_accessors_preserve_snapshot_isolation_and_save_mutations() {
        let mut state = system_state();
        assert!(state.legacy().is_none());
        assert!(state.legacy_mut().is_none());
        let snapshot = Arc::clone(state.system_arc().unwrap());
        let next = date();
        Arc::make_mut(state.system_arc_mut().unwrap())
            .advance_day(next)
            .unwrap();
        assert_eq!(snapshot.advanced_through(), date().prev().unwrap());
        assert_eq!(state.company_system().unwrap().advanced_through(), next);
        // 合成代扣回执只验证容器的读写与存档隔离，不执行付款或税务计算。
        let receipt = engine::session::FlatWithholdingReceipt {
            payment_id: "payment-1".into(),
            plan_id: "plan-1".into(),
            account: AccountId(1),
            paid_on: date(),
            gross: Money::from_cents(100),
            rate_bp: 1000,
            withheld: Money::from_cents(10),
        };
        state
            .corporate_actions_mut()
            .unwrap()
            .flat_withholding_receipts
            .push(receipt.clone());
        let mut saved = state.save();
        assert!(saved.issuer_ids().is_empty());
        assert_eq!(saved.system().unwrap().advanced_through(), next);
        assert_eq!(
            saved.actions().unwrap().flat_withholding_receipts,
            vec![receipt.clone()]
        );
        saved
            .actions_for_test_mut()
            .flat_withholding_receipts
            .clear();
        let later = next.next().unwrap();
        saved.system_for_test_mut().advance_day(later).unwrap();
        assert!(saved
            .actions()
            .unwrap()
            .flat_withholding_receipts
            .is_empty());
        assert_eq!(saved.system().unwrap().advanced_through(), later);
        assert_eq!(
            state.corporate_actions().unwrap().flat_withholding_receipts,
            vec![receipt]
        );
        assert_eq!(state.company_system().unwrap().advanced_through(), next);
    }

    #[test]
    #[should_panic(expected = "测试必须显式构造 LegacyOperations")]
    fn system_save_rejects_legacy_test_read() {
        system_state().save().legacy_for_test();
    }

    #[test]
    #[should_panic(expected = "测试必须显式构造 LegacyOperations")]
    fn system_save_rejects_legacy_test_write() {
        system_state().save().legacy_for_test_mut();
    }

    #[test]
    #[should_panic(expected = "测试必须显式构造 CompanySystem")]
    fn legacy_save_rejects_system_test_write() {
        legacy_state().save().system_for_test_mut();
    }

    #[test]
    #[should_panic(expected = "测试必须显式构造 CompanySystem")]
    fn legacy_save_rejects_actions_test_write() {
        legacy_state().save().actions_for_test_mut();
    }
}

use super::*;

fn simple_setup() -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system {
        config.prehistory_periods = 12;
    }
    setup
}

fn session_with_approved_cash_dividend() -> (GameSession, crate::company::CompanyId, crate::account::StockCode) {
    use crate::company::{
        cash_dividend::CashDividendPlan,
        ex_reference_price::CashDividendFormula,
        share_registry::{AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction},
    };
    use crate::account::Position;
    use crate::orderbook::AccountId;
    use crate::accounting::AccountingAmount;

    let mut setup = simple_setup();
    setup.npcs.inst_count = 1;
    setup.ticks_per_day = 1;
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system {
        config.prehistory_periods = 24;
        config.settlement_cycle = crate::company::simple::period::SettlementCycle::Monthly;
    }
    let mut session = GameSession::new(setup, 42).unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let issuer = session.state.company_system.issuers().issuer_of(&stock).unwrap().clone();
    let date = |value| crate::CivilDate::from_iso(value).unwrap();
    let approved_on = date("2030-01-02");
    let announced_on = date("2030-01-03");
    let registered_on = date("2030-01-04");
    let ex_date = date("2030-01-07");
    let payable_on = date("2030-01-08");
    let capital = AccountingAmount::from_cents(1_000_000);
    session.define_dividend_legal_facts(&issuer, capital, "explicit test legal fact".into()).unwrap();
    let total_shares = session.state.setup.stocks[0].total_shares;
    session.state.accounts.get_mut(&AccountId(0)).unwrap().fixture_insert_position(
        stock.clone(), Position::from_restored_parts(1, 0, 1_000, 0),
    );
    let registry = ShareRegistry::new(
        stock.clone(), issuer.clone(), total_shares, approved_on,
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(0)),
                lots: vec![ShareLot {
                    id: "player-lot".into(), qty: 1, acquired_on: approved_on,
                    source: AcquisitionSource::InitialAllocation { evidence: "fixture".into() },
                    restriction: ShareRestriction::Unrestricted,
                }],
            },
            ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![ShareLot {
                    id: "treasury-lot".into(), qty: total_shares - 2, acquired_on: approved_on,
                    source: AcquisitionSource::InitialAllocation { evidence: "fixture".into() },
                    restriction: ShareRestriction::Unrestricted,
                }],
            },
            ShareHolding {
                holder: HolderId::External("external-holder".into()),
                lots: vec![ShareLot {
                    id: "external-lot".into(), qty: 1, acquired_on: approved_on,
                    source: AcquisitionSource::InitialAllocation { evidence: "fixture".into() },
                    restriction: ShareRestriction::Unrestricted,
                }],
            },
        ],
    ).unwrap();
    session.configure_share_registry(registry).unwrap();
    let plan = CashDividendPlan::new(
        "announcement-test".into(), issuer.clone(), stock.clone(), crate::calendar::CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly, approved_on, announced_on, registered_on, ex_date, payable_on,
        Money::from_cents(1), Money::from_cents(2), session.state.civil_clock.calendar(),
    ).unwrap();
    session.approve_cash_dividend(crate::company::DividendDeclaration {
        plan_id: plan.plan_id.clone(), approved_on,
        total_gross: AccountingAmount::from_cents(2), registered_capital: capital,
    }, plan).unwrap();
    let account = AccountId(1);
    session.state.belief_participants.get_mut(&account).unwrap()
        .watchlist_mut().record_attention(&stock, 0, 0).unwrap();
    session.state.npc_attention.get_mut(&account).unwrap().information_cadence = NpcInformationCadence::Immediate;
    let opened = session.observation_civil_instant();
    let known_reports = session.state.library.reports_for_company(&issuer, opened);
    assert!(known_reports.iter().any(|report| report.reports.kind == crate::accounting::reports::ReportKind::Annual),
        "fixture must expose a genuinely published annual report for institutional revaluation");
    session.deliver_public_information(opened).unwrap();
    assert!(session.state.belief_participants[&account].belief().entry(&stock).is_some(),
        "institution must form a baseline from its own public annual material");
    (session, issuer, stock)
}

fn complete_test_civil_day(session: &mut GameSession) -> CivilDayEndReport {
    for _ in 0..session.state.setup.ticks_per_day {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap()
}

#[test]
fn approved_cash_dividend_becomes_public_and_is_personally_acquired_only_at_disclosure() {
    use crate::orderbook::AccountId;
    use crate::information::AnnouncementContent;
    let (mut session, issuer, stock) = session_with_approved_cash_dividend();
    let account = AccountId(1);
    let previous_cause = session.state.belief_participants[&account].belief().entry(&stock)
        .and_then(|entry| entry.last_cause.clone());
    let next_day = complete_test_civil_day(&mut session);
    assert!(session.state.library.announcements_for_company(&issuer, next_day.disclosure_instant)
        .iter().all(|announcement| !matches!(announcement.content, AnnouncementContent::CashDividend(_))));

    let disclosure = complete_test_civil_day(&mut session);
    let announcement = session.state.library.announcements_for_company(&issuer, disclosure.disclosure_instant)
        .into_iter().find(|announcement| matches!(announcement.content, AnnouncementContent::CashDividend(_)))
        .expect("approved cash dividend is announced at its planned 18:00 phase");
    assert_eq!(announcement.published_at, disclosure.disclosure_instant);
    let acquired = session.state.belief_participants[&account].information().observed_at_of(announcement.id);
    assert_eq!(acquired, Some(disclosure.disclosure_instant));
    assert_eq!(session.state.belief_participants[&account].belief().entry(&stock).unwrap().last_cause, previous_cause,
        "cash-dividend acquisition must not apply a Shock or CreditDefault cause");
    let restored = GameSession::restore(&session.save().unwrap()).unwrap();
    assert_eq!(restored.state.belief_participants[&account].information().observed_at_of(announcement.id), acquired);
}

#[test]
fn restore_rejects_cash_dividend_announcement_gross_that_disagrees_with_approved_finance_fact() {
    use crate::information::AnnouncementContent;
    let (mut session, issuer, _) = session_with_approved_cash_dividend();
    complete_test_civil_day(&mut session);
    complete_test_civil_day(&mut session);
    let save = session.save().unwrap();
    let mut value = serde_json::to_value(&save).unwrap();
    let announcement = value["public_library"]["announcements"].as_array_mut().unwrap().iter_mut()
        .find(|announcement| announcement["company"] == serde_json::to_value(&issuer).unwrap())
        .expect("cash dividend announcement is persisted");
    assert_eq!(announcement["content"]["kind"], "CashDividend");
    let AnnouncementContent::CashDividend(dividend) = serde_json::from_value(announcement["content"].clone()).unwrap() else {
        panic!("expected cash dividend announcement");
    };
    let mut dividend = dividend;
    dividend.total_gross = Money::from_cents(1);
    announcement["content"] = serde_json::to_value(AnnouncementContent::CashDividend(dividend)).unwrap();
    let corrupted: SaveSlot = serde_json::from_value(value).unwrap();
    assert!(matches!(GameSession::restore(&corrupted), Err(SessionError::InvalidSave(_))));
}

#[test]
fn repeated_session_ticks_on_cash_ex_date_do_not_subtract_dividend_twice() {
    use crate::company::{
        cash_dividend::CashDividendPlan,
        ex_reference_price::CashDividendFormula,
        share_registry::{AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction},
    };
    use crate::account::Position;
    use crate::orderbook::AccountId;
    use crate::accounting::AccountingAmount;

    let mut setup = simple_setup();
    setup.ticks_per_day = 2;
    setup.start_date = crate::calendar::CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = GameSession::new(setup, 42).unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let issuer = crate::company::CompanyId(format!("C-{}", stock.0));
    let approved_on = crate::calendar::CivilDate::from_iso("2030-01-02").unwrap();
    let announced_on = crate::calendar::CivilDate::from_iso("2030-01-03").unwrap();
    let registered_on = crate::calendar::CivilDate::from_iso("2030-01-04").unwrap();
    let ex_date = crate::calendar::CivilDate::from_iso("2030-01-07").unwrap();
    let payable_on = crate::calendar::CivilDate::from_iso("2030-01-08").unwrap();
    let capital = AccountingAmount::from_cents(1_000_000);
    session.define_dividend_legal_facts(&issuer, capital, "explicit test legal fact".into()).unwrap();
    session.state.accounts.get_mut(&AccountId(0)).unwrap().fixture_insert_position(
        stock.clone(), Position::from_restored_parts(1, 0, 1_000, 0),
    );
    let registry = ShareRegistry::new(
        stock.clone(),
        issuer.clone(),
        session.state.setup.stocks[0].total_shares,
        approved_on,
        vec![
            ShareHolding { holder: HolderId::Account(AccountId(0)), lots: vec![ShareLot {
                id: "explicit-player-lot".into(), qty: 1, acquired_on: approved_on,
                source: AcquisitionSource::InitialAllocation { evidence: "explicit setup facts".into() },
                restriction: ShareRestriction::Unrestricted,
            }] },
            ShareHolding { holder: HolderId::IssuerTreasury, lots: vec![ShareLot {
                id: "explicit-treasury-lot".into(), qty: session.state.setup.stocks[0].total_shares - 2,
                acquired_on: approved_on,
                source: AcquisitionSource::InitialAllocation { evidence: "explicit setup facts".into() },
                restriction: ShareRestriction::Unrestricted,
            }] },
            ShareHolding {
            holder: HolderId::External("explicit-owner".into()),
            lots: vec![ShareLot {
                id: "explicit-lot".into(),
                qty: 1,
                acquired_on: approved_on,
                source: AcquisitionSource::InitialAllocation { evidence: "fixture".into() },
                restriction: ShareRestriction::Unrestricted,
            }],
        }],
    ).unwrap();
    session.configure_share_registry(registry.clone()).unwrap();
    let calendar = session.state.civil_clock.calendar().clone();
    let initial_cash = session.account(AccountId(0)).unwrap().cash();
    for (plan_id, cents) in [("ex-a", 1), ("ex-b", 2)] {
        let plan = CashDividendPlan::new(
            plan_id.into(), issuer.clone(), stock.clone(), crate::calendar::CalendarExchange::Sse,
            CashDividendFormula::StandardCashOnly,
            approved_on, announced_on, registered_on, ex_date, payable_on,
            Money::from_cents(cents), Money::from_cents(cents * 2), &calendar,
        ).unwrap();
        session.approve_cash_dividend(crate::company::DividendDeclaration {
            plan_id: plan_id.into(), approved_on,
            total_gross: AccountingAmount::from_cents(i128::from(cents) * 2), registered_capital: capital,
        }, plan).unwrap();
    }
    let finish_day = |session: &mut GameSession| {
        for _ in 0..session.state.setup.ticks_per_day { session.step().unwrap(); }
        session.end_civil_day().unwrap();
    };
    finish_day(&mut session);
    finish_day(&mut session);
    finish_day(&mut session);
    session.end_civil_day().unwrap();
    session.end_civil_day().unwrap();
    let previous_close = session.state.markets[&stock].last_close();
    let mut failed_candidate = session.clone_for_tick_shadow().unwrap();
    failed_candidate.inject_post_shadow_failure(crate::session::StepFatal::InvariantViolation {
        description: "fixture rejects completed private shadow".into(),
        location: "cash-ex-atomicity-test".into(),
    });
    let before_failure = failed_candidate.business_state_hash().unwrap();
    assert!(failed_candidate.step().is_err());
    assert_eq!(failed_candidate.business_state_hash().unwrap(), before_failure);
    assert!(failed_candidate.state.corporate_actions.applied_ex_dividend_groups.is_empty());
    assert_eq!(failed_candidate.state.markets[&stock].last_cash_ex_reference(), None);
    session.step().unwrap();
    let once = session.state.markets[&stock].last_close();
    let intraday_save = session.save().unwrap();
    let mut restored = GameSession::restore(&intraday_save).unwrap();
    restored.step().unwrap();
    let twice = restored.state.markets[&stock].last_close();
    assert_eq!(once, previous_close.sub(Money::from_cents(3)).unwrap());
    assert_eq!(twice, once);
    restored.end_civil_day().unwrap();
    finish_day(&mut restored);
    let before_payment = restored.account(AccountId(0)).unwrap().cash();
    assert_eq!(before_payment, initial_cash.add(Money::from_cents(3)).unwrap());
    assert_eq!(restored.corporate_actions().account_gross_receipts.len(), 2);
    assert_eq!(restored.corporate_actions().external_receipts.len(), 2);
    assert!(restored.corporate_actions().account_gross_receipts.iter().all(|receipt| receipt.tax_status == crate::session::corporate_actions::DividendTaxStatus::TreatmentNotConfigured));
    let mut corrupt_actions = restored.state.corporate_actions.clone();
    corrupt_actions.account_gross_receipts[0].paid_on = restored.civil_date().next().unwrap();
    let positions = restored.state.accounts.iter().map(|(id, account)| {
        (*id, account.positions().iter().map(|(code, position)| (code.clone(), u64::from(position.qty()))).collect())
    }).collect();
    assert!(matches!(corrupt_actions.validate(&positions, &restored.state.company_system, restored.civil_date()), Err(crate::session::SessionCorporateActionsError::Invalid(message)) if message.contains("到账日期晚于")));
    let saved = restored.save().unwrap();
    let mut paid_restore = GameSession::restore(&saved).unwrap();
    assert_eq!(paid_restore.account(AccountId(0)).unwrap().cash(), before_payment);
    assert_eq!(paid_restore.corporate_actions().account_gross_receipts.len(), 2);
}

#[test]
fn future_approved_date_cash_dividend_is_rejected_atomically() {
    let session = GameSession::new(simple_setup(), 42).unwrap();
    let before = session.business_state_hash().unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let issuer = crate::company::CompanyId(format!("C-{}", stock.0));
    let date = |value| crate::calendar::CivilDate::from_iso(value).unwrap();
    let plan = crate::company::cash_dividend::CashDividendPlan::new(
        "future-plan".into(), issuer, stock, crate::calendar::CalendarExchange::Sse,
        crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
        date("2030-01-02"), date("2030-01-02"), date("2030-01-03"), date("2030-01-04"), date("2030-01-07"),
        Money::from_cents(1), Money::from_cents(1), session.state.civil_clock.calendar(),
    ).unwrap();
    let declaration = crate::company::DividendDeclaration {
        plan_id: "future-plan".into(), approved_on: date("2030-01-02"),
        total_gross: crate::accounting::AccountingAmount::from_cents(1),
        registered_capital: crate::accounting::AccountingAmount::from_cents(1),
    };
    let mut candidate = session;
    assert!(candidate.approve_cash_dividend(declaration, plan).is_err());
    assert_eq!(candidate.business_state_hash().unwrap(), before);
    assert!(candidate.corporate_actions().dividends.is_empty());
}

#[test]
fn issuer_identity_uses_explicit_company_kind_instead_of_stock_code() {
    for kind in [crate::company::CompanyKind::Bank, crate::company::CompanyKind::Insurance, crate::company::CompanyKind::RealEstate] {
        let mut setup = simple_setup();
        if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system {
            config.companies[0].kind = kind;
        }
        let specs = company_assembly::issuer_specs(&setup).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].kind, kind);
        assert_eq!(specs[0].listed_stock, Some(setup.stocks[0].code.clone()));
        assert_eq!(specs[0].issued_shares, setup.stocks[0].total_shares);
    }
}

#[test]
fn issuer_identity_rejects_missing_duplicate_configuration_and_unavailable_simulation() {
    let setup = simple_setup();
    for duplicate in [false, true] {
        let mut changed = setup.clone();
        if let crate::company::config::CompanySystemConfig::Simple(config) = &mut changed.company_system {
            if duplicate {
                config.companies.push(config.companies[0].clone());
            } else {
                config.companies.clear();
            }
        }
        assert!(matches!(company_assembly::issuer_specs(&changed), Err(SessionError::InvalidSetup(_))));
    }
    let mut unavailable = setup;
    unavailable.company_system = crate::company::config::CompanySystemConfig::Simulation;
    assert!(matches!(company_assembly::issuer_specs(&unavailable), Err(SessionError::InvalidSetup(message)) if message.contains("Simulation")));
}

#[test]
fn explicit_financial_kinds_publish_matching_policy_and_restore_actual_session() {
    use crate::accounting::{AccountingAmount, JournalLine, LedgerAccountId, PostingSide};
    for (kind, cash_code, chart_version) in [
        (crate::company::CompanyKind::Bank, "1003", 3),
        (crate::company::CompanyKind::Insurance, "1002", 4),
        (crate::company::CompanyKind::RealEstate, "1002", 5),
    ] {
        let mut setup = simple_setup();
        if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system {
            config.companies[0].kind = kind;
            config.companies[0].finance.opening_lines = vec![
                JournalLine { account: LedgerAccountId(cash_code.into()), side: PostingSide::Debit, amount: AccountingAmount::from_cents(500_000_000) },
                JournalLine { account: LedgerAccountId("4001".into()), side: PostingSide::Credit, amount: AccountingAmount::from_cents(500_000_000) },
            ];
        }
        let session = GameSession::new(setup, 42).unwrap();
        let save = session.save().unwrap();
        let company = crate::company::CompanyId(format!("C-{}", save.setup.stocks[0].code.0));
        assert_eq!(save.company_system.issuers().get(&company).unwrap().kind, kind);
        let public = save.public_library.save();
        assert!(!public.reports.is_empty());
        assert!(public.reports.iter().all(|report| report.company == company && report.policy.chart_version == chart_version));
        let restored = GameSession::restore(&save).unwrap();
        assert_eq!(restored.business_state_hash().unwrap(), session.business_state_hash().unwrap());
    }
}

#[test]
fn session_setup_rejects_legacy_company_fields_without_compatibility() {
    for field in ["company_operations", "groups"] {
        let mut wire = serde_json::to_value(simple_setup()).unwrap();
        wire[field] = serde_json::json!(null);
        assert!(serde_json::from_value::<SessionSetup>(wire).is_err());
    }
}

#[test]
fn restore_rejects_company_config_and_civil_date_drift() {
    let session = GameSession::new(simple_setup(), 42).unwrap();
    let save = session.save().unwrap();
    let mut changed = save.clone();
    if let crate::company::config::CompanySystemConfig::Simple(config) = &mut changed.setup.company_system {
        config.environment.noise.monthly_bp += 1;
    }
    assert!(matches!(GameSession::restore(&changed), Err(SessionError::InvalidSave(message)) if message.contains("配置")));
    let mut wire = serde_json::to_value(&save).unwrap();
    wire["company_system"]["implementation"]["state"]["advanced_through"] = serde_json::json!("2030-01-01");
    let edited: SaveSlot = serde_json::from_value(wire).unwrap();
    assert!(matches!(GameSession::restore(&edited), Err(SessionError::InvalidSave(message)) if message.contains("日期")));
}

#[test]
fn simple_restore_rejects_foreign_model_publication_source() {
    let session = GameSession::new(simple_setup(), 42).unwrap();
    let mut save = session.save().unwrap();
    let mut library = save.public_library.save();
    assert!(!library.reports.is_empty());
    library.reports[0].source = crate::information::PublicationSource::SimulationAccounting;
    save.public_library = crate::information::PublicLibrary::from_parts(library).unwrap();
    assert!(matches!(GameSession::restore(&save), Err(SessionError::InvalidSave(message)) if message.contains("来源")));
}

#[test]
fn simple_session_save_has_selected_system_without_financial_simulation() {
    let session = GameSession::new(simple_setup(), 42).unwrap();
    let save = serde_json::to_value(session.save().unwrap()).unwrap();
    assert_eq!(save["company_system"]["implementation"]["mode"], "Simple");
    for field in ["company_operations", "closing_registry", "ops_wiring"] {
        assert!(save.get(field).is_none(), "Simple 存档不得携带 {field}");
    }
    assert_eq!(save["company_system"]["implementation"]["state"]["advanced_through"], "2029-12-31");
}

#[test]
fn session_setup_requires_explicit_company_model_without_legacy_fallback() {
    let mut setup = serde_json::to_value(simple_setup()).unwrap();
    setup.as_object_mut().unwrap().remove("company_system");
    assert!(serde_json::from_value::<SessionSetup>(setup).is_err());
}

#[test]
fn unavailable_simulation_is_rejected_before_creating_session() {
    let mut setup = serde_json::to_value(simple_setup()).unwrap();
    setup["company_system"] = serde_json::json!({ "mode": "Simulation" });
    let parsed: SessionSetup = serde_json::from_value(setup).unwrap();
    assert!(matches!(GameSession::new(parsed, 42), Err(SessionError::InvalidSetup(message)) if message.contains("Simulation")));
}

#[test]
fn simple_day_end_advances_selected_state_and_restore_preserves_it() {
    let mut session = GameSession::new(simple_setup(), 42).unwrap();
    session.end_civil_day().unwrap();
    let save = session.save().unwrap();
    let wire = serde_json::to_value(&save).unwrap();
    assert_eq!(wire["company_system"]["implementation"]["state"]["advanced_through"], "2030-01-01");
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(restored.business_state_hash().unwrap(), session.business_state_hash().unwrap());
}

#[test]
fn monthly_intraday_checkpoint_restores_without_becoming_public_day_end_archive() {
    let mut setup = simple_setup();
    setup.report_frequency = crate::information::ReportFrequency::Monthly {
        schedule: crate::information::MonthlyReportSchedule::Custom {
            day: 2,
            second_of_day: 37800,
            delay: crate::information::MonthlyReportDelay::None,
        },
    };
    let mut session = GameSession::new(setup, 42).unwrap();
    session.end_civil_day().unwrap();
    session.step().unwrap();
    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(restored.business_state_hash().unwrap(), session.business_state_hash().unwrap());
    assert!(crate::session::protocol::ProtocolSession::restore(&save).is_err());
    for future_second in [37800, 54000, 86399] {
        let mut wire = serde_json::to_value(&save).unwrap();
        wire["disclosures"]["published_through"] = serde_json::to_value(CivilInstant::new(session.civil_date(), future_second).unwrap()).unwrap();
        let edited: SaveSlot = serde_json::from_value(wire).unwrap();
        assert!(matches!(GameSession::restore(&edited), Err(SessionError::InvalidSave(message)) if message.contains("disclosure cursor")));
    }
}

#[test]
fn disclosure_checkpoint_handles_open_close_and_closed_day_without_relaxing_public_archive() {
    for frequency in [
        crate::information::ReportFrequency::Quarterly,
        crate::information::ReportFrequency::Monthly {
            schedule: crate::information::MonthlyReportSchedule::Custom {
                day: 2,
                second_of_day: 37800,
                delay: crate::information::MonthlyReportDelay::None,
            },
        },
    ] {
        let mut setup = simple_setup();
        setup.start_date = CivilDate::from_ymd(2030, 1, 4).unwrap();
        setup.ticks_per_day = 2;
        setup.npcs.inst_count = 0;
        if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system {
            config.prehistory_periods = 0;
        }
        setup.report_frequency = frequency;
        let mut session = GameSession::new(setup, 42).unwrap();
        let initial = session.save().unwrap();
        assert!(GameSession::restore(&initial).is_ok());
        for _ in 0..2 {
            session.step().unwrap();
            let checkpoint = session.save().unwrap();
            let restored = GameSession::restore(&checkpoint).unwrap();
            assert_eq!(restored.business_state_hash().unwrap(), session.business_state_hash().unwrap());
            assert!(crate::session::protocol::ProtocolSession::restore(&checkpoint).is_err());
        }
        for _ in 0..2 {
            session.end_civil_day().unwrap();
            assert_eq!(session.civil_clock().phase(), CivilPhase::ClosedDay);
            let archive = session.save().unwrap();
            assert!(GameSession::restore(&archive).is_ok());
            assert!(crate::session::protocol::ProtocolSession::restore(&archive).is_ok());
        }
    }
}

use super::*;

fn simple_setup() -> SessionSetup {
    let mut setup = npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    if let crate::company::config::CompanySystemConfig::Simple(config) = &mut setup.company_system {
        config.prehistory_periods = 12;
    }
    setup
}

fn session_with_approved_cash_dividend() -> (
    GameSession,
    crate::company::CompanyId,
    crate::account::StockCode,
) {
    use crate::account::Position;
    use crate::accounting::AccountingAmount;
    use crate::company::{
        cash_dividend::CashDividendPlan,
        ex_reference_price::CashDividendFormula,
        share_registry::{
            AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
        },
    };
    use crate::orderbook::AccountId;

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
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&stock)
        .unwrap()
        .clone();
    let date = |value| crate::CivilDate::from_iso(value).unwrap();
    let approved_on = date("2030-01-02");
    let announced_on = date("2030-01-03");
    let registered_on = date("2030-01-04");
    let ex_date = date("2030-01-07");
    let payable_on = date("2030-01-08");
    let capital = AccountingAmount::from_cents(1_000_000);
    session
        .define_dividend_legal_facts(&issuer, capital, "explicit test legal fact".into())
        .unwrap();
    let total_shares = session.state.setup.stocks[0].total_shares;
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(stock.clone(), Position::from_restored_parts(5, 0, 1_000, 0));
    session
        .state
        .accounts
        .get_mut(&AccountId(1))
        .unwrap()
        .fixture_insert_position(stock.clone(), Position::from_restored_parts(1, 0, 1_000, 0));
    let registry = ShareRegistry::new(
        stock.clone(),
        issuer.clone(),
        total_shares,
        approved_on,
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(0)),
                lots: vec![ShareLot {
                    id: "player-lot".into(),
                    qty: 5,
                    acquired_on: approved_on,
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "fixture".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            },
            ShareHolding {
                holder: HolderId::Account(AccountId(1)),
                lots: vec![ShareLot {
                    id: "institution-lot".into(),
                    qty: 1,
                    acquired_on: approved_on,
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "fixture".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            },
            ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![ShareLot {
                    id: "treasury-lot".into(),
                    qty: total_shares - 7,
                    acquired_on: approved_on,
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "fixture".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            },
            ShareHolding {
                holder: HolderId::External("external-holder".into()),
                lots: vec![ShareLot {
                    id: "external-lot".into(),
                    qty: 1,
                    acquired_on: approved_on,
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "fixture".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            },
        ],
    )
    .unwrap();
    session.configure_share_registry(registry).unwrap();
    assert_eq!(
        session.account(AccountId(0)).unwrap().sellable_qty(&stock),
        5,
        "fixture must keep the dividend lot sellable on payment date"
    );
    session
        .configure_cash_dividend_tax_book(
            AccountId(0),
            stock.clone(),
            crate::company::cash_dividend_tax::DividendTaxProfile::IndividualPublicMarket,
        )
        .unwrap();
    let plan = CashDividendPlan::new(
        "announcement-test".into(),
        issuer.clone(),
        stock.clone(),
        crate::calendar::CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        approved_on,
        announced_on,
        registered_on,
        ex_date,
        payable_on,
        Money::from_cents(10),
        Money::from_cents(70),
        session.state.civil_clock.calendar(),
    )
    .unwrap();
    session
        .approve_cash_dividend(
            crate::company::DividendDeclaration {
                plan_id: plan.plan_id.clone(),
                approved_on,
                total_gross: AccountingAmount::from_cents(70),
                registered_capital: capital,
            },
            plan,
        )
        .unwrap();
    let account = AccountId(1);
    session
        .state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .watchlist_mut()
        .record_attention(&stock, 0, 0)
        .unwrap();
    session
        .state
        .npc_attention
        .get_mut(&account)
        .unwrap()
        .information_cadence = NpcInformationCadence::Immediate;
    let opened = session.observation_civil_instant();
    let known_reports = session.state.library.reports_for_company(&issuer, opened);
    assert!(
        known_reports
            .iter()
            .any(|report| report.reports.kind == crate::accounting::reports::ReportKind::Annual),
        "fixture must expose a genuinely published annual report for institutional revaluation"
    );
    session.deliver_public_information(opened).unwrap();
    assert!(
        session.state.belief_participants[&account]
            .belief()
            .entry(&stock)
            .is_some(),
        "institution must form a baseline from its own public annual material"
    );
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
    use crate::information::AnnouncementContent;
    use crate::orderbook::AccountId;
    let (mut session, issuer, stock) = session_with_approved_cash_dividend();
    let account = AccountId(1);
    let previous_cause = session.state.belief_participants[&account]
        .belief()
        .entry(&stock)
        .and_then(|entry| entry.last_cause.clone());
    let next_day = complete_test_civil_day(&mut session);
    assert!(session
        .state
        .library
        .announcements_for_company(&issuer, next_day.disclosure_instant)
        .iter()
        .all(|announcement| !matches!(announcement.content, AnnouncementContent::CashDividend(_))));

    let disclosure = complete_test_civil_day(&mut session);
    let announcement = session
        .state
        .library
        .announcements_for_company(&issuer, disclosure.disclosure_instant)
        .into_iter()
        .find(|announcement| matches!(announcement.content, AnnouncementContent::CashDividend(_)))
        .expect("approved cash dividend is announced at its planned 18:00 phase");
    assert_eq!(announcement.published_at, disclosure.disclosure_instant);
    let acquired = session.state.belief_participants[&account]
        .information()
        .observed_at_of(announcement.id);
    assert_eq!(acquired, Some(disclosure.disclosure_instant));
    assert_eq!(
        session.state.belief_participants[&account]
            .belief()
            .entry(&stock)
            .unwrap()
            .last_cause,
        previous_cause,
        "cash-dividend acquisition must not apply a Shock or CreditDefault cause"
    );
    let restored = GameSession::restore(&session.save().unwrap()).unwrap();
    assert_eq!(
        restored.state.belief_participants[&account]
            .information()
            .observed_at_of(announcement.id),
        acquired
    );
}

#[test]
fn restore_rejects_cash_dividend_announcement_gross_that_disagrees_with_approved_finance_fact() {
    use crate::information::AnnouncementContent;
    let (mut session, issuer, _) = session_with_approved_cash_dividend();
    complete_test_civil_day(&mut session);
    complete_test_civil_day(&mut session);
    let save = session.save().unwrap();
    let mut value = serde_json::to_value(&save).unwrap();
    let announcement = value["public_library"]["announcements"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|announcement| announcement["company"] == serde_json::to_value(&issuer).unwrap())
        .expect("cash dividend announcement is persisted");
    assert_eq!(announcement["content"]["kind"], "CashDividend");
    let AnnouncementContent::CashDividend(dividend) =
        serde_json::from_value(announcement["content"].clone()).unwrap()
    else {
        panic!("expected cash dividend announcement");
    };
    let mut dividend = dividend;
    dividend.total_gross = Money::from_cents(1);
    announcement["content"] =
        serde_json::to_value(AnnouncementContent::CashDividend(dividend)).unwrap();
    let corrupted: SaveSlot = serde_json::from_value(value).unwrap();
    assert!(matches!(
        GameSession::restore(&corrupted),
        Err(SessionError::InvalidSave(_))
    ));
}

#[test]
fn repeated_session_ticks_on_cash_ex_date_do_not_subtract_dividend_twice() {
    use crate::account::Position;
    use crate::accounting::AccountingAmount;
    use crate::company::{
        cash_dividend::CashDividendPlan,
        ex_reference_price::CashDividendFormula,
        share_registry::{
            AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
        },
    };
    use crate::orderbook::AccountId;

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
    session
        .define_dividend_legal_facts(&issuer, capital, "explicit test legal fact".into())
        .unwrap();
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(stock.clone(), Position::from_restored_parts(1, 0, 1_000, 0));
    let registry = ShareRegistry::new(
        stock.clone(),
        issuer.clone(),
        session.state.setup.stocks[0].total_shares,
        approved_on,
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(0)),
                lots: vec![ShareLot {
                    id: "explicit-player-lot".into(),
                    qty: 1,
                    acquired_on: approved_on,
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "explicit setup facts".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            },
            ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![ShareLot {
                    id: "explicit-treasury-lot".into(),
                    qty: session.state.setup.stocks[0].total_shares - 2,
                    acquired_on: approved_on,
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "explicit setup facts".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            },
            ShareHolding {
                holder: HolderId::External("explicit-owner".into()),
                lots: vec![ShareLot {
                    id: "explicit-lot".into(),
                    qty: 1,
                    acquired_on: approved_on,
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "fixture".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            },
        ],
    )
    .unwrap();
    session.configure_share_registry(registry.clone()).unwrap();
    session
        .configure_cash_dividend_tax_book(
            AccountId(0),
            stock.clone(),
            crate::company::cash_dividend_tax::DividendTaxProfile::IndividualPublicMarket,
        )
        .unwrap();
    let calendar = session.state.civil_clock.calendar().clone();
    let initial_cash = session.account(AccountId(0)).unwrap().cash();
    for (plan_id, cents) in [("ex-a", 1), ("ex-b", 2)] {
        let plan = CashDividendPlan::new(
            plan_id.into(),
            issuer.clone(),
            stock.clone(),
            crate::calendar::CalendarExchange::Sse,
            CashDividendFormula::StandardCashOnly,
            approved_on,
            announced_on,
            registered_on,
            ex_date,
            payable_on,
            Money::from_cents(cents),
            Money::from_cents(cents * 2),
            &calendar,
        )
        .unwrap();
        session
            .approve_cash_dividend(
                crate::company::DividendDeclaration {
                    plan_id: plan_id.into(),
                    approved_on,
                    total_gross: AccountingAmount::from_cents(i128::from(cents) * 2),
                    registered_capital: capital,
                },
                plan,
            )
            .unwrap();
    }
    let finish_day = |session: &mut GameSession| {
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
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
    assert_eq!(
        failed_candidate.business_state_hash().unwrap(),
        before_failure
    );
    assert!(failed_candidate
        .state
        .corporate_actions
        .applied_ex_reference_groups
        .is_empty());
    assert_eq!(
        failed_candidate.state.markets[&stock].last_cash_ex_reference(),
        None
    );
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
    assert_eq!(
        before_payment,
        initial_cash.add(Money::from_cents(3)).unwrap()
    );
    let tax_book = &restored.corporate_actions().dividend_tax_books[0];
    assert_eq!(tax_book.collections().len(), 7);
    assert_eq!(
        tax_book
            .collections()
            .iter()
            .map(|receipt| receipt.collected.cents())
            .sum::<i64>(),
        0
    );
    assert!(tax_book
        .collections()
        .iter()
        .all(|receipt| !receipt.needs_funds));
    assert_eq!(restored.corporate_actions().account_gross_receipts.len(), 2);
    assert_eq!(restored.corporate_actions().external_receipts.len(), 2);
    assert!(restored
        .corporate_actions()
        .account_gross_receipts
        .iter()
        .filter(|receipt| receipt.account == AccountId(0))
        .all(|receipt| receipt.tax_status
            == crate::session::corporate_actions::DividendTaxStatus::IndividualPublicMarket));
    assert!(restored
        .corporate_actions()
        .account_gross_receipts
        .iter()
        .filter(|receipt| receipt.account == AccountId(1))
        .all(|receipt| receipt.tax_status
            == crate::session::corporate_actions::DividendTaxStatus::TreatmentNotConfigured));
    let mut corrupt_actions = restored.state.corporate_actions.clone();
    corrupt_actions.account_gross_receipts[0].paid_on = restored.civil_date().next().unwrap();
    let positions = restored
        .state
        .accounts
        .iter()
        .map(|(id, account)| {
            (
                *id,
                account
                    .positions()
                    .iter()
                    .map(|(code, position)| (code.clone(), u64::from(position.qty())))
                    .collect(),
            )
        })
        .collect();
    assert!(
        matches!(corrupt_actions.validate(&positions, &restored.state.company_system, restored.civil_date()), Err(crate::session::SessionCorporateActionsError::Invalid(message)) if message.contains("到账日期晚于"))
    );
    let saved = restored.save().unwrap();
    let mut paid_restore = GameSession::restore(&saved).unwrap();
    assert_eq!(
        paid_restore.account(AccountId(0)).unwrap().cash(),
        before_payment
    );
    assert_eq!(paid_restore.corporate_actions().dividend_tax_books.len(), 1);
    assert_eq!(
        paid_restore.corporate_actions().dividend_tax_books[0]
            .outstanding_tax()
            .unwrap()
            .numerator(),
        0
    );
    assert_eq!(
        paid_restore
            .corporate_actions()
            .account_gross_receipts
            .len(),
        2
    );
}

#[test]
fn cash_dividend_tax_is_collected_from_actual_disposal_only_and_restores() {
    let (mut session, _issuer, stock) = session_with_approved_cash_dividend();
    let account = crate::orderbook::AccountId(0);
    let initial_cash = session.account(account).unwrap().cash();
    while session.civil_date() <= crate::calendar::CivilDate::from_iso("2030-01-08").unwrap() {
        if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
            session.step().unwrap();
        }
        session.end_civil_day().unwrap();
        if session
            .corporate_actions()
            .account_gross_receipts
            .iter()
            .any(|receipt| receipt.account == account)
        {
            break;
        }
    }
    let cash_after_dividend = session.account(account).unwrap().cash();
    assert!(
        session
            .corporate_actions()
            .account_gross_receipts
            .iter()
            .any(|receipt| receipt.account == account),
        "fixture must reach the actual cash dividend payment date"
    );
    assert_eq!(
        cash_after_dividend,
        initial_cash.add(Money::from_cents(50)).unwrap()
    );
    let tax_book = session
        .corporate_actions()
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == account && book.stock() == &stock)
        .unwrap();
    assert_eq!(tax_book.outstanding_tax().unwrap().numerator(), 0);

    let before_sale = session.corporate_actions().dividend_tax_books.clone();
    let sale_date = session.civil_date();
    let institution = crate::orderbook::AccountId(1);
    session
        .state
        .npc_attention
        .get_mut(&institution)
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    session.state.attention_scheduler = [(u64::MAX, institution)].into_iter().collect();
    let buyer_order_id = session.state.next_order_id;
    let buy_result = session
        .state
        .markets
        .get_mut(&stock)
        .unwrap()
        .place(crate::Order {
            id: crate::OrderId(buyer_order_id),
            side: crate::Side::Buy,
            price: Money::from_cents(1_000),
            qty: 100,
            original_qty: 100,
            filled_qty: 0,
            filled_value: Money::ZERO,
            owner: institution,
            seq: buyer_order_id,
        })
        .unwrap();
    assert!(buy_result.trades.is_empty());
    session.state.next_order_id += 1;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    session
        .enqueue_player_intent(
            account,
            crate::Intent::PlaceLimit {
                code: stock.clone(),
                side: crate::Side::Sell,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 5,
            },
        )
        .unwrap();
    if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
        let events = session.step().unwrap();
        assert!(
            events.iter().any(|event| matches!(event, crate::Event::Trade { .. })),
            "fixture must generate a real five-share disposal; phase={:?}, tick={}, events={events:?}",
            session.phase(),
            session.tick()
        );
    } else {
        panic!(
            "fixture must trade inside intraday; phase={:?}, day={}, tick={}, pending={}, resting={}",
            session.civil_clock().phase(),
            session.day(),
            session.tick(),
            session.state.pending_player.len(),
            session.state.markets[&stock].resting_orders().len()
        );
    }
    let confirmations = session.personal_trade_confirmations(account);
    assert_eq!(
        session.state.pending_player.len(),
        0,
        "player intent must be consumed by the authoritative tick; tick={}, next_order_id={}, ledger={:?}",
        session.tick(),
        session.state.next_order_id,
        session.state.envelope_ledger
    );
    let sale_confirmation = confirmations
        .iter()
        .find(|confirmation| {
            confirmation.civil_date == sale_date
                && confirmation.code == stock
                && confirmation.side == crate::Side::Sell
                && confirmation.quantity_shares == 5
        })
        .cloned()
        .unwrap_or_else(|| {
            panic!(
            "the tax disposal must use a real personal sell confirmation; phase={:?}, confirmations={confirmations:?}, resting={:?}",
            session.civil_clock().phase(),
            session.state.markets[&stock].resting_orders()
        )
        });
    let sale_net = sale_confirmation
        .gross
        .sub(sale_confirmation.actual_fees.total().unwrap())
        .unwrap();
    session.end_civil_day().unwrap();
    let after_sale = session
        .corporate_actions()
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == account && book.stock() == &stock)
        .unwrap();
    assert!(before_sale[0].collections().len() < after_sale.collections().len());
    assert_eq!(
        session
            .corporate_actions()
            .dividend_tax_books
            .iter()
            .find(|book| book.account() == account && book.stock() == &stock)
            .unwrap()
            .lots()
            .iter()
            .map(|lot| lot.qty)
            .sum::<u64>(),
        0,
        "the entire disposed dividend lot must leave the FIFO tax book"
    );
    assert_eq!(
        after_sale.collections().last().unwrap().collected,
        Money::from_cents(10),
        "five held shares use the within-one-month 20% rate on the 50-cent gross dividend"
    );
    assert_eq!(after_sale.outstanding_tax().unwrap().numerator(), 0);
    assert_eq!(
        session.account(account).unwrap().cash(),
        cash_after_dividend
            .add(sale_net)
            .unwrap()
            .sub(Money::from_cents(10))
            .unwrap()
    );

    let save = session.save().unwrap();
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        restored.business_state_hash().unwrap(),
        session.business_state_hash().unwrap()
    );
}

#[test]
fn future_approved_date_cash_dividend_is_rejected_atomically() {
    let session = GameSession::new(simple_setup(), 42).unwrap();
    let before = session.business_state_hash().unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let issuer = crate::company::CompanyId(format!("C-{}", stock.0));
    let date = |value| crate::calendar::CivilDate::from_iso(value).unwrap();
    let plan = crate::company::cash_dividend::CashDividendPlan::new(
        "future-plan".into(),
        issuer,
        stock,
        crate::calendar::CalendarExchange::Sse,
        crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
        date("2030-01-02"),
        date("2030-01-02"),
        date("2030-01-03"),
        date("2030-01-04"),
        date("2030-01-07"),
        Money::from_cents(1),
        Money::from_cents(1),
        session.state.civil_clock.calendar(),
    )
    .unwrap();
    let declaration = crate::company::DividendDeclaration {
        plan_id: "future-plan".into(),
        approved_on: date("2030-01-02"),
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
    for kind in [
        crate::company::CompanyKind::Bank,
        crate::company::CompanyKind::Insurance,
        crate::company::CompanyKind::RealEstate,
    ] {
        let mut setup = simple_setup();
        if let crate::company::config::CompanySystemConfig::Simple(config) =
            &mut setup.company_system
        {
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
        if let crate::company::config::CompanySystemConfig::Simple(config) =
            &mut changed.company_system
        {
            if duplicate {
                config.companies.push(config.companies[0].clone());
            } else {
                config.companies.clear();
            }
        }
        assert!(matches!(
            company_assembly::issuer_specs(&changed),
            Err(SessionError::InvalidSetup(_))
        ));
    }
    let mut unavailable = setup;
    unavailable.company_system = crate::company::config::CompanySystemConfig::Simulation;
    assert!(
        matches!(company_assembly::issuer_specs(&unavailable), Err(SessionError::InvalidSetup(message)) if message.contains("Simulation"))
    );
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
        if let crate::company::config::CompanySystemConfig::Simple(config) =
            &mut setup.company_system
        {
            config.companies[0].kind = kind;
            config.companies[0].finance.opening_lines = vec![
                JournalLine {
                    account: LedgerAccountId(cash_code.into()),
                    side: PostingSide::Debit,
                    amount: AccountingAmount::from_cents(500_000_000),
                },
                JournalLine {
                    account: LedgerAccountId("4001".into()),
                    side: PostingSide::Credit,
                    amount: AccountingAmount::from_cents(500_000_000),
                },
            ];
        }
        let session = GameSession::new(setup, 42).unwrap();
        let save = session.save().unwrap();
        let company = crate::company::CompanyId(format!("C-{}", save.setup.stocks[0].code.0));
        assert_eq!(
            save.company_system.issuers().get(&company).unwrap().kind,
            kind
        );
        let public = save.public_library.save();
        assert!(!public.reports.is_empty());
        assert!(public.reports.iter().all(
            |report| report.company == company && report.policy.chart_version == chart_version
        ));
        let restored = GameSession::restore(&save).unwrap();
        assert_eq!(
            restored.business_state_hash().unwrap(),
            session.business_state_hash().unwrap()
        );
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
    if let crate::company::config::CompanySystemConfig::Simple(config) =
        &mut changed.setup.company_system
    {
        config.environment.noise.monthly_bp += 1;
    }
    assert!(
        matches!(GameSession::restore(&changed), Err(SessionError::InvalidSave(message)) if message.contains("配置"))
    );
    let mut wire = serde_json::to_value(&save).unwrap();
    wire["company_system"]["implementation"]["state"]["advanced_through"] =
        serde_json::json!("2030-01-01");
    let edited: SaveSlot = serde_json::from_value(wire).unwrap();
    assert!(
        matches!(GameSession::restore(&edited), Err(SessionError::InvalidSave(message)) if message.contains("日期"))
    );
}

#[test]
fn simple_restore_rejects_foreign_model_publication_source() {
    let session = GameSession::new(simple_setup(), 42).unwrap();
    let mut save = session.save().unwrap();
    let mut library = save.public_library.save();
    assert!(!library.reports.is_empty());
    library.reports[0].source = crate::information::PublicationSource::SimulationAccounting;
    save.public_library = crate::information::PublicLibrary::from_parts(library).unwrap();
    assert!(
        matches!(GameSession::restore(&save), Err(SessionError::InvalidSave(message)) if message.contains("来源"))
    );
}

#[test]
fn simple_session_save_has_selected_system_without_financial_simulation() {
    let session = GameSession::new(simple_setup(), 42).unwrap();
    let save = serde_json::to_value(session.save().unwrap()).unwrap();
    assert_eq!(save["company_system"]["implementation"]["mode"], "Simple");
    for field in ["company_operations", "closing_registry", "ops_wiring"] {
        assert!(save.get(field).is_none(), "Simple 存档不得携带 {field}");
    }
    assert_eq!(
        save["company_system"]["implementation"]["state"]["advanced_through"],
        "2029-12-31"
    );
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
    assert!(
        matches!(GameSession::new(parsed, 42), Err(SessionError::InvalidSetup(message)) if message.contains("Simulation"))
    );
}

#[test]
fn simple_day_end_advances_selected_state_and_restore_preserves_it() {
    let mut session = GameSession::new(simple_setup(), 42).unwrap();
    session.end_civil_day().unwrap();
    let save = session.save().unwrap();
    let wire = serde_json::to_value(&save).unwrap();
    assert_eq!(
        wire["company_system"]["implementation"]["state"]["advanced_through"],
        "2030-01-01"
    );
    let restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        restored.business_state_hash().unwrap(),
        session.business_state_hash().unwrap()
    );
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
    assert_eq!(
        restored.business_state_hash().unwrap(),
        session.business_state_hash().unwrap()
    );
    assert!(crate::session::protocol::ProtocolSession::restore(&save).is_err());
    for future_second in [37800, 54000, 86399] {
        let mut wire = serde_json::to_value(&save).unwrap();
        wire["disclosures"]["published_through"] =
            serde_json::to_value(CivilInstant::new(session.civil_date(), future_second).unwrap())
                .unwrap();
        let edited: SaveSlot = serde_json::from_value(wire).unwrap();
        assert!(
            matches!(GameSession::restore(&edited), Err(SessionError::InvalidSave(message)) if message.contains("disclosure cursor"))
        );
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
        if let crate::company::config::CompanySystemConfig::Simple(config) =
            &mut setup.company_system
        {
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
            assert_eq!(
                restored.business_state_hash().unwrap(),
                session.business_state_hash().unwrap()
            );
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

fn session_with_approved_stock_distribution(
    mixed_restriction: bool,
) -> (GameSession, crate::company::CompanyId, crate::account::StockCode) {
    use crate::account::Position;
    use crate::accounting::AccountingAmount;
    use crate::company::share_registry::{
        AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
    };
    use crate::company::stock_distribution::{
        StockDistributionEventPlan, StockDistributionKind,
    };
    use crate::orderbook::AccountId;

    let mut setup = simple_setup();
    setup.ticks_per_day = 1;
    setup.start_date = crate::CivilDate::from_iso("2030-01-02").unwrap();
    let mut session = GameSession::new(setup, 42).unwrap();
    let stock = session.state.setup.stocks[0].code.clone();
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&stock)
        .unwrap()
        .clone();
    let date = |value| crate::CivilDate::from_iso(value).unwrap();
    let approved_on = date("2030-01-02");
    let announced_on = date("2030-01-03");
    let registered_on = date("2030-01-04");
    let ex_rights_on = date("2030-01-07");
    let total_shares = session.state.setup.stocks[0].total_shares;
    // 面值 0.01 元：注册资本 = 发行股数 × 1 分，保证整除。
    let capital = AccountingAmount::from_cents(i128::from(total_shares));
    session
        .define_dividend_legal_facts(&issuer, capital, "explicit test legal fact".into())
        .unwrap();
    let unrestricted_lot = |id: &str, qty: u64| ShareLot {
        id: id.into(),
        qty,
        acquired_on: approved_on,
        source: AcquisitionSource::InitialAllocation {
            evidence: "fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    };
    let player_lots = if mixed_restriction {
        vec![
            ShareLot {
                id: "player-restricted".into(),
                qty: 3,
                acquired_on: approved_on,
                source: AcquisitionSource::InitialAllocation {
                    evidence: "fixture".into(),
                },
                restriction: ShareRestriction::Restricted {
                    reason: "nonfloat-lock".into(),
                    release_on: date("2030-08-01"),
                },
            },
            unrestricted_lot("player-free", 3),
        ]
    } else {
        vec![unrestricted_lot("player-lot", 6)]
    };
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(stock.clone(), Position::from_restored_parts(6, 0, 1_000, 0));
    let registry = ShareRegistry::new(
        stock.clone(),
        issuer.clone(),
        total_shares,
        approved_on,
        vec![
            ShareHolding {
                holder: HolderId::Account(AccountId(0)),
                lots: player_lots,
            },
            ShareHolding {
                holder: HolderId::External("external-a".into()),
                lots: vec![unrestricted_lot("external-a-lot", 1)],
            },
            ShareHolding {
                holder: HolderId::External("external-b".into()),
                lots: vec![unrestricted_lot("external-b-lot", 1)],
            },
            ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![unrestricted_lot("treasury-lot", total_shares - 8)],
            },
        ],
    )
    .unwrap();
    session.configure_share_registry(registry).unwrap();
    let plan = StockDistributionEventPlan {
        event_id: "distribution-1".into(),
        approval_reference: "shareholders-resolution-1".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        kind: StockDistributionKind::BonusShares,
        approved_on,
        announced_on,
        registered_on,
        ex_rights_on,
        shares_per_existing_share_micros: 250_000,
        approved_total_new_shares: 2,
    };
    session.approve_stock_distribution(plan).unwrap();
    (session, issuer, stock)
}

#[test]
fn stock_distribution_credits_registry_accounts_issuer_and_ex_rights_anchor() {
    use crate::orderbook::AccountId;

    let (mut session, issuer, stock) = session_with_approved_stock_distribution(false);
    let total_shares = session.state.setup.stocks[0].total_shares;
    // 01-02：批准日无事件推进；01-03：公告；01-04：R 日冻结分配。
    for _ in 0..3 {
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
        session.end_civil_day().unwrap();
    }
    let book = &session.corporate_actions().stock_distributions[0];
    assert_eq!(
        book.status(),
        &crate::company::stock_distribution::StockDistributionStatus::Registered
    );
    let receipt = book.receipt().unwrap();
    assert_eq!(receipt.approved_total_new_shares, 2);
    let player_award = receipt
        .holders
        .iter()
        .find(|holder| holder.holder == crate::company::share_registry::HolderId::Account(AccountId(0)))
        .unwrap()
        .whole_shares;
    assert_eq!(player_award, 2);
    // 周末两个自然日只做日结，不做 tick。
    session.end_civil_day().unwrap();
    session.end_civil_day().unwrap();
    // 01-07（R+1）首个 tick 安装除权参考价：1000 分 / 1.25 = 800 分。
    session.step().unwrap();
    assert_eq!(session.state.markets[&stock].last_close(), Money::from_cents(800));
    let groups = &session.corporate_actions().applied_ex_reference_groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].date, crate::CivilDate::from_iso("2030-01-07").unwrap());
    assert!(groups[0].cash_plan_ids.is_empty());
    assert_eq!(groups[0].stock_event_ids, vec!["distribution-1".to_owned()]);
    assert_eq!(groups[0].reference.reference_price, Money::from_cents(800));
    // R+1 日终：非交易过户入账、投资者持仓加股、发行股数与账面事实同步。
    session.end_civil_day().unwrap();
    let registry = &session.corporate_actions().registries[0];
    assert_eq!(registry.issued_shares(), total_shares + 2);
    let credit_receipt = registry
        .receipt_by_event("stock-distribution:distribution-1")
        .expect("非交易过户回执必须存在");
    assert_eq!(credit_receipt.request.day, crate::CivilDate::from_iso("2030-01-07").unwrap());
    assert!(matches!(
        credit_receipt.request.scope,
        crate::company::share_registry::MovementScope::NonTradingTransfer { .. }
    ));
    assert_eq!(session.account(AccountId(0)).unwrap().positions()[&stock].qty(), 8);
    assert_eq!(session.account(AccountId(0)).unwrap().sellable_qty(&stock), 8);
    assert_eq!(
        session
            .state
            .company_system
            .issuers()
            .get(&issuer)
            .unwrap()
            .issued_shares,
        total_shares + 2
    );
    let book = &session.corporate_actions().stock_distributions[0];
    assert_eq!(
        book.status(),
        &crate::company::stock_distribution::StockDistributionStatus::Credited
    );
    assert_eq!(book.credited_on(), Some(crate::CivilDate::from_iso("2030-01-07").unwrap()));
    let fact = session
        .state
        .company_system
        .stock_distribution_facts(&issuer)
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "distribution-1")
        .unwrap();
    assert_eq!(fact.credited_on, Some(crate::CivilDate::from_iso("2030-01-07").unwrap()));
    assert_eq!(fact.capital_increase, crate::accounting::AccountingAmount::from_cents(2));
    // 严格恢复后继续一天不重复入账，锚点事实保持。
    let save = session.save().unwrap();
    let mut restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        restored.business_state_hash().unwrap(),
        session.business_state_hash().unwrap()
    );
    for _ in 0..restored.state.setup.ticks_per_day {
        restored.step().unwrap();
    }
    restored.end_civil_day().unwrap();
    assert_eq!(restored.corporate_actions().registries[0].issued_shares(), total_shares + 2);
    assert_eq!(restored.account(AccountId(0)).unwrap().positions()[&stock].qty(), 8);
    assert_eq!(restored.corporate_actions().registries.len(), 1);
}

#[test]
fn stock_distribution_delivery_rolls_back_whole_day_on_mixed_restriction_sources() {
    use crate::orderbook::AccountId;

    let (mut session, _issuer, stock) = session_with_approved_stock_distribution(true);
    let total_shares = session.state.setup.stocks[0].total_shares;
    for _ in 0..3 {
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
        session.end_civil_day().unwrap();
    }
    session.end_civil_day().unwrap();
    session.end_civil_day().unwrap();
    session.step().unwrap();
    let before_failure = session.business_state_hash().unwrap();
    let failure = session.end_civil_day().unwrap_err();
    assert!(
        failure.to_string().contains("mixes restriction sources"),
        "失败必须显式指向混合限售来源：{failure}"
    );
    assert_eq!(session.business_state_hash().unwrap(), before_failure);
    let registry = &session.corporate_actions().registries[0];
    assert_eq!(registry.issued_shares(), total_shares);
    assert!(registry
        .receipt_by_event("stock-distribution:distribution-1")
        .is_none());
    assert_eq!(session.account(AccountId(0)).unwrap().positions()[&stock].qty(), 6);
    assert_eq!(
        session.corporate_actions().stock_distributions[0].status(),
        &crate::company::stock_distribution::StockDistributionStatus::Registered
    );
}

/// 推进 `days` 个自然日（每交易日先走满 tick），返回推进后的会话。
fn advance_civil_days(session: &mut GameSession, days: u32) {
    for _ in 0..days {
        for _ in 0..session.state.setup.ticks_per_day {
            session.step().unwrap();
        }
        session.end_civil_day().unwrap();
    }
}

fn account_position_projection(
    session: &GameSession,
) -> std::collections::BTreeMap<
    crate::orderbook::AccountId,
    std::collections::BTreeMap<crate::account::StockCode, u64>,
> {
    session
        .state
        .accounts
        .iter()
        .map(|(id, account)| {
            (
                *id,
                account
                    .positions()
                    .iter()
                    .map(|(code, position)| (code.clone(), u64::from(position.qty())))
                    .collect(),
            )
        })
        .collect()
}

#[test]
fn consecutive_stock_distributions_keep_par_constant_and_evolve_registered_capital() {
    use crate::company::stock_distribution::{
        StockDistributionEventPlan, StockDistributionKind,
    };
    use crate::orderbook::AccountId;

    let (mut session, issuer, stock) = session_with_approved_stock_distribution(false);
    let total_shares = session.state.setup.stocks[0].total_shares;
    let date = |value| crate::CivilDate::from_iso(value).unwrap();
    // 01-02 批准、01-03 公告、01-04 R 日登记；周末两日后 01-07 R+1 入账。
    advance_civil_days(&mut session, 3);
    session.end_civil_day().unwrap();
    session.end_civil_day().unwrap();
    session.step().unwrap();
    session.end_civil_day().unwrap();
    let fact1 = session
        .state
        .company_system
        .stock_distribution_facts(&issuer)
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "distribution-1")
        .unwrap();
    assert_eq!(fact1.par_value_per_share, Money::from_cents(1));
    assert_eq!(
        fact1.registered_capital_at_approval,
        crate::accounting::AccountingAmount::from_cents(i128::from(total_shares))
    );
    // 入账演进后注册资本 = 初始 + 面值 × 新增股数。
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital,
        crate::accounting::AccountingAmount::from_cents(i128::from(total_shares) + 2)
    );

    // 第二次 10 送 3：入账后合格股份 10 × 0.3 = 3 股。旧实现用
    // 「注册资本 ÷ 增大后的发行股数」重推面值，T/(T+2) 不整除会永久锁死第二次送转。
    let second = StockDistributionEventPlan {
        event_id: "distribution-2".into(),
        approval_reference: "shareholders-resolution-2".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        kind: StockDistributionKind::BonusShares,
        approved_on: date("2030-01-08"),
        announced_on: date("2030-01-09"),
        registered_on: date("2030-01-10"),
        ex_rights_on: date("2030-01-11"),
        shares_per_existing_share_micros: 300_000,
        approved_total_new_shares: 3,
    };
    session.approve_stock_distribution(second).unwrap();
    let fact2 = session
        .state
        .company_system
        .stock_distribution_facts(&issuer)
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "distribution-2")
        .unwrap();
    // 面值恒定；第二次声明的批准时点注册资本按演进后事实冻结。
    assert_eq!(fact2.par_value_per_share, fact1.par_value_per_share);
    assert_eq!(
        fact2.registered_capital_at_approval,
        crate::accounting::AccountingAmount::from_cents(i128::from(total_shares) + 2)
    );

    // 走完第二次全链路：公告 → R 日登记 → R+1 入账。
    advance_civil_days(&mut session, 3);
    session.step().unwrap();
    session.end_civil_day().unwrap();
    let registry = &session.corporate_actions().registries[0];
    assert_eq!(registry.issued_shares(), total_shares + 5);
    assert_eq!(session.account(AccountId(0)).unwrap().positions()[&stock].qty(), 11);
    assert_eq!(
        session
            .state
            .company_system
            .issuers()
            .get(&issuer)
            .unwrap()
            .issued_shares,
        total_shares + 5
    );
    assert_eq!(
        session
            .state
            .company_system
            .dividend_legal_facts(&issuer)
            .unwrap()
            .unwrap()
            .registered_capital,
        crate::accounting::AccountingAmount::from_cents(i128::from(total_shares) + 5)
    );
    let fact2 = session
        .state
        .company_system
        .stock_distribution_facts(&issuer)
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "distribution-2")
        .unwrap();
    assert_eq!(
        fact2.credited_on,
        Some(crate::CivilDate::from_iso("2030-01-11").unwrap())
    );

    // 可分配利润上限按演进后事实核定：面值总额明显超过上限的第三次送股被显式拒绝。
    let oversized = StockDistributionEventPlan {
        event_id: "distribution-oversized".into(),
        approval_reference: "shareholders-resolution-3".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        kind: StockDistributionKind::BonusShares,
        approved_on: date("2030-01-12"),
        announced_on: date("2030-01-13"),
        registered_on: date("2030-01-14"),
        ex_rights_on: date("2030-01-15"),
        shares_per_existing_share_micros: 300_000,
        approved_total_new_shares: 1_000_000_000,
    };
    let rejection = session.approve_stock_distribution(oversized).unwrap_err();
    assert!(
        rejection.to_string().contains("送股面值总额"),
        "超上限送股必须因可分配利润上限被拒：{rejection}"
    );
    assert_eq!(session.corporate_actions().stock_distributions.len(), 2);
}

#[test]
fn second_stock_distribution_on_same_ex_rights_day_is_rejected_at_approval() {
    use crate::company::stock_distribution::{
        StockDistributionEventPlan, StockDistributionKind,
    };

    let (mut session, _issuer, stock) = session_with_approved_stock_distribution(false);
    let date = |value| crate::CivilDate::from_iso(value).unwrap();
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&stock)
        .unwrap()
        .clone();
    let same_day = StockDistributionEventPlan {
        event_id: "distribution-same-day".into(),
        approval_reference: "shareholders-resolution-2".into(),
        issuer,
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        kind: StockDistributionKind::BonusShares,
        approved_on: date("2030-01-02"),
        announced_on: date("2030-01-03"),
        registered_on: date("2030-01-04"),
        ex_rights_on: date("2030-01-07"),
        shares_per_existing_share_micros: 100_000,
        approved_total_new_shares: 1,
    };
    // 同 issuer/stock 同 ex_rights_on 的第二起事件必须在受理时被拒，
    // 不能推迟到 R+1 首 tick 的除权准备才 StepFatal。
    let rejection = session.approve_stock_distribution(same_day).unwrap_err();
    assert!(
        rejection.to_string().contains("同日多起送转"),
        "同日第二起送转必须在受理时显式拒绝：{rejection}"
    );
    assert_eq!(session.corporate_actions().stock_distributions.len(), 1);
}

#[test]
fn validate_rejects_nontrading_receipt_without_credited_stock_distribution_book() {
    use crate::company::share_registry::{
        AcquisitionSource, DayNetChange, MovementScope, NetAcquisition, ShareDayRequest,
        ShareRestriction,
    };

    let (mut session, issuer, _stock) = session_with_approved_stock_distribution(false);
    advance_civil_days(&mut session, 3);
    session.end_civil_day().unwrap();
    session.end_civil_day().unwrap();
    session.step().unwrap();
    session.end_civil_day().unwrap();
    let day = crate::CivilDate::from_iso("2030-01-07").unwrap();
    // 篡改注入：同日追加一笔没有对应送转账簿的非交易过户回执，并同步发行股数，
    // 使其只违反「名册回执 → 已入账送转账簿」的反向勾稽方向。
    session.state.corporate_actions.registries[0]
        .close_day(ShareDayRequest {
            event_id: "stock-distribution:ghost-event".into(),
            day,
            scope: MovementScope::NonTradingTransfer {
                basis: "ghost-resolution".into(),
            },
            changes: vec![DayNetChange {
                holder: crate::company::share_registry::HolderId::External("ghost".into()),
                change: 1,
                acquisition: Some(NetAcquisition {
                    lot_id: "stock-distribution:ghost-event:external-ghost".into(),
                    source: AcquisitionSource::CorporateAction {
                        event: "ghost-event".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }),
            }],
        })
        .unwrap();
    std::sync::Arc::make_mut(&mut session.state.company_system)
        .issuers
        .record_share_issuance(&issuer, 1)
        .unwrap();
    let positions = account_position_projection(&session);
    let rejection = session
        .state
        .corporate_actions
        .validate(&positions, &session.state.company_system, day)
        .unwrap_err();
    assert!(
        rejection.to_string().contains("非交易过户回执"),
        "无对应已入账送转账簿的回执必须被反向勾稽拒绝：{rejection}"
    );
}

#[test]
fn validate_fails_when_registered_book_missed_ex_rights_credit_date() {
    let (mut session, _issuer, _stock) = session_with_approved_stock_distribution(false);
    // 推进到 R 日登记完成（01-04 日终，状态 Registered）。
    advance_civil_days(&mut session, 3);
    let positions = account_position_projection(&session);
    // 恢复校验在已过入账日的日期上不得接受仍未入账的 Registered 账簿。
    let rejection = session
        .state
        .corporate_actions
        .validate(
            &positions,
            &session.state.company_system,
            crate::CivilDate::from_iso("2030-01-08").unwrap(),
        )
        .unwrap_err();
    assert!(
        rejection.to_string().contains("错过入账日"),
        "已过入账日仍 Registered 的送转账簿必须显式失败：{rejection}"
    );
}

#[test]
fn day_end_processing_fails_when_registered_book_missed_ex_rights_date() {
    let (mut session, _issuer, _stock) = session_with_approved_stock_distribution(false);
    advance_civil_days(&mut session, 3);
    // 直接以已过入账日的日期执行送转日结：必须显式失败而不是静默跳过。
    let failure = session
        .state
        .corporate_actions
        .process_stock_distributions_on_day_end(
            crate::CivilDate::from_iso("2030-01-08").unwrap(),
            session.state.civil_clock.calendar(),
            std::sync::Arc::make_mut(&mut session.state.company_system),
            &mut session.state.accounts,
        )
        .unwrap_err();
    assert!(
        failure.to_string().contains("错过入账日"),
        "已过入账日仍 Registered 的送转事件在日结处理中必须显式失败：{failure}"
    );
}

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
    session_with_approved_cash_dividend_and_player_lots(vec![
        crate::company::share_registry::ShareLot {
            id: "player-lot".into(),
            qty: 5,
            acquired_on: crate::CivilDate::from_iso("2030-01-02").unwrap(),
            source: crate::company::share_registry::AcquisitionSource::InitialAllocation {
                evidence: "fixture".into(),
            },
            restriction: crate::company::share_registry::ShareRestriction::Unrestricted,
        },
    ])
}

fn session_with_approved_cash_dividend_and_player_lots(
    player_lots: Vec<crate::company::share_registry::ShareLot>,
) -> (
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
    let player_qty: u64 = player_lots.iter().map(|lot| lot.qty).sum();
    session
        .state
        .accounts
        .get_mut(&AccountId(0))
        .unwrap()
        .fixture_insert_position(
            stock.clone(),
            Position::from_restored_parts(player_qty.try_into().unwrap(), 0, 1_000, 0),
        );
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
                lots: player_lots,
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
                    qty: total_shares - player_qty - 2,
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
        u32::try_from(player_qty).unwrap(),
        "fixture must keep the dividend lot sellable on payment date"
    );
    session
        .configure_cash_dividend_tax_book(
            AccountId(0),
            stock.clone(),
            crate::company::cash_dividend_tax::DividendTaxProfile::IndividualPublicMarket,
        )
        .unwrap();
    let eligible_gross_cents = 10_i64 * i64::try_from(player_qty + 2).unwrap();
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
        Money::from_cents(eligible_gross_cents),
        session.state.civil_clock.calendar(),
    )
    .unwrap();
    session
        .approve_cash_dividend(
            crate::company::DividendDeclaration {
                plan_id: plan.plan_id.clone(),
                approved_on,
                total_gross: AccountingAmount::from_cents(i128::from(eligible_gross_cents)),
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
        .applied_ex_dividend_groups
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
    // m5：仅付款日照常留一张零税回执，安静日不再逐日落零税回执。
    assert_eq!(tax_book.collections().len(), 1);
    assert_eq!(
        tax_book.collections()[0].day,
        crate::CivilDate::from_iso("2030-01-08").unwrap()
    );
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

/// 推进会话直到玩家真实收到现金分红到账回执（到 2030-01-08 付款日）。
fn advance_until_player_dividend_paid(
    session: &mut GameSession,
    account: crate::orderbook::AccountId,
) {
    while session.civil_date() <= crate::CivilDate::from_iso("2030-01-08").unwrap() {
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
            return;
        }
    }
    panic!(
        "fixture must reach the dividend payment date by 2030-01-08; current date {}",
        session.civil_date().to_iso()
    );
}

/// 在当前交易日内用真实限价单卖出玩家股份，返回扣除真实费用后的卖出净额。
fn sell_player_shares_today(
    session: &mut GameSession,
    account: crate::orderbook::AccountId,
    stock: &crate::account::StockCode,
    qty: u32,
) -> Money {
    let institution = crate::orderbook::AccountId(1);
    session
        .state
        .npc_attention
        .get_mut(&institution)
        .unwrap()
        .next_attention_candidate_tick = u64::MAX;
    session.state.attention_scheduler = [(u64::MAX, institution)].into_iter().collect();
    let buyer_order_id = session.state.next_order_id;
    session
        .state
        .markets
        .get_mut(stock)
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
    session.state.next_order_id += 1;
    session.hydrate_or_validate_envelope_ledger().unwrap();
    session
        .enqueue_player_intent(
            account,
            crate::Intent::PlaceLimit {
                code: stock.clone(),
                side: crate::Side::Sell,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty,
            },
        )
        .unwrap();
    assert!(
        matches!(
            session.civil_clock().phase(),
            crate::session::CivilPhase::IntradayTrading
        ),
        "sale fixture must trade inside intraday; phase={:?}, day={}, tick={}",
        session.phase(),
        session.day(),
        session.tick()
    );
    let events = session.step().unwrap();
    assert!(
        events
            .iter()
            .any(|event| matches!(event, crate::Event::Trade { .. })),
        "fixture must generate a real {qty}-share disposal; events={events:?}"
    );
    assert_eq!(
        session.state.pending_player.len(),
        0,
        "player intent must be consumed by the authoritative tick"
    );
    let sale_date = session.civil_date();
    let confirmation = session
        .personal_trade_confirmations(account)
        .iter()
        .find(|confirmation| {
            confirmation.civil_date == sale_date
                && confirmation.code == *stock
                && confirmation.side == crate::Side::Sell
                && confirmation.quantity_shares == qty
        })
        .cloned()
        .unwrap_or_else(|| panic!("the tax disposal must use a real personal sell confirmation"));
    confirmation
        .gross
        .sub(confirmation.actual_fees.total().unwrap())
        .unwrap()
}

fn player_tax_book<'a>(
    session: &'a GameSession,
    account: crate::orderbook::AccountId,
    stock: &crate::account::StockCode,
) -> &'a crate::company::cash_dividend_tax::CashDividendTaxBook {
    session
        .corporate_actions()
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == account && book.stock() == stock)
        .expect("player tax book must exist")
}

#[test]
fn insufficient_cash_partial_collection_and_next_day_end_chase() {
    use crate::company::share_registry::{AcquisitionSource, ShareLot, ShareRestriction};
    use crate::orderbook::AccountId;
    // 玩家 105 股单一批次：第一日整手卖出 100 股，第二日零股一次性卖出剩余 5 股。
    let player_lot = ShareLot {
        id: "player-lot".into(),
        qty: 105,
        acquired_on: crate::CivilDate::from_iso("2030-01-02").unwrap(),
        source: AcquisitionSource::InitialAllocation {
            evidence: "fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    };
    let (mut session, _issuer, stock) =
        session_with_approved_cash_dividend_and_player_lots(vec![player_lot]);
    let account = AccountId(0);
    let initial_cash = session.account(account).unwrap().cash();
    advance_until_player_dividend_paid(&mut session, account);
    let cash_after_dividend = session.account(account).unwrap().cash();
    assert_eq!(
        cash_after_dividend,
        initial_cash.add(Money::from_cents(1_050)).unwrap()
    );
    assert_eq!(
        session.civil_date(),
        crate::CivilDate::from_iso("2030-01-09").unwrap()
    );

    // 第一日真实卖出 100 股：应纳 10×100×20% = 200 分，现金充足全额划收，形成历史收缴。
    let _sale_net_day1 = sell_player_shares_today(&mut session, account, &stock, 100);
    session.end_civil_day().unwrap();
    let after_first_sale = player_tax_book(&session, account, &stock);
    assert_eq!(
        after_first_sale.collections().last().unwrap().collected,
        Money::from_cents(200)
    );
    assert_eq!(after_first_sale.outstanding_tax().unwrap().numerator(), 0);

    // 第二日真实卖出剩余 5 股：应纳 10×5×20% = 10 分，但账户真实现金只有 3 分。
    // 不得把历史已收缴税额加回可用现金；应按 min(due, cash)=3 部分收缴并继续追缴。
    let _sale_net_day2 = sell_player_shares_today(&mut session, account, &stock, 5);
    session
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_set_cash(Money::from_cents(3));
    session.end_civil_day().unwrap();
    let after_partial = player_tax_book(&session, account, &stock);
    let partial_receipt = after_partial.collections().last().unwrap();
    assert_eq!(partial_receipt.collected, Money::from_cents(3));
    assert_eq!(
        partial_receipt.available_cash,
        Money::from_cents(3),
        "收缴上限必须使用账户真实现金，不得叠加历史已收缴税额"
    );
    assert!(partial_receipt.needs_funds);
    assert_eq!(
        after_partial.outstanding_tax().unwrap(),
        crate::company::cash_dividend_tax::ExactDividendTaxAmount::new(7, 1).unwrap()
    );
    assert_eq!(session.account(account).unwrap().cash(), Money::ZERO);

    // 查询面显式暴露未划收税额与资金不足原因。
    let mut views = session.dividend_tax_outstanding_views().unwrap();
    assert_eq!(views.len(), 1);
    let view = views.remove(0);
    assert_eq!(view.account, account);
    assert_eq!(view.stock, stock);
    assert_eq!(
        view.outstanding.numerator,
        crate::company::cash_dividend_tax::ExactDividendTaxAmount::new(7, 1)
            .unwrap()
            .numerator()
    );
    assert!(view.needs_funds);
    assert_eq!(
        view.cause,
        crate::session::corporate_actions::DividendTaxOutstandingCause::InsufficientAvailableCash
    );

    // 次日日终继续追缴：补足资金后划收剩余 7 分。
    session
        .state
        .accounts
        .get_mut(&account)
        .unwrap()
        .fixture_set_cash(Money::from_cents(20));
    session.step().unwrap();
    session.end_civil_day().unwrap();
    let after_chase = player_tax_book(&session, account, &stock);
    assert_eq!(
        after_chase.collections().last().unwrap().collected,
        Money::from_cents(7)
    );
    assert_eq!(after_chase.outstanding_tax().unwrap().numerator(), 0);
    assert_eq!(
        session.account(account).unwrap().cash(),
        Money::from_cents(13)
    );
    let views = session.dividend_tax_outstanding_views().unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(
        views[0].cause,
        crate::session::corporate_actions::DividendTaxOutstandingCause::Cleared
    );
    assert!(!views[0].needs_funds);
}

#[test]
fn cross_tier_dispositions_apply_statutory_rates_per_lot() {
    use crate::company::share_registry::{AcquisitionSource, ShareLot, ShareRestriction};
    use crate::orderbook::AccountId;
    let date = |value| crate::CivilDate::from_iso(value).unwrap();
    let lot = |id: &str, qty: u64, acquired_on: &'static str| ShareLot {
        id: id.into(),
        qty,
        acquired_on: date(acquired_on),
        source: AcquisitionSource::InitialAllocation {
            evidence: "fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    };
    // 2030-01-09 卖出全部 5 股：满年次日(2029-01-08 取得)→0%、
    // 恰满月次日(2029-12-08 取得)→10%、恰满月当日(2029-12-09 取得)→20%。
    let (mut session, _issuer, stock) = session_with_approved_cash_dividend_and_player_lots(vec![
        lot("year-lot", 1, "2029-01-08"),
        lot("month-lot", 2, "2029-12-08"),
        lot("boundary-lot", 2, "2029-12-09"),
    ]);
    let account = AccountId(0);
    advance_until_player_dividend_paid(&mut session, account);
    assert_eq!(
        session.civil_date(),
        crate::CivilDate::from_iso("2030-01-09").unwrap()
    );
    let cash_after_dividend = session.account(account).unwrap().cash();
    let sale_net = sell_player_shares_today(&mut session, account, &stock, 5);
    session.end_civil_day().unwrap();
    let tax_book = player_tax_book(&session, account, &stock);
    // 应纳 = 10×1×0% + 10×2×10% + 10×2×20% = 6 分。
    assert_eq!(
        tax_book.collections().last().unwrap().collected,
        Money::from_cents(6),
        "per-lot statutory rates must produce 0% + 10% + 20% mix"
    );
    assert_eq!(tax_book.outstanding_tax().unwrap().numerator(), 0);
    assert_eq!(
        session.account(account).unwrap().cash(),
        cash_after_dividend
            .add(sale_net)
            .unwrap()
            .sub(Money::from_cents(6))
            .unwrap()
    );
}

#[test]
fn restored_session_still_collects_tax_on_real_sale() {
    use crate::orderbook::AccountId;
    let (mut session, _issuer, stock) = session_with_approved_cash_dividend();
    let account = AccountId(0);
    advance_until_player_dividend_paid(&mut session, account);
    let save = session.save().unwrap();
    let mut restored = GameSession::restore(&save).unwrap();
    assert_eq!(
        restored.business_state_hash().unwrap(),
        session.business_state_hash().unwrap()
    );
    let cash_after_restore = restored.account(account).unwrap().cash();
    let sale_net = sell_player_shares_today(&mut restored, account, &stock, 5);
    restored.end_civil_day().unwrap();
    let tax_book = player_tax_book(&restored, account, &stock);
    assert_eq!(
        tax_book.collections().last().unwrap().collected,
        Money::from_cents(10),
        "restored session must collect the 20% within-one-month tax on a real sale"
    );
    assert_eq!(tax_book.outstanding_tax().unwrap().numerator(), 0);
    assert_eq!(
        restored.account(account).unwrap().cash(),
        cash_after_restore
            .add(sale_net)
            .unwrap()
            .sub(Money::from_cents(10))
            .unwrap()
    );
    let resaved = restored.save().unwrap();
    let replayed = GameSession::restore(&resaved).unwrap();
    assert_eq!(
        replayed.business_state_hash().unwrap(),
        restored.business_state_hash().unwrap()
    );
}

#[test]
fn month_end_acquired_lot_sells_with_clamped_boundary_rates() {
    use crate::company::share_registry::{AcquisitionSource, ShareLot, ShareRestriction};
    use crate::orderbook::AccountId;
    let date = |value| crate::CivilDate::from_iso(value).unwrap();
    // 8月31日取得：一个月边界按《民法典》期间规则钳制到 9月30日，
    // 2030-01-09 卖出时处于 1 个月以上至 1 年（含）档 → 10%。
    let lot = ShareLot {
        id: "month-end-lot".into(),
        qty: 5,
        acquired_on: date("2029-08-31"),
        source: AcquisitionSource::InitialAllocation {
            evidence: "fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    };
    let (mut session, _issuer, stock) =
        session_with_approved_cash_dividend_and_player_lots(vec![lot]);
    let account = AccountId(0);
    advance_until_player_dividend_paid(&mut session, account);
    assert_eq!(
        session.civil_date(),
        crate::CivilDate::from_iso("2030-01-09").unwrap()
    );
    let cash_after_dividend = session.account(account).unwrap().cash();
    let sale_net = sell_player_shares_today(&mut session, account, &stock, 5);
    session.end_civil_day().unwrap();
    let tax_book = player_tax_book(&session, account, &stock);
    assert_eq!(
        tax_book.collections().last().unwrap().collected,
        Money::from_cents(5),
        "month-end acquired lot must be taxed at the 10% tier, not stall day end"
    );
    assert_eq!(tax_book.outstanding_tax().unwrap().numerator(), 0);
    assert_eq!(
        session.account(account).unwrap().cash(),
        cash_after_dividend
            .add(sale_net)
            .unwrap()
            .sub(Money::from_cents(5))
            .unwrap()
    );
}

#[test]
fn quiet_days_produce_no_zero_tax_collection_receipts() {
    use crate::orderbook::AccountId;
    let (mut session, _issuer, _stock) = session_with_approved_cash_dividend();
    let account = AccountId(0);
    // 付款日之后连续 7 个自然日无新付款、无处置：不得逐日落零税回执。
    advance_until_player_dividend_paid(&mut session, account);
    let receipts_at_payment = player_tax_book(&session, account, &_stock)
        .collections()
        .len();
    for _ in 0..7 {
        if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
            session.step().unwrap();
        }
        session.end_civil_day().unwrap();
    }
    let tax_book = player_tax_book(&session, account, &_stock);
    assert_eq!(
        tax_book.collections().len(),
        receipts_at_payment,
        "quiet days without payments or dispositions must not append zero-tax receipts"
    );
    assert!(tax_book
        .collections()
        .iter()
        .all(|receipt| !receipt.needs_funds));
    // 有付款事件的付款日仍照常留痕。
    assert_eq!(receipts_at_payment, 1);
    assert_eq!(
        tax_book.collections()[0].day,
        crate::CivilDate::from_iso("2030-01-08").unwrap()
    );
}

#[test]
fn tax_book_configuration_is_rejected_after_registry_history() {
    use crate::orderbook::AccountId;
    let (mut session, _issuer, stock) = session_with_approved_cash_dividend();
    for _ in 0..2 {
        if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
            session.step().unwrap();
        }
        session.end_civil_day().unwrap();
    }
    let error = session
        .configure_cash_dividend_tax_book(
            AccountId(1),
            stock.clone(),
            crate::company::cash_dividend_tax::DividendTaxProfile::IndividualPublicMarket,
        )
        .unwrap_err();
    assert!(
        error.to_string().contains("装配期"),
        "late configuration must be rejected with an assembly-time explanation: {error}"
    );
    // 推进到登记日之后：名册同时有历史日结回执与已登记分红，同样拒绝。
    for _ in 0..2 {
        if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
            session.step().unwrap();
        }
        session.end_civil_day().unwrap();
    }
    assert!(session
        .configure_cash_dividend_tax_book(
            AccountId(1),
            stock.clone(),
            crate::company::cash_dividend_tax::DividendTaxProfile::IndividualPublicMarket,
        )
        .is_err());
    assert!(session
        .corporate_actions()
        .dividend_tax_books
        .iter()
        .all(|book| book.account() != AccountId(1)));
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

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
        matches!(corrupt_actions.validate(&positions, &restored.state.company_system, restored.civil_date(), None), Err(crate::session::SessionCorporateActionsError::Invalid(message)) if message.contains("到账日期晚于"))
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

fn session_with_approved_stock_distribution(
    mixed_restriction: bool,
) -> (
    GameSession,
    crate::company::CompanyId,
    crate::account::StockCode,
) {
    use crate::account::Position;
    use crate::accounting::AccountingAmount;
    use crate::company::share_registry::{
        AcquisitionSource, HolderId, ShareHolding, ShareLot, ShareRegistry, ShareRestriction,
    };
    use crate::company::stock_distribution::{StockDistributionEventPlan, StockDistributionKind};
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
        .find(|holder| {
            holder.holder == crate::company::share_registry::HolderId::Account(AccountId(0))
        })
        .unwrap()
        .whole_shares;
    assert_eq!(player_award, 2);
    // 周末两个自然日只做日结，不做 tick。
    session.end_civil_day().unwrap();
    session.end_civil_day().unwrap();
    // 01-07（R+1）首个 tick 安装除权参考价：1000 分 / 1.25 = 800 分。
    session.step().unwrap();
    assert_eq!(
        session.state.markets[&stock].last_close(),
        Money::from_cents(800)
    );
    let groups = &session.corporate_actions().applied_ex_reference_groups;
    assert_eq!(groups.len(), 1);
    assert_eq!(
        groups[0].date,
        crate::CivilDate::from_iso("2030-01-07").unwrap()
    );
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
    assert_eq!(
        credit_receipt.request.day,
        crate::CivilDate::from_iso("2030-01-07").unwrap()
    );
    assert!(matches!(
        credit_receipt.request.scope,
        crate::company::share_registry::MovementScope::NonTradingTransfer { .. }
    ));
    assert_eq!(
        session.account(AccountId(0)).unwrap().positions()[&stock].qty(),
        8
    );
    assert_eq!(
        session.account(AccountId(0)).unwrap().sellable_qty(&stock),
        8
    );
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
    assert_eq!(
        book.credited_on(),
        Some(crate::CivilDate::from_iso("2030-01-07").unwrap())
    );
    let fact = session
        .state
        .company_system
        .stock_distribution_facts(&issuer)
        .unwrap()
        .into_iter()
        .find(|fact| fact.event_id == "distribution-1")
        .unwrap();
    assert_eq!(
        fact.credited_on,
        Some(crate::CivilDate::from_iso("2030-01-07").unwrap())
    );
    assert_eq!(
        fact.capital_increase,
        crate::accounting::AccountingAmount::from_cents(2)
    );
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
    assert_eq!(
        restored.corporate_actions().registries[0].issued_shares(),
        total_shares + 2
    );
    assert_eq!(
        restored.account(AccountId(0)).unwrap().positions()[&stock].qty(),
        8
    );
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
    assert_eq!(
        session.account(AccountId(0)).unwrap().positions()[&stock].qty(),
        6
    );
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
    use crate::company::stock_distribution::{StockDistributionEventPlan, StockDistributionKind};
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
    assert_eq!(
        session.account(AccountId(0)).unwrap().positions()[&stock].qty(),
        11
    );
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
    use crate::company::stock_distribution::{StockDistributionEventPlan, StockDistributionKind};

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
        .validate(&positions, &session.state.company_system, day, None)
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
            None,
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

/// 集成修复轮（送转×税账）共用 fixture：默认 `IndividualPublicMarket` 模式装配
/// 完整名册，玩家持 `player_lots`（自动开个人税账），机构 NPC(1) 作潜在买方，
/// 另含两个具名 External 与 Treasury。送转方案 10送3、获批 3 股全部归玩家
/// （外部股东每户 0.3 股不足整股，获批数恰等于逐户 floor 合计，无碎股补整）。
fn session_with_distribution_tax_interaction(
    player_lots: Vec<crate::company::share_registry::ShareLot>,
    player_qty: u64,
    plan_dates: (&str, &str, &str, &str),
    ticks_per_day: u64,
    freeze_npc: bool,
) -> (
    GameSession,
    crate::company::CompanyId,
    crate::account::StockCode,
) {
    use crate::accounting::AccountingAmount;
    use crate::company::share_registry::{HolderId, ShareHolding, ShareRegistry, ShareRestriction};
    use crate::company::stock_distribution::{StockDistributionEventPlan, StockDistributionKind};

    let mut setup = simple_setup();
    setup.ticks_per_day = ticks_per_day;
    setup.npcs.inst_count = 1;
    // 默认大 A 个人差别化：装配名册即自动为玩家开个人税账。
    setup.dividend_tax_mode =
        crate::company::cash_dividend_tax::CashDividendTaxMode::IndividualPublicMarket;
    setup.start_date = crate::CivilDate::from_iso(plan_dates.0).unwrap();
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
    let approved_on = date(plan_dates.0);
    let total_shares = session.state.setup.stocks[0].total_shares;
    // 面值 0.01 元：注册资本 = 发行股数 × 1 分，保证整除。
    let capital = AccountingAmount::from_cents(i128::from(total_shares));
    session
        .define_dividend_legal_facts(&issuer, capital, "explicit test legal fact".into())
        .unwrap();
    session
        .state
        .accounts
        .get_mut(&crate::orderbook::AccountId(0))
        .unwrap()
        .fixture_insert_position(
            stock.clone(),
            crate::account::Position::from_restored_parts(
                u32::try_from(player_qty).unwrap(),
                0,
                1_000,
                0,
            ),
        );
    let unrestricted = |id: &str, qty: u64| crate::company::share_registry::ShareLot {
        id: id.into(),
        qty,
        acquired_on: approved_on,
        source: crate::company::share_registry::AcquisitionSource::InitialAllocation {
            evidence: "fixture".into(),
        },
        restriction: ShareRestriction::Unrestricted,
    };
    let registry = ShareRegistry::new(
        stock.clone(),
        issuer.clone(),
        total_shares,
        approved_on,
        vec![
            ShareHolding {
                holder: HolderId::Account(crate::orderbook::AccountId(0)),
                lots: player_lots,
            },
            ShareHolding {
                holder: HolderId::External("external-a".into()),
                lots: vec![unrestricted("external-a-lot", 1)],
            },
            ShareHolding {
                holder: HolderId::External("external-b".into()),
                lots: vec![unrestricted("external-b-lot", 1)],
            },
            ShareHolding {
                holder: HolderId::IssuerTreasury,
                lots: vec![unrestricted("treasury-lot", total_shares - player_qty - 2)],
            },
        ],
    )
    .unwrap();
    // 默认大 A 模式：登记成功即自动为玩家开个人税账。
    session.configure_share_registry(registry).unwrap();
    if freeze_npc {
        let institution = crate::orderbook::AccountId(1);
        session
            .state
            .npc_attention
            .get_mut(&institution)
            .unwrap()
            .next_attention_candidate_tick = u64::MAX;
        session.state.attention_scheduler = [(u64::MAX, institution)].into_iter().collect();
    }
    let plan = StockDistributionEventPlan {
        event_id: "distribution-1".into(),
        approval_reference: "shareholders-resolution-1".into(),
        issuer: issuer.clone(),
        stock: stock.clone(),
        exchange: crate::calendar::CalendarExchange::Sse,
        kind: StockDistributionKind::BonusShares,
        approved_on,
        announced_on: date(plan_dates.1),
        registered_on: date(plan_dates.2),
        ex_rights_on: date(plan_dates.3),
        shares_per_existing_share_micros: 300_000,
        approved_total_new_shares: 3,
    };
    session.approve_stock_distribution(plan).unwrap();
    (session, issuer, stock)
}

/// 逐自然日推进到送转入账日 R+1：交易日走满全部 tick，非交易日仅日终；
/// 到达 R+1 当日走满 tick（首个 tick 安装除权锚）并完成当日日终入账。
fn advance_through_distribution_credit(session: &mut GameSession, ex_rights_on: &str) {
    let target = crate::CivilDate::from_iso(ex_rights_on).unwrap();
    while session.civil_date() < target {
        if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
            for _ in 0..session.state.setup.ticks_per_day {
                session.step().unwrap();
            }
        }
        session.end_civil_day().unwrap();
    }
    assert_eq!(
        session.civil_date().to_string(),
        ex_rights_on,
        "fixture 必须恰在除权/入账日到达"
    );
    for _ in 0..session.state.setup.ticks_per_day {
        session.step().unwrap();
    }
    session.end_civil_day().unwrap();
}

/// 当日交易辅助：假定当前处于交易日开盘前，先走首 tick 安装当日锚点，
/// 再挂机构买单与玩家卖单，走完剩余 tick 并完成日终。
fn trade_on_current_day(
    session: &mut GameSession,
    stock: &crate::account::StockCode,
    price_cents: i64,
    qty: u64,
) -> Vec<crate::Event> {
    session.step().unwrap();
    let institution = crate::orderbook::AccountId(1);
    let buyer_order_id = session.state.next_order_id;
    session
        .state
        .markets
        .get_mut(stock)
        .unwrap()
        .place(crate::Order {
            id: crate::OrderId(buyer_order_id),
            side: crate::Side::Buy,
            price: Money::from_cents(price_cents),
            qty: u32::try_from(qty).unwrap(),
            original_qty: u32::try_from(qty).unwrap(),
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
            crate::orderbook::AccountId(0),
            crate::Intent::PlaceLimit {
                code: stock.clone(),
                side: crate::Side::Sell,
                price: crate::LimitPrice::Fixed(Money::from_cents(price_cents)),
                qty: u32::try_from(qty).unwrap(),
            },
        )
        .unwrap();
    let mut events = Vec::new();
    for _ in 1..session.state.setup.ticks_per_day {
        events.extend(session.step().unwrap());
    }
    session.end_civil_day().unwrap();
    events
}

fn distribution_tax_book(
    session: &GameSession,
    stock: &crate::account::StockCode,
) -> crate::company::cash_dividend_tax::CashDividendTaxBook {
    session
        .corporate_actions()
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == crate::orderbook::AccountId(0) && book.stock() == stock)
        .cloned()
        .expect("默认大 A 模式装配名册后玩家必须自动开个人税账")
}

/// 送转新股必须在 R+1 到账当日以独立税批次进入税账 FIFO：
/// 取得日 = 到账日（财税〔2012〕85号第六条（八）+第一条第二款），
/// 来源为 CorporateAction，不与原股份视为同一批次。
#[test]
fn stock_distribution_credit_day_becomes_tax_lot_acquisition_day() {
    let (mut session, _issuer, stock) = session_with_distribution_tax_interaction(
        vec![crate::company::share_registry::ShareLot {
            id: "player-lot".into(),
            qty: 10,
            acquired_on: crate::CivilDate::from_iso("2030-01-02").unwrap(),
            source: crate::company::share_registry::AcquisitionSource::InitialAllocation {
                evidence: "fixture".into(),
            },
            restriction: crate::company::share_registry::ShareRestriction::Unrestricted,
        }],
        10,
        ("2030-01-02", "2030-01-03", "2030-01-04", "2030-01-07"),
        1,
        true,
    );
    advance_through_distribution_credit(&mut session, "2030-01-07");
    let book = distribution_tax_book(&session, &stock);
    let lots = book.lots();
    assert_eq!(
        lots.iter().map(|lot| lot.qty).sum::<u64>(),
        13,
        "送转新股必须进入税账：{lots:?}"
    );
    let credit_lot = lots
        .iter()
        .find(|lot| lot.id == "tax:stock-distribution:distribution-1:account-0")
        .expect("送转税批次必须以回执 lot 身份落账");
    assert_eq!(credit_lot.qty, 3);
    assert_eq!(
        credit_lot.acquired_on,
        crate::CivilDate::from_iso("2030-01-07").unwrap(),
        "送转股份税法取得日必须是 R+1 到账日"
    );
    assert_eq!(
        credit_lot.source,
        crate::company::cash_dividend_tax::TaxAcquisitionSource::CorporateAction {
            event: "distribution-1".into()
        }
    );
    assert_eq!(
        credit_lot.class,
        crate::company::cash_dividend_tax::TaxShareClass::PublicMarket
    );
    assert!(
        book.receipt_by_event("stock-distribution:distribution-1:0")
            .is_some(),
        "税账必须以非交易过户回执自身事件身份入账，而不是与同日公开市场回执撞车被跳过"
    );
}

/// 组合链路：默认税账 + 送转入账 + 其后现金分红 + 含送转股全额卖出。
/// 月末取得边界：旧批次 01-31 取得，2 月无 31 日钳制到 02-28；
/// 手算期望税额 = 旧 10 股 × 10 分 × 10%（已过 1 个月）+ 送转 3 股 × 10 分 × 20%
/// （02-14 取得，03-04 卖出未满 1 个月）= 10 + 6 = 16 分。
#[test]
fn dividend_after_stock_distribution_taxes_full_snapshot_shares_with_month_end_boundary() {
    let (mut session, _issuer, stock) = session_with_distribution_tax_interaction(
        vec![crate::company::share_registry::ShareLot {
            id: "player-lot".into(),
            qty: 10,
            acquired_on: crate::CivilDate::from_iso("2030-01-31").unwrap(),
            source: crate::company::share_registry::AcquisitionSource::InitialAllocation {
                evidence: "fixture".into(),
            },
            restriction: crate::company::share_registry::ShareRestriction::Unrestricted,
        }],
        10,
        ("2030-02-11", "2030-02-12", "2030-02-13", "2030-02-14"),
        2,
        true,
    );
    advance_through_distribution_credit(&mut session, "2030-02-14");
    assert_eq!(
        session
            .account(crate::orderbook::AccountId(0))
            .unwrap()
            .positions()[&stock]
            .qty(),
        13
    );
    // 入账次日批准现金分红（此时名册 eligible = 玩家 13 + 外部 2 = 15 股）。
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&stock)
        .unwrap()
        .clone();
    let total_shares = session.state.setup.stocks[0].total_shares;
    let date = |value| crate::CivilDate::from_iso(value).unwrap();
    let capital = crate::accounting::AccountingAmount::from_cents(i128::from(total_shares + 3));
    let plan = crate::company::cash_dividend::CashDividendPlan::new(
        "dividend-after-distribution".into(),
        issuer.clone(),
        stock.clone(),
        crate::calendar::CalendarExchange::Sse,
        crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
        date("2030-02-15"),
        date("2030-02-18"),
        date("2030-02-19"),
        date("2030-02-20"),
        date("2030-02-21"),
        Money::from_cents(10),
        Money::from_cents(150),
        session.state.civil_clock.calendar(),
    )
    .unwrap();
    session
        .approve_cash_dividend(
            crate::company::DividendDeclaration {
                plan_id: plan.plan_id.clone(),
                approved_on: date("2030-02-15"),
                total_gross: crate::accounting::AccountingAmount::from_cents(150),
                registered_capital: capital,
            },
            plan,
        )
        .unwrap();
    // 推进到发放日 02-21：玩家税前到账 13 × 10 = 130 分。
    while session.civil_date() <= date("2030-02-21") {
        if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
            for _ in 0..session.state.setup.ticks_per_day {
                session.step().unwrap();
            }
        }
        session.end_civil_day().unwrap();
    }
    assert!(
        session
            .corporate_actions()
            .account_gross_receipts
            .iter()
            .any(|receipt| receipt.account == crate::orderbook::AccountId(0)),
        "fixture 必须到达现金分红发放日"
    );
    // 推进到 03-04（周二）：旧批次已过 1 个月（01-31 → 02-28 钳制），
    // 送转批次 02-14 取得未满 1 个月（03-14 前）。全额卖出 13 股不卡日终。
    while session.civil_date() < date("2030-03-04") {
        if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
            for _ in 0..session.state.setup.ticks_per_day {
                session.step().unwrap();
            }
        }
        session.end_civil_day().unwrap();
    }
    // 卖价取 02-20 现金除息锚：送转锚 769 − 每股红利 10 = 759 分。
    let events = trade_on_current_day(&mut session, &stock, 759, 13);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, crate::Event::Trade { .. })),
        "fixture 必须产生含送转股的 13 股真实成交：{events:?}"
    );
    let book = distribution_tax_book(&session, &stock);
    assert_eq!(
        book.lots().iter().map(|lot| lot.qty).sum::<u64>(),
        0,
        "全额卖出后税账 FIFO 必须清零"
    );
    let last_collection = book.collections().last().expect("卖出日终必须收缴税款");
    assert_eq!(
        last_collection.collected,
        Money::from_cents(16),
        "税额必须按手算口径对全部 13 股求值（旧 10 股 10% + 送转 3 股 20%）"
    );
}

/// 限售继承：原批次全部限售时送转新股继承限售属性，税账按 StatutoryRestricted
/// 口径在解禁前对现金分红立即按 10% 计税（财税〔2012〕85号第四条）。
/// 手算期望：13 股 × 10 分 × 10% = 13 分（集成修复前税账缺送转股只收 10 分）。
#[test]
fn restricted_stock_distribution_lot_keeps_statutory_tax_class_and_pays_ten_percent() {
    let (mut session, _issuer, stock) = session_with_distribution_tax_interaction(
        vec![crate::company::share_registry::ShareLot {
            id: "player-lot".into(),
            qty: 10,
            acquired_on: crate::CivilDate::from_iso("2030-01-02").unwrap(),
            source: crate::company::share_registry::AcquisitionSource::InitialAllocation {
                evidence: "fixture".into(),
            },
            restriction: crate::company::share_registry::ShareRestriction::Restricted {
                reason: "nonfloat-lock".into(),
                release_on: crate::CivilDate::from_iso("2030-09-01").unwrap(),
            },
        }],
        10,
        ("2030-01-02", "2030-01-03", "2030-01-04", "2030-01-07"),
        1,
        true,
    );
    advance_through_distribution_credit(&mut session, "2030-01-07");
    let book = distribution_tax_book(&session, &stock);
    let credit_lot = book
        .lots()
        .iter()
        .find(|lot| lot.id == "tax:stock-distribution:distribution-1:account-0")
        .expect("限售送转新股必须进入税账");
    assert_eq!(
        credit_lot.class,
        crate::company::cash_dividend_tax::TaxShareClass::StatutoryRestricted {
            release_on: crate::CivilDate::from_iso("2030-09-01").unwrap(),
            basis: crate::company::cash_dividend_tax::StatutoryRestrictedBasis::FinanceTax2009167,
            qualification_evidence: "nonfloat-lock".to_string(),
        },
        "送转税批次必须继承限售类与解禁日"
    );
    let issuer = session
        .state
        .company_system
        .issuers()
        .issuer_of(&stock)
        .unwrap()
        .clone();
    let total_shares = session.state.setup.stocks[0].total_shares;
    let date = |value| crate::CivilDate::from_iso(value).unwrap();
    let capital = crate::accounting::AccountingAmount::from_cents(i128::from(total_shares + 3));
    let plan = crate::company::cash_dividend::CashDividendPlan::new(
        "dividend-restricted".into(),
        issuer.clone(),
        stock.clone(),
        crate::calendar::CalendarExchange::Sse,
        crate::company::ex_reference_price::CashDividendFormula::StandardCashOnly,
        date("2030-01-08"),
        date("2030-01-09"),
        date("2030-01-10"),
        date("2030-01-11"),
        date("2030-01-14"),
        Money::from_cents(10),
        Money::from_cents(150),
        session.state.civil_clock.calendar(),
    )
    .unwrap();
    session
        .approve_cash_dividend(
            crate::company::DividendDeclaration {
                plan_id: plan.plan_id.clone(),
                approved_on: date("2030-01-08"),
                total_gross: crate::accounting::AccountingAmount::from_cents(150),
                registered_capital: capital,
            },
            plan,
        )
        .unwrap();
    while session.civil_date() <= date("2030-01-14") {
        if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
            for _ in 0..session.state.setup.ticks_per_day {
                session.step().unwrap();
            }
        }
        session.end_civil_day().unwrap();
    }
    let book = distribution_tax_book(&session, &stock);
    let last_collection = book
        .collections()
        .last()
        .expect("解禁前分红必须当日按 10% 立即收缴");
    assert_eq!(
        last_collection.collected,
        Money::from_cents(13),
        "限售送转股必须并入 10% 立即计税基数：13 股 × 10 分 × 10%"
    );
}

/// 同日「先公开市场后送转」：R+1 当天玩家先全额卖出旧股，日终两条回执都
/// 必须入账且顺序确定（市场回执在前、送转回执在后），税账 lots = 0 + 3 = 3。
#[test]
fn same_day_market_sell_and_distribution_credit_both_recorded_in_order() {
    let (mut session, _issuer, stock) = session_with_distribution_tax_interaction(
        vec![crate::company::share_registry::ShareLot {
            id: "player-lot".into(),
            qty: 10,
            acquired_on: crate::CivilDate::from_iso("2030-01-02").unwrap(),
            source: crate::company::share_registry::AcquisitionSource::InitialAllocation {
                evidence: "fixture".into(),
            },
            restriction: crate::company::share_registry::ShareRestriction::Unrestricted,
        }],
        10,
        ("2030-01-02", "2030-01-03", "2030-01-04", "2030-01-07"),
        2,
        true,
    );
    // 推进到 R+1（01-07 周一）开盘前：01-02..01-04 完成公告与登记，
    // 周末两日仅日结。
    let target = crate::CivilDate::from_iso("2030-01-07").unwrap();
    while session.civil_date() < target {
        if session.civil_clock().phase() == crate::session::CivilPhase::IntradayTrading {
            for _ in 0..session.state.setup.ticks_per_day {
                session.step().unwrap();
            }
        }
        session.end_civil_day().unwrap();
    }
    assert_eq!(session.civil_date(), target);
    // R+1 当天：首 tick 安装除权锚（1000/1.3 → 769 分）后全额卖出 10 股旧股，
    // 日终同日追加送转 +3 入账。
    let events = trade_on_current_day(&mut session, &stock, 769, 10);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, crate::Event::Trade { .. })),
        "fixture 必须在除权日产生 10 股真实卖出：{events:?}"
    );
    let book = distribution_tax_book(&session, &stock);
    let days = book.tax_day_receipts();
    let same_day: Vec<&crate::company::cash_dividend_tax::TaxDayReceipt> = days
        .iter()
        .filter(|receipt| receipt.day == target)
        .collect();
    assert_eq!(
        same_day.len(),
        2,
        "同日公开市场与送转两条回执都必须入账：{same_day:?}"
    );
    assert_eq!(
        same_day[0].event_id,
        format!("session-market:{}:0:2030-01-07", stock.0)
    );
    assert_eq!(same_day[0].net_change, -10);
    assert!(
        !same_day[0].dispositions.is_empty(),
        "市场回执必须携带 FIFO 处置事实"
    );
    assert_eq!(
        same_day[1].event_id, "stock-distribution:distribution-1:0",
        "送转回执必须以自身事件身份入账且排在公开市场之后"
    );
    assert_eq!(same_day[1].net_change, 3);
    assert_eq!(
        book.lots().iter().map(|lot| lot.qty).sum::<u64>(),
        3,
        "税账持股必须等于 10 − 10 + 3（同日先市场后送转）"
    );
}

/// 送转税账事实必须经严格存档往返保持：恢复后深等、税批次仍在，
/// 继续日终不重复入账。
#[test]
fn stock_distribution_tax_facts_survive_strict_restore() {
    let (mut session, _issuer, stock) = session_with_distribution_tax_interaction(
        vec![crate::company::share_registry::ShareLot {
            id: "player-lot".into(),
            qty: 10,
            acquired_on: crate::CivilDate::from_iso("2030-01-02").unwrap(),
            source: crate::company::share_registry::AcquisitionSource::InitialAllocation {
                evidence: "fixture".into(),
            },
            restriction: crate::company::share_registry::ShareRestriction::Unrestricted,
        }],
        10,
        ("2030-01-02", "2030-01-03", "2030-01-04", "2030-01-07"),
        1,
        true,
    );
    advance_through_distribution_credit(&mut session, "2030-01-07");
    let before = distribution_tax_book(&session, &stock);
    let saved = session.save().unwrap();
    let mut restored = GameSession::restore(&saved).unwrap();
    assert_eq!(
        restored.business_state_hash().unwrap(),
        session.business_state_hash().unwrap(),
        "送转税账事实必须可严格恢复"
    );
    let after = distribution_tax_book(&restored, &stock);
    assert_eq!(before, after);
    assert!(after
        .receipt_by_event("stock-distribution:distribution-1:0")
        .is_some());
    for _ in 0..restored.state.setup.ticks_per_day {
        restored.step().unwrap();
    }
    restored.end_civil_day().unwrap();
    let continued = distribution_tax_book(&restored, &stock);
    // 次日新增的只能是常规公开市场日结；送转税事实必须仍然恰好一条，不重复入账。
    assert_eq!(
        continued
            .tax_day_receipts()
            .iter()
            .filter(|receipt| receipt.event_id == "stock-distribution:distribution-1:0")
            .count(),
        1,
        "恢复后继续日终不得重复入账送转税事实"
    );
    assert_eq!(
        continued.tax_day_receipts().len(),
        after.tax_day_receipts().len() + 1
    );
}

/// 恢复勾稽防御：名册已有非交易过户回执而税账缺少对应日结事实时必须显式失败，
/// 不允许静默缺股（铁律 2；集成修复前该状态校验通过即为漏洞）。
#[test]
fn validate_rejects_registry_receipt_missing_from_dividend_tax_book() {
    use crate::company::cash_dividend_tax::{
        CashDividendTaxBook, DividendTaxLot, DividendTaxProfile,
    };

    let (mut session, _issuer, stock) = session_with_distribution_tax_interaction(
        vec![crate::company::share_registry::ShareLot {
            id: "player-lot".into(),
            qty: 10,
            acquired_on: crate::CivilDate::from_iso("2030-01-02").unwrap(),
            source: crate::company::share_registry::AcquisitionSource::InitialAllocation {
                evidence: "fixture".into(),
            },
            restriction: crate::company::share_registry::ShareRestriction::Unrestricted,
        }],
        10,
        ("2030-01-02", "2030-01-03", "2030-01-04", "2030-01-07"),
        1,
        true,
    );
    advance_through_distribution_credit(&mut session, "2030-01-07");
    // 重建一个只回放公开市场日结、缺少送转回执事实的税账。
    let mut rebuilt = CashDividendTaxBook::new(
        crate::orderbook::AccountId(0),
        stock.clone(),
        DividendTaxProfile::IndividualPublicMarket,
        crate::CivilDate::from_iso("2030-01-02").unwrap(),
        vec![DividendTaxLot {
            id: "tax:player-lot".into(),
            qty: 10,
            acquired_on: crate::CivilDate::from_iso("2030-01-02").unwrap(),
            source: crate::company::cash_dividend_tax::TaxAcquisitionSource::InitialAllocation {
                evidence: "fixture".into(),
            },
            class: crate::company::cash_dividend_tax::TaxShareClass::PublicMarket,
        }],
    )
    .unwrap();
    for day in [
        "2030-01-03",
        "2030-01-04",
        "2030-01-05",
        "2030-01-06",
        "2030-01-07",
    ] {
        rebuilt
            .record_net_day(
                format!("session-market:{}:0:{day}", stock.0),
                crate::CivilDate::from_iso(day).unwrap(),
                0,
                None,
            )
            .unwrap();
    }
    session.state.corporate_actions.dividend_tax_books[0] = rebuilt;
    let positions = account_position_projection(&session);
    let rejection = session
        .state
        .corporate_actions
        .validate(
            &positions,
            &session.state.company_system,
            crate::CivilDate::from_iso("2030-01-07").unwrap(),
            None,
        )
        .unwrap_err();
    assert!(
        rejection.to_string().contains("送转"),
        "税账缺少名册回执日结事实必须显式指向送转×税账交互：{rejection}"
    );
}

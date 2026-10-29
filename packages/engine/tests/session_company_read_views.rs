//! B4e 最小只读切片：读取既有公司与税账事实，不调用日终或付款命令。

include!("../test-support/company_state/system_saved_mut.rs");
#[allow(dead_code)]
#[path = "publications/session_fixture.rs"]
mod session_fixture;
include!("../test-support/simple_company.rs");

use engine::calendar::{CalendarExchange, TradingCalendar};
use engine::company::cash_dividend::{CashDividendBook, CashDividendPlan, HolderPaymentOutcome};
use engine::company::cash_dividend_tax::{
    CashDividendTaxBook, CashDividendTaxMode, DividendTaxLot, DividendTaxProfile,
    ExactDividendTaxAmount, TaxAcquisitionSource, TaxShareClass,
};
use engine::company::ex_reference_price::CashDividendFormula;
use engine::company::share_registry::HolderId;
use engine::company::CompanyId;
use engine::session::{DividendTaxStatus, FlatWithholdingReceipt, TaxpayerIdentity};
use engine::{AccountId, CivilDate, GameSession, Money, SessionSetup, StockCode};

fn date(text: &str) -> CivilDate {
    CivilDate::from_iso(text).unwrap()
}

fn setup(mode: CashDividendTaxMode, foundation: bool) -> SessionSetup {
    let mut setup = session_fixture::civil_setup(date("2030-01-08"));
    setup.npcs.retail_count = 2;
    setup.npcs.inst_count = 1;
    setup.npcs.hot_count = 0;
    setup.float_allocation = engine::FloatAllocation::class_percentages(
        80.0,
        20.0,
        0.0,
        engine::session::WithinKindDistribution::EqualPercentage,
    );
    setup.company_system = Some(simple_company_fixture!(engine; ["600101"]));
    setup.dividend_tax_mode = Some(mode);
    setup.flat_withholding_bp = (mode == CashDividendTaxMode::FlatWithholding).then_some(1000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.auto_corporate_foundation = foundation;
    setup.report_frequency = Some(engine::information::ReportFrequency::Quarterly);
    if mode == CashDividendTaxMode::Exempt {
        setup.config.stamp_tax_rate = 0.0;
    }
    setup
}

fn save_bytes(session: &GameSession) -> Vec<u8> {
    serde_json::to_vec(&session.save().unwrap()).unwrap()
}

#[test]
fn period_explanation_preserves_domain_result_and_rejects_unsettled_period() {
    let session = GameSession::new(setup(CashDividendTaxMode::AShareIndividual, true), 7).unwrap();
    let before = save_bytes(&session);
    let period = date("2029-12-31");
    assert_eq!(
        session
            .company_period_explanation(&CompanyId(0), period)
            .unwrap(),
        session
            .company_system()
            .unwrap()
            .period_change_explanation(&CompanyId(0), period)
            .unwrap()
    );
    assert!(session
        .company_period_explanation(&CompanyId(0), date("2030-01-31"))
        .unwrap_err()
        .to_string()
        .contains("尚未结算"));
    assert!(session
        .company_period_explanation(&CompanyId(999), period)
        .unwrap_err()
        .to_string()
        .contains("未知公司"));
    assert!(session
        .company_period_explanation(&CompanyId(0), date("2029-12-30"))
        .unwrap_err()
        .to_string()
        .contains("不是结算周期末日"));
    assert_eq!(before, save_bytes(&session));
}

#[test]
fn account_tax_status_distinguishes_mode_identity_and_configured_books() {
    for mode in [
        CashDividendTaxMode::FlatWithholding,
        CashDividendTaxMode::AShareIndividual,
        CashDividendTaxMode::Exempt,
    ] {
        let session = GameSession::new(setup(mode, true), 7).unwrap();
        let before = save_bytes(&session);
        for (account, identity) in [
            (AccountId(0), TaxpayerIdentity::Personal),
            (AccountId(1), TaxpayerIdentity::Personal),
            (AccountId(3), TaxpayerIdentity::NonIndividualPending),
        ] {
            let view = session.account_dividend_tax_status(account).unwrap();
            assert_eq!(view.mode, mode);
            assert_eq!(view.identity, identity);
            assert_eq!(view.stocks.len(), 1);
            assert_eq!(view.stocks[0].stock, StockCode("600101".into()));
            let has_book = mode == CashDividendTaxMode::AShareIndividual && account == AccountId(1);
            let expected = if mode == CashDividendTaxMode::FlatWithholding {
                DividendTaxStatus::FlatWithholding
            } else if has_book {
                DividendTaxStatus::IndividualPublicMarket
            } else {
                DividendTaxStatus::TreatmentNotConfigured
            };
            assert_eq!(view.stocks[0].status, expected);
        }
        let outstanding = session.dividend_tax_outstanding_views().unwrap();
        assert_eq!(
            outstanding.len(),
            if mode == CashDividendTaxMode::AShareIndividual {
                2
            } else {
                0
            }
        );
        for view in outstanding {
            assert_eq!(view.outstanding.numerator(), 0);
            assert_eq!(view.outstanding.denominator(), 1);
            assert!(view.latest_collection.is_none());
        }
        assert!(session
            .account_dividend_tax_status(AccountId(999))
            .unwrap_err()
            .to_string()
            .contains("999"));
        assert_eq!(before, save_bytes(&session));
    }
}

#[test]
fn configured_company_without_registry_has_explicitly_empty_tax_views() {
    let session = GameSession::new(setup(CashDividendTaxMode::AShareIndividual, false), 7).unwrap();
    let before = save_bytes(&session);
    assert!(session
        .account_dividend_tax_status(AccountId(0))
        .unwrap()
        .stocks
        .is_empty());
    assert!(session.dividend_tax_outstanding_views().unwrap().is_empty());
    assert_eq!(before, save_bytes(&session));
}

#[test]
fn queries_reject_sessions_without_company_line() {
    let session = GameSession::new(session_fixture::civil_setup(date("2030-01-08")), 7).unwrap();
    assert!(session.corporate_actions().is_none());
    assert!(session
        .company_period_explanation(&CompanyId(0), date("2029-12-31"))
        .unwrap_err()
        .to_string()
        .contains("公司"));
    assert!(session
        .account_dividend_tax_status(AccountId(0))
        .unwrap_err()
        .to_string()
        .contains("公司"));
    assert!(session
        .dividend_tax_outstanding_views()
        .unwrap_err()
        .to_string()
        .contains("公司"));
    assert!(session
        .owner_flat_withholding_receipts(AccountId(0))
        .unwrap_err()
        .to_string()
        .contains("公司"));
}

#[test]
fn flat_receipts_are_owner_filtered_and_queries_do_not_change_save_bytes() {
    // 从已完成的领域账簿构造冻结历史事实；不以 Session 日终接线证明付款能力。
    let session = GameSession::new(setup(CashDividendTaxMode::FlatWithholding, true), 7).unwrap();
    let mut saved = session.save().unwrap();
    let actions = system_saved_mut(&mut saved).1;
    let calendar = TradingCalendar::current_default_calendar().unwrap();
    let plan = CashDividendPlan::new(
        "history-dividend".into(),
        CompanyId(0),
        StockCode("600101".into()),
        CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        date("2030-01-02"),
        date("2030-01-03"),
        date("2030-01-07"),
        date("2030-01-08"),
        date("2030-01-08"),
        Money::from_cents(10),
        Money::from_cents(100_000_000),
        &calendar,
    )
    .unwrap();
    let mut book = CashDividendBook::new(plan).unwrap();
    book.announce(date("2030-01-03")).unwrap();
    let snapshot = actions.registries[0]
        .register("history-dividend".into(), date("2030-01-07"))
        .unwrap()
        .clone();
    book.register(snapshot, &calendar).unwrap();
    book.mark_payable(date("2030-01-08")).unwrap();
    let outcomes: Vec<_> = book
        .entitlements()
        .unwrap()
        .iter()
        .map(|entitlement| HolderPaymentOutcome::Paid {
            holder: entitlement.holder.clone(),
            amount: entitlement.gross,
        })
        .collect();
    book.settle("history-payment".into(), date("2030-01-08"), outcomes)
        .unwrap();
    book.validate().unwrap();
    for (holder, gross) in book.successful_payments() {
        if let HolderId::Account(account) = holder {
            actions
                .flat_withholding_receipts
                .push(FlatWithholdingReceipt {
                    payment_id: "history-payment".into(),
                    plan_id: "history-dividend".into(),
                    account: *account,
                    paid_on: date("2030-01-08"),
                    gross: *gross,
                    rate_bp: 1000,
                    withheld: Money::from_cents(
                        engine::company::cash_dividend_tax::flat_withholding_cents(
                            gross.cents(),
                            1000,
                        )
                        .unwrap(),
                    ),
                });
            actions.account_gross_receipts.push(
                serde_json::from_value(serde_json::json!({
                    "payment_id": "history-payment", "plan_id": "history-dividend",
                    "account": account, "paid_on": date("2030-01-08"), "gross": gross,
                    "tax_status": "FlatWithholding"
                }))
                .unwrap(),
            );
        } else {
            actions.external_receipts.push(
                serde_json::from_value(serde_json::json!({
                    "payment_id": "history-payment", "plan_id": "history-dividend",
                    "holder": holder, "paid_on": date("2030-01-08"), "gross": gross,
                    "tax_status": "TreatmentNotConfigured"
                }))
                .unwrap(),
            );
        }
    }
    actions.dividends.push(book);
    assert!(actions
        .flat_withholding_receipts
        .iter()
        .any(|r| r.account == AccountId(1)));
    assert!(actions
        .flat_withholding_receipts
        .iter()
        .any(|r| r.account == AccountId(2)));
    let session = GameSession::restore(&saved).unwrap();
    let before = save_bytes(&session);
    for owner in [AccountId(1), AccountId(2)] {
        let receipts = session.owner_flat_withholding_receipts(owner).unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].account, owner);
        assert_eq!(receipts[0].rate_bp, 1000);
        assert_eq!(
            receipts[0].withheld.cents(),
            engine::company::cash_dividend_tax::flat_withholding_cents(
                receipts[0].gross.cents(),
                1000
            )
            .unwrap()
        );
    }
    assert!(session
        .owner_flat_withholding_receipts(AccountId(0))
        .unwrap()
        .is_empty());
    assert!(session
        .owner_flat_withholding_receipts(AccountId(999))
        .unwrap_err()
        .to_string()
        .contains("999"));
    assert_eq!(before, save_bytes(&session));
}

#[test]
fn non_flat_receipt_query_rejects_instead_of_returning_empty_ledger() {
    let session = GameSession::new(setup(CashDividendTaxMode::AShareIndividual, true), 7).unwrap();
    let before = save_bytes(&session);
    assert!(session
        .owner_flat_withholding_receipts(AccountId(0))
        .unwrap_err()
        .to_string()
        .contains("FlatWithholding"));
    assert_eq!(before, save_bytes(&session));
}

fn assessed_tax_book_before_collection() -> CashDividendTaxBook {
    // 两天的合法 FIFO / 到账事实，尚未尝试划收。
    let stock = StockCode("600101".into());
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        stock.clone(),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![DividendTaxLot {
            id: "lot".into(),
            qty: 10,
            acquired_on: date("2029-12-01"),
            source: TaxAcquisitionSource::InitialAllocation {
                evidence: "initial-allocation".into(),
            },
            class: TaxShareClass::PublicMarket,
        }],
    )
    .unwrap();
    book.register_dividend(
        "dividend".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.register_dividend(
        "later-dividend".into(),
        date("2030-01-01"),
        ExactDividendTaxAmount::new(100, 1).unwrap(),
    )
    .unwrap();
    book.record_net_day("sell".into(), date("2030-01-02"), -5, None)
        .unwrap();
    book.record_payment(
        "dividend",
        "payment".into(),
        date("2030-01-02"),
        Money::from_cents(1000),
        "historical-receipt".into(),
    )
    .unwrap();
    book.validate().unwrap();
    book
}

#[test]
fn uncollected_assessment_does_not_claim_insufficient_cash() {
    let mut actions = engine::session::SessionCorporateActions::default();
    actions
        .dividend_tax_books
        .push(assessed_tax_book_before_collection());
    let before = serde_json::to_vec(&actions).unwrap();
    let views = actions.dividend_tax_outstanding_views().unwrap();
    assert_eq!(views[0].outstanding.numerator(), 50);
    assert!(
        views[0].latest_collection.is_none(),
        "未尝试划收不能制造资金不足回执"
    );
    assert_eq!(before, serde_json::to_vec(&actions).unwrap());
}

#[test]
fn outstanding_projection_preserves_exact_tax_and_actual_collection_receipts() {
    let stock = StockCode("600101".into());
    let mut book = assessed_tax_book_before_collection();
    book.collect_due(
        "partial-collection".into(),
        date("2030-01-02"),
        Money::from_cents(20),
    )
    .unwrap();
    book.validate().unwrap();
    let mut actions = engine::session::SessionCorporateActions::default();
    actions.dividend_tax_books.push(book.clone());
    let before = serde_json::to_vec(&actions).unwrap();
    let views = actions.dividend_tax_outstanding_views().unwrap();
    assert_eq!(views.len(), 1);
    assert_eq!(views[0].account, AccountId(1));
    assert_eq!(views[0].stock, stock);
    assert_eq!(views[0].outstanding.numerator(), 30);
    assert_eq!(views[0].outstanding.denominator(), 1);
    let collection = views[0].latest_collection.as_ref().unwrap();
    assert_eq!(collection.event_id, "partial-collection");
    assert_eq!(collection.available_cash.cents(), 20);
    assert_eq!(collection.collected.cents(), 20);
    assert_eq!(collection.outstanding.numerator(), 30);
    assert!(collection.needs_funds);
    assert_eq!(
        serde_json::to_value(&views[0].outstanding).unwrap(),
        serde_json::json!({"numerator":"30", "denominator":"1"})
    );
    assert_eq!(before, serde_json::to_vec(&actions).unwrap());
    book.collect_due(
        "clear-collection".into(),
        date("2030-01-02"),
        Money::from_cents(30),
    )
    .unwrap();
    actions.dividend_tax_books = vec![book];
    let cleared = actions.dividend_tax_outstanding_views().unwrap();
    assert_eq!(cleared[0].outstanding.numerator(), 0);
    assert_eq!(cleared[0].outstanding.denominator(), 1);
    let collection = cleared[0].latest_collection.as_ref().unwrap();
    assert_eq!(collection.event_id, "clear-collection");
    assert!(!collection.needs_funds);
    assert_eq!(collection.outstanding.numerator(), 0);
}

#[test]
fn later_assessment_preserves_latest_collection_without_explaining_new_outstanding() {
    let mut book = assessed_tax_book_before_collection();
    book.collect_due(
        "partial-collection".into(),
        date("2030-01-02"),
        Money::from_cents(20),
    )
    .unwrap();
    let previous = book.collections().last().unwrap().clone();
    book.record_payment(
        "later-dividend",
        "later-payment".into(),
        date("2030-01-02"),
        Money::from_cents(1000),
        "later-historical-receipt".into(),
    )
    .unwrap();
    book.validate().unwrap();
    let mut actions = engine::session::SessionCorporateActions::default();
    actions.dividend_tax_books.push(book);
    let before = serde_json::to_vec(&actions).unwrap();
    let views = actions.dividend_tax_outstanding_views().unwrap();
    assert_eq!(views[0].outstanding.numerator(), 80);
    assert_eq!(views[0].latest_collection, Some(previous));
    assert_eq!(
        views[0]
            .latest_collection
            .as_ref()
            .unwrap()
            .outstanding
            .numerator(),
        30
    );
    assert_eq!(before, serde_json::to_vec(&actions).unwrap());
}

#[test]
fn session_outstanding_query_preserves_populated_tax_book_after_restore() {
    let session = GameSession::new(setup(CashDividendTaxMode::AShareIndividual, true), 7).unwrap();
    let mut saved = session.save().unwrap();
    let actions = system_saved_mut(&mut saved).1;
    // 编辑已有税账的合法历史事实，不改变股份持有量；无处置时到账尚不产生应纳税额。
    let book = actions
        .dividend_tax_books
        .iter_mut()
        .find(|book| book.account() == AccountId(1))
        .unwrap();
    let quantity: u64 = book.lots().iter().map(|lot| lot.qty).sum();
    book.register_dividend(
        "historical-payment".into(),
        book.settled_on(),
        ExactDividendTaxAmount::new(10, 1).unwrap(),
    )
    .unwrap();
    book.record_payment(
        "historical-payment",
        "historical-payment-receipt".into(),
        book.settled_on(),
        Money::from_cents(i64::try_from(quantity).unwrap() * 10),
        "edited-historical-cash-receipt".into(),
    )
    .unwrap();
    book.validate().unwrap();
    let expected = actions.dividend_tax_outstanding_views().unwrap();
    let session = GameSession::restore(&saved).unwrap();
    let before = save_bytes(&session);
    let actual = session.dividend_tax_outstanding_views().unwrap();
    assert_eq!(actual, expected);
    assert_eq!(actual.len(), 2);
    assert_eq!(
        actual.iter().map(|view| view.account).collect::<Vec<_>>(),
        vec![AccountId(1), AccountId(2)]
    );
    assert!(actual
        .iter()
        .all(|view| view.stock == StockCode("600101".into())));
    assert_eq!(before, save_bytes(&session));
}

#[test]
fn latest_collection_uses_operation_sequence_after_valid_same_day_receipts_reordering() {
    let mut book = CashDividendTaxBook::new(
        AccountId(1),
        StockCode("600101".into()),
        DividendTaxProfile::IndividualPublicMarket,
        date("2030-01-01"),
        vec![],
    )
    .unwrap();
    book.collect_due("first".into(), date("2030-01-01"), Money::from_cents(1))
        .unwrap();
    let latest = book
        .collect_due("second".into(), date("2030-01-01"), Money::from_cents(2))
        .unwrap();
    let mut wire = serde_json::to_value(&book).unwrap();
    wire["collections"].as_array_mut().unwrap().swap(0, 1);
    let restored: CashDividendTaxBook = serde_json::from_value(wire).unwrap();
    restored.validate().unwrap();
    let mut actions = engine::session::SessionCorporateActions::default();
    actions.dividend_tax_books.push(restored);
    let before = serde_json::to_vec(&actions).unwrap();
    let views = actions.dividend_tax_outstanding_views().unwrap();
    assert_eq!(views[0].latest_collection, Some(latest));
    assert_eq!(before, serde_json::to_vec(&actions).unwrap());
}

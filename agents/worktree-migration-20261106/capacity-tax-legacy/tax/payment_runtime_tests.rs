//! R4 现金分红付款的真实 GameSession 路径：登记快照、账户到账与个人税划收。

use super::*;
use crate::company::cash_dividend::{CashDividendBook, CashDividendPlan, CashDividendStatus};
use crate::company::cash_dividend_tax::CashDividendTaxMode;
use crate::company::ex_reference_price::CashDividendFormula;
use crate::company::share_registry::HolderId;
use crate::company::{CompanyId, DividendDeclaration};
use crate::runtime::bridge_tests::setup_fixture;
use crate::{AccountId, Event, Intent, LimitPrice, Money, Side, StockId};
use std::sync::Arc;

include!("../../../test-support/simple_company.rs");

const STOCK: &str = "600101";
const PLAN_ID: &str = "r4-cash-payment";
const START: &str = "2030-06-03";
const PAYMENT_DAY: &str = "2030-06-04";
const GROSS_PER_SHARE_CENTS: i64 = 1;
const RETAIL_HOLDERS: [AccountId; 6] = [
    AccountId(1),
    AccountId(2),
    AccountId(3),
    AccountId(4),
    AccountId(5),
    AccountId(6),
];

fn date(value: &str) -> crate::CivilDate {
    crate::CivilDate::from_iso(value).expect("fixture 日期有效")
}

fn payment_session(mode: CashDividendTaxMode) -> GameSession {
    let mut setup = setup_fixture();
    setup.stocks.truncate(1);
    setup.stocks[0].code = crate::StockCode(STOCK.into());
    setup.stocks[0].exchange = crate::StockExchange::Shanghai;
    setup.stocks[0].total_shares = 1_200;
    setup.stocks[0].float_shares = 1_200;
    setup.npcs.retail_count = RETAIL_HOLDERS.len() as u32;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    setup.strategy_params.retail.arrival_rate = 0.0;
    setup.ticks_per_day = 1;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.history_len = 1;
    setup.start_date = date(START);
    setup.float_allocation = crate::FloatAllocation::class_percentages(
        100.0,
        0.0,
        0.0,
        crate::WithinKindDistribution::EqualPercentage,
    );
    setup.company_system = Some(simple_company_fixture!(crate; [STOCK]));
    setup.dividend_tax_mode = Some(mode);
    setup.flat_withholding_bp = (mode == CashDividendTaxMode::FlatWithholding).then_some(1_000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.auto_corporate_foundation = true;

    let mut session = GameSession::new_with_runtime_groups(setup, 7, 3)
        .expect("构造含真实 runtime groups 的公司会话");
    for account in RETAIL_HOLDERS {
        let position = session
            .account(account)
            .and_then(|account| account.position(&StockId(0)))
            .expect("六名散户均从真实 float allocation 获得证券持仓");
        assert_eq!(position.qty(), 200, "登记前每名散户持有 200 股");
    }

    let company = CompanyId(0);
    let registered_capital = session
        .company_system()
        .expect("Simple 公司系统已安装")
        .dividend_legal_facts(&company)
        .expect("读取真实公司法定股本事实")
        .expect("自动公司基础设施已建立注册资本事实")
        .registered_capital;
    let plan = CashDividendPlan::new(
        PLAN_ID.into(),
        company,
        crate::StockCode(STOCK.into()),
        crate::calendar::CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        date(START),
        date(START),
        date(START),
        date(PAYMENT_DAY),
        date(PAYMENT_DAY),
        Money::from_cents(GROSS_PER_SHARE_CENTS),
        Money::from_cents(1_200),
        session.civil_clock().calendar(),
    )
    .expect("登记及除息、付款日期由真实交易日历校验");
    let system = session
        .state
        .companies
        .system_arc_mut()
        .expect("CompanySystem 状态存在");
    Arc::make_mut(system)
        .declare_dividend(
            &company,
            DividendDeclaration {
                plan_id: PLAN_ID.into(),
                approved_on: date(START),
                total_gross: crate::accounting::AccountingAmount::from_cents(1_200),
                registered_capital,
            },
        )
        .expect("使用真实法定注册资本批准分红");
    session
        .state
        .companies
        .corporate_actions_mut()
        .expect("CompanySystem 公司行为容器存在")
        .dividends
        .push(CashDividendBook::new(plan).expect("批准后的分红账簿可创建"));
    session.save().expect("初始批准状态必须是真实合法存档");
    session
}

fn cash(session: &GameSession, account: AccountId) -> Money {
    session.account(account).expect("账户存在").cash()
}

fn register_on_june_3(session: &mut GameSession) {
    assert_eq!(session.civil_date(), date(START));
    session.step().expect("6 月 3 日真实 market step 完成");
    session
        .end_civil_day()
        .expect("6 月 3 日真实自然日日结完成");
    let book = &session.corporate_actions().expect("公司行为存在").dividends[0];
    assert_eq!(book.status(), &CashDividendStatus::Registered);
    assert!(
        book.registration().is_some(),
        "登记来自真实 market close capture"
    );
    assert!(book.payments().is_empty(), "登记日不得提前付款");
}

fn holder_entitlements(session: &GameSession) -> Vec<(AccountId, Money)> {
    let book = &session.corporate_actions().expect("公司行为存在").dividends[0];
    book.entitlements()
        .expect("实际登记后权益可读")
        .iter()
        .filter_map(|entitlement| match entitlement.holder {
            HolderId::Account(account) => Some((account, entitlement.gross)),
            _ => None,
        })
        .collect()
}

fn step_on_june_4(session: &mut GameSession) {
    assert_eq!(session.civil_date(), date(PAYMENT_DAY));
    session.step().expect("付款日真实 market step 完成");
}

#[test]
fn r4_cash_01_payable_credits_gross_and_activates_personal_income_once() {
    let mut session = payment_session(CashDividendTaxMode::AShareIndividual);
    register_on_june_3(&mut session);
    let entitlements = holder_entitlements(&session);
    assert_eq!(entitlements.len(), RETAIL_HOLDERS.len());
    step_on_june_4(&mut session);
    let before: Vec<_> = RETAIL_HOLDERS
        .iter()
        .map(|account| (*account, cash(&session, *account)))
        .collect();
    session.end_civil_day().expect("付款日真实自然日日结完成");
    let book = &session.corporate_actions().unwrap().dividends[0];
    for (account, gross) in &entitlements {
        assert_eq!(
            cash(&session, *account).cents()
                - before
                    .iter()
                    .find(|row| row.0 == *account)
                    .unwrap()
                    .1
                    .cents(),
            gross.cents(),
            "R4 付款日必须真实增加持有人现金，不能只保持 Registered"
        );
    }
    assert_eq!(book.status(), &CashDividendStatus::Paid);
    assert_eq!(book.successful_payments().len(), RETAIL_HOLDERS.len());
    assert_eq!(book.paid_amount().unwrap(), Money::from_cents(1_200));
    assert_eq!(book.payments().len(), 1);
    let actions = session.corporate_actions().unwrap();
    assert_eq!(actions.account_gross_receipts.len(), RETAIL_HOLDERS.len());
    assert_eq!(actions.flat_withholding_receipts.len(), 0);
    for tax_book in &actions.dividend_tax_books {
        assert!(
            tax_book
                .payment_by_id(&format!("dividend-payment:{PLAN_ID}:{PAYMENT_DAY}"))
                .is_some()
        );
        let serialized_tax_book = serde_json::to_value(tax_book).unwrap();
        assert_eq!(
            serialized_tax_book["dividends"][0]["payments"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            serialized_tax_book["collections"].as_array().unwrap().len(),
            1
        );
        assert_eq!(tax_book.collections().len(), 1);
        assert_eq!(tax_book.collections()[0].collected, Money::ZERO);
        assert_eq!(tax_book.outstanding_tax().unwrap().numerator(), 0);
    }

    let restored = GameSession::restore(&session.save().unwrap()).expect("付款后真实存档可恢复");
    let restored_before: Vec<_> = RETAIL_HOLDERS
        .iter()
        .map(|account| (*account, cash(&restored, *account)))
        .collect();
    let mut restored = restored;
    restored
        .step()
        .expect("恢复后 6 月 5 日真实 market step 完成");
    restored.end_civil_day().expect("恢复后 6 月 5 日日结完成");
    for (account, previous) in restored_before {
        assert_eq!(
            cash(&restored, account),
            previous,
            "恢复后不得重复支付同一分红"
        );
    }
}

#[test]
fn r4_cash_02_flat_payment_credits_net_and_records_real_withholding() {
    let mut session = payment_session(CashDividendTaxMode::FlatWithholding);
    register_on_june_3(&mut session);
    step_on_june_4(&mut session);
    let before: Vec<_> = RETAIL_HOLDERS
        .iter()
        .map(|account| (*account, cash(&session, *account)))
        .collect();
    session.end_civil_day().expect("付款日真实自然日日结完成");
    let actions = session.corporate_actions().unwrap();
    assert_eq!(actions.account_gross_receipts.len(), RETAIL_HOLDERS.len());
    assert_eq!(
        actions.flat_withholding_receipts.len(),
        RETAIL_HOLDERS.len()
    );
    for receipt in &actions.flat_withholding_receipts {
        assert_eq!(receipt.gross, Money::from_cents(200));
        assert_eq!(receipt.withheld, Money::from_cents(20));
        assert_eq!(receipt.rate_bp, 1_000);
        assert_eq!(
            cash(&session, receipt.account).cents()
                - before
                    .iter()
                    .find(|row| row.0 == receipt.account)
                    .unwrap()
                    .1
                    .cents(),
            180,
            "R4 简税付款必须在真实账户留下税后净额"
        );
    }
    assert_eq!(
        actions
            .flat_withholding_receipts
            .iter()
            .map(|receipt| receipt.gross.cents())
            .sum::<i64>(),
        actions
            .flat_withholding_receipts
            .iter()
            .map(|receipt| receipt.withheld.cents())
            .sum::<i64>()
            + actions
                .account_gross_receipts
                .iter()
                .map(|receipt| receipt.gross.cents() - 20)
                .sum::<i64>()
    );
    assert!(actions.dividend_tax_books.is_empty());
    assert_eq!(
        session.corporate_actions().unwrap().dividends[0].status(),
        &CashDividendStatus::Paid
    );
}

#[test]
fn r4_cash_03_actual_sale_before_payment_collects_personal_tax() {
    let mut session = payment_session(CashDividendTaxMode::AShareIndividual);
    register_on_june_3(&mut session);
    let seller = AccountId(1);
    let buyer = AccountId(0);
    let before_position = session
        .account(seller)
        .unwrap()
        .position(&StockId(0))
        .unwrap();
    assert_eq!(before_position.qty(), 200);
    let before_sellable = before_position.sellable();
    let before_buyer_entitled = holder_entitlements(&session)
        .iter()
        .any(|(account, _)| *account == buyer);
    assert!(!before_buyer_entitled, "玩家买入前不属于既有登记快照");
    session.seed_order_for_test(
        seller,
        Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Sell,
            price: LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
        &mut Vec::new(),
    );
    session
        .enqueue_player_intent(
            buyer,
            Intent::PlaceLimit {
                code: StockId(0),
                side: Side::Buy,
                price: LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
        )
        .expect("真实玩家买入意图受理");
    let events = session.step().expect("付款日前真实撮合完成");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, Event::Trade { .. })),
        "两笔真实委托必须成交"
    );
    let seller_position = session
        .account(seller)
        .unwrap()
        .position(&StockId(0))
        .unwrap();
    assert_eq!(seller_position.qty(), 100);
    assert_eq!(seller_position.sellable(), before_sellable - 100);
    assert!(
        session
            .personal_trade_confirmations(seller)
            .unwrap()
            .iter()
            .any(|confirmation| confirmation.civil_date == date(PAYMENT_DAY)
                && confirmation.side == Side::Sell
                && confirmation.quantity_shares == 100)
    );
    let seller_cash_before_payment = cash(&session, seller);
    session
        .end_civil_day()
        .expect("真实交易确认驱动登记权益税务与付款日收尾");
    let after = cash(&session, seller);
    assert_eq!(
        after.cents() - seller_cash_before_payment.cents(),
        180,
        "R4 真实转让后的付款必须按原登记权益收缴个人税"
    );
    let actions = session.corporate_actions().unwrap();
    assert!(
        !actions.dividends[0]
            .entitlements()
            .unwrap()
            .iter()
            .any(|entitlement| entitlement.holder == HolderId::Account(buyer))
    );
    let seller_tax = actions
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == seller)
        .expect("登记时已存在个人税账");
    assert_eq!(seller_tax.collections().len(), 1);
    assert_eq!(seller_tax.collections()[0].collected, Money::from_cents(20));
    assert_eq!(seller_tax.outstanding_tax().unwrap().numerator(), 0);
    assert_eq!(actions.account_gross_receipts.len(), RETAIL_HOLDERS.len());
    assert_eq!(
        actions
            .account_gross_receipts
            .iter()
            .map(|receipt| receipt.gross.cents())
            .sum::<i64>(),
        1_200
    );
}

//! R4 现金分红付款的真实 GameSession 路径：登记快照、账户到账与个人税划收。

use super::*;
use crate::company::cash_dividend::{CashDividendBook, CashDividendPlan, CashDividendStatus};
use crate::company::cash_dividend_tax::CashDividendTaxMode;
use crate::company::config::CompanySystemConfig;
use crate::company::ex_reference_price::CashDividendFormula;
use crate::company::share_registry::HolderId;
use crate::company::{CompanyId, DividendDeclaration};
use crate::runtime::bridge_tests::setup_fixture;
use crate::{AccountId, Event, Intent, LimitPrice, Money, Side, StockId};

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
    let mut companies = simple_company_fixture!(crate; [STOCK]);
    let CompanySystemConfig::Simple(simple) = &mut companies else {
        unreachable!("fixture 显式选择 Simple")
    };
    // 覆盖 2029 完整自然年，保留真实法定公积金与年度利润校验。
    simple.prehistory_periods = 17;
    setup.company_system = Some(companies);
    setup.dividend_tax_mode = Some(mode);
    if mode == CashDividendTaxMode::Exempt {
        setup.config.stamp_tax_rate = 0.0;
    }
    setup.flat_withholding_bp = (mode == CashDividendTaxMode::FlatWithholding).then_some(1_000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.auto_corporate_foundation = true;

    let session = GameSession::new_with_runtime_groups(setup, 7, 3)
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
    let calendar = session.civil_clock().calendar();
    let ex_on = calendar
        .next_trading_day(crate::calendar::CalendarExchange::Sse, date(START))
        .expect("实际 Calendar 提供登记日下一交易日");
    assert_eq!(ex_on, date(PAYMENT_DAY));
    let plan = CashDividendPlan::new(
        PLAN_ID.into(),
        company,
        crate::StockCode(STOCK.into()),
        crate::calendar::CalendarExchange::Sse,
        CashDividendFormula::StandardCashOnly,
        date(START),
        date(START),
        date(START),
        ex_on,
        ex_on,
        Money::from_cents(GROSS_PER_SHARE_CENTS),
        Money::from_cents(1_200),
        calendar,
    )
    .expect("登记及除息、付款日期由真实交易日历校验");
    let mut saved = session.save().expect("开局事实可保存");
    // 只提供批准来源与新账簿；完整公共 restore 必须先校验，不预登记或预付款。
    saved
        .company_state
        .system_for_test_mut()
        .declare_dividend(
            &company,
            DividendDeclaration {
                plan_id: PLAN_ID.into(),
                approved_on: date(START),
                total_gross: crate::accounting::AccountingAmount::from_money(Money::from_cents(
                    1_200,
                )),
                registered_capital,
            },
        )
        .expect("使用真实法定注册资本批准分红");
    let actions = saved.company_state.actions_for_test_mut();
    actions
        .dividends
        .push(CashDividendBook::new(plan).expect("批准后的分红账簿可创建"));
    assert!(actions.dividends[0].registration().is_none());
    assert!(actions.dividends[0].payments().is_empty());
    restore_with_three_groups(&saved)
}

fn restore_with_three_groups(saved: &crate::SaveSlot) -> GameSession {
    let mut session =
        GameSession::restore(saved).expect("完整 fixture 必须先通过公共 strict restore");
    // group 数是运行资源配置，不入业务存档；在 lazy driver 前保留本 fixture 的三组拓扑。
    assert!(session.runtime_driver.is_none());
    session.runtime_group_count = 3;
    session
}

fn cash(session: &GameSession, account: AccountId) -> Money {
    session.account(account).expect("账户存在").cash()
}

fn advance_to_date(session: &mut GameSession, target: crate::CivilDate) {
    while session.civil_date() < target {
        let day = session.civil_date();
        if session
            .civil_clock()
            .calendar()
            .is_trading_day(crate::calendar::CalendarExchange::Sse, day)
            .expect("冻结 Calendar 必须能判定自然日")
        {
            session.step().expect("交易日执行 market step");
        }
        session.end_civil_day().expect("逐自然日日结");
    }
    assert_eq!(session.civil_date(), target);
}

fn advance_to_next_trading_day(session: &mut GameSession, previous: crate::CivilDate) {
    let next = session
        .civil_clock()
        .calendar()
        .next_trading_day(crate::calendar::CalendarExchange::Sse, previous)
        .expect("冻结 Calendar 必须提供下一交易日");
    advance_to_date(session, next);
}

fn register_on_june_3(session: &mut GameSession) {
    assert_eq!(session.civil_date(), date(START));
    session.step().expect("6 月 3 日真实 market step 完成");
    {
        let driver = session.runtime_driver.as_ref().unwrap().lock().unwrap();
        assert_eq!(driver.account_by_owner((1, 0)), Some(AccountId(3)));
        assert_eq!(driver.account_by_owner((2, 0)), Some(AccountId(6)));
        assert_eq!(
            driver.account_by_owner((2, 1)),
            None,
            "tail padding 不能冒充真实账户"
        );
    }
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
    let actions = session.corporate_actions().unwrap();
    assert!(actions.account_gross_receipts.is_empty());
    assert!(actions.flat_withholding_receipts.is_empty());
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
    assert_eq!(
        entitlements,
        RETAIL_HOLDERS.map(|account| (account, Money::from_cents(200)))
    );
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
        assert!(tax_book
            .payment_by_id(&format!("dividend-payment:{PLAN_ID}:{PAYMENT_DAY}"))
            .is_some());
        let serialized_tax_book = serde_json::to_value(tax_book).unwrap();
        assert_eq!(
            serialized_tax_book["dividends"][0]["payments"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(tax_book
            .collections()
            .iter()
            .all(|receipt| receipt.collected == Money::ZERO));
        assert_eq!(tax_book.outstanding_tax().unwrap().numerator(), 0);
    }

    let gross_receipts = actions.account_gross_receipts.clone();
    let payments = actions.dividends[0].payments().to_vec();
    let restored = restore_with_three_groups(&session.save().unwrap());
    let restored_before: Vec<_> = RETAIL_HOLDERS
        .iter()
        .map(|account| (*account, cash(&restored, *account)))
        .collect();
    let mut restored = restored;
    let next_market_date = restored
        .civil_clock()
        .calendar()
        .next_trading_day(crate::calendar::CalendarExchange::Sse, date(PAYMENT_DAY))
        .expect("恢复后使用本局冻结 Calendar 求下一真实交易日");
    while restored.civil_date() < next_market_date {
        let closed_date = restored.civil_date();
        assert!(!restored
            .civil_clock()
            .calendar()
            .is_trading_day(crate::calendar::CalendarExchange::Sse, closed_date)
            .expect("恢复后中间自然日的冻结 Calendar 判定有效"));
        let market_progress = (restored.tick(), restored.day());
        let report = restored
            .end_civil_day()
            .expect("恢复后中间休市日只推进真实自然日日结");
        assert_eq!(report.settled_date, closed_date);
        assert_eq!(report.next_date, closed_date.next().unwrap());
        assert_eq!((restored.tick(), restored.day()), market_progress);
    }
    assert_eq!(restored.civil_date(), next_market_date);
    restored
        .step()
        .expect("恢复后下一真实交易日 market step 完成");
    restored
        .end_civil_day()
        .expect("恢复后下一真实交易日日结完成");
    for (account, previous) in restored_before {
        assert_eq!(
            cash(&restored, account),
            previous,
            "恢复后不得重复支付同一分红"
        );
    }
    let restored_actions = restored.corporate_actions().unwrap();
    assert_eq!(restored_actions.account_gross_receipts, gross_receipts);
    assert_eq!(restored_actions.dividends[0].payments(), payments);
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
    for (account, previous) in &before {
        assert_eq!(
            cash(&session, *account).cents() - previous.cents(),
            180,
            "R4 简税付款必须在真实账户留下税后净额"
        );
    }
    let actions = session.corporate_actions().unwrap();
    assert_eq!(actions.account_gross_receipts.len(), RETAIL_HOLDERS.len());
    assert_eq!(
        actions.flat_withholding_receipts.len(),
        RETAIL_HOLDERS.len()
    );
    for account in RETAIL_HOLDERS {
        assert_eq!(
            actions
                .flat_withholding_receipts
                .iter()
                .filter(|receipt| receipt.account == account)
                .count(),
            1
        );
        assert_eq!(
            actions
                .account_gross_receipts
                .iter()
                .filter(|receipt| receipt.account == account)
                .count(),
            1
        );
    }
    for receipt in &actions.flat_withholding_receipts {
        assert_eq!(receipt.gross, Money::from_cents(200));
        assert_eq!(receipt.withheld, Money::from_cents(20));
        assert_eq!(receipt.rate_bp, 1_000);
        assert_eq!(receipt.plan_id, PLAN_ID);
        assert_eq!(receipt.paid_on, date(PAYMENT_DAY));
        assert_eq!(
            cash(&session, receipt.account).cents()
                - before
                    .iter()
                    .find(|row| row.0 == receipt.account)
                    .unwrap()
                    .1
                    .cents(),
            180,
            "实际净到账必须与对应代扣回执一致"
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
    let events = session.step().expect("付款日真实撮合完成");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(event,
                Event::Trade { code, maker, taker, qty: 100, price, .. }
                    if *code == StockId(0) && *maker == seller && *taker == buyer
                        && price.cents() == 1_000
            ))
            .count(),
        1,
        "两笔真实委托必须有唯一对应成交"
    );
    let seller_position = session
        .account(seller)
        .unwrap()
        .position(&StockId(0))
        .unwrap();
    assert_eq!(seller_position.qty(), 100);
    assert_eq!(seller_position.sellable(), before_sellable - 100);
    assert!(session
        .personal_trade_confirmations(seller)
        .unwrap()
        .iter()
        .any(|confirmation| confirmation.civil_date == date(PAYMENT_DAY)
            && confirmation.side == Side::Sell
            && confirmation.quantity_shares == 100));
    let seller_cash_before_payment = cash(&session, seller);
    let buyer_cash_before_payment = cash(&session, buyer);
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
    assert_eq!(
        cash(&session, buyer),
        buyer_cash_before_payment,
        "登记后首次买入者不得获得既有股利"
    );
    assert!(!actions.dividends[0]
        .entitlements()
        .unwrap()
        .iter()
        .any(|entitlement| entitlement.holder == HolderId::Account(buyer)));
    let seller_tax = actions
        .dividend_tax_books
        .iter()
        .find(|book| book.account() == seller)
        .expect("登记时已存在个人税账");
    assert_eq!(seller_tax.collections().len(), 1);
    assert_eq!(seller_tax.collections()[0].collected, Money::from_cents(20));
    assert_eq!(
        seller_tax.collections()[0].available_cash,
        seller_cash_before_payment
            .add(Money::from_cents(200))
            .unwrap()
    );
    assert_eq!(seller_tax.collections()[0].remaining_cash, after);
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

#[test]
fn r4_cash_04_final_projection_rejection_aborts_payment_and_company_candidates() {
    let mut session = payment_session(CashDividendTaxMode::AShareIndividual);
    register_on_june_3(&mut session);
    step_on_june_4(&mut session);
    let before_hash = session.business_state_hash().unwrap();
    let before_actions = session.corporate_actions().unwrap().clone();
    let before_cash = cash(&session, AccountId(1));
    let (before_runtime, before_raw_accounts) = {
        let mut driver = session.runtime_driver.as_ref().unwrap().lock().unwrap();
        (
            driver.administrative_cache_snapshot_for_test(),
            driver.world_accounts_snapshot().unwrap(),
        )
    };
    let mut inspect_calls = 0;
    let result: Result<(), SessionError> = session.with_prepared_runtime_civil_day(|shadow, _| {
        inspect_calls += 1;
        assert_eq!(
            cash(shadow, AccountId(1)),
            before_cash.add(Money::from_cents(200)).unwrap(),
            "拒绝点必须已持有真实 prepared 账户付款候选"
        );
        assert_eq!(
            shadow.corporate_actions().unwrap().dividends[0].status(),
            &CashDividendStatus::Paid
        );
        Err(SessionError::CivilInformation(
            super::super::administrative_mirror::CivilInformationError::new(
                date(PAYMENT_DAY),
                crate::runtime::RuntimeError::ProtocolInvariant {
                    detail: "R4 付款最终公开投影主动拒绝".into(),
                },
            ),
        ))
    });
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("R4 付款最终公开投影主动拒绝"));
    assert_eq!(inspect_calls, 1);
    assert_eq!(session.business_state_hash().unwrap(), before_hash);
    assert_eq!(session.corporate_actions().unwrap(), &before_actions);
    assert_eq!(cash(&session, AccountId(1)), before_cash);
    let mut driver = session.runtime_driver.as_ref().unwrap().lock().unwrap();
    assert!(
        !driver.administrative_world_is_terminal(),
        "业务投影拒绝应只 Abort 本次 Admin"
    );
    assert_eq!(
        driver.administrative_cache_snapshot_for_test(),
        before_runtime
    );
    assert_eq!(
        driver.world_accounts_snapshot().unwrap(),
        before_raw_accounts
    );
}

#[test]
fn rights_payment_commits_npc_debits_and_survives_restore() {
    use crate::company::rights_offering::{
        RightsOfferingBook, RightsOfferingDeclaration, RightsOfferingEventPlan, RightsOfferingMode,
        RightsSubscriptionStrategy,
    };
    let initial = payment_session(CashDividendTaxMode::FlatWithholding);
    let mut saved = initial.save().unwrap();
    saved.setup.rights_offering_enabled = true;
    let company = CompanyId(0);
    let capital = initial
        .company_system()
        .unwrap()
        .dividend_legal_facts(&company)
        .unwrap()
        .unwrap()
        .registered_capital;
    let mut plan = RightsOfferingEventPlan {
        event_id: "真实缴款测试".into(),
        approval_reference: "测试批准".into(),
        issuer: company,
        stock: crate::StockCode(STOCK.into()),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: date(START),
        announced_on: date(START),
        registered_on: date(START),
        payment_start_on: date(START),
        payment_deadline_on: date(START),
        ex_rights_on: date(START),
        settlement_on: date(START),
        listing_on: date(START),
        price_per_share: Money::from_cents(100),
        mode: RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 100_000,
        },
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    };
    plan.derive_schedule(initial.civil_clock().calendar(), plan.exchange, 1)
        .unwrap();
    saved
        .company_state
        .system_for_test_mut()
        .declare_rights_offering(
            &company,
            RightsOfferingDeclaration {
                event_id: plan.event_id.clone(),
                approval_reference: plan.approval_reference.clone(),
                approved_on: plan.approved_on,
                price_per_share: plan.price_per_share,
                par_value_per_share: Money::from_cents(100),
                registered_capital_at_approval: capital,
            },
        )
        .unwrap();
    saved
        .company_state
        .actions_for_test_mut()
        .rights_offerings
        .push(RightsOfferingBook::new(plan.clone()).unwrap());
    let mut session = restore_with_three_groups(&saved);
    register_on_june_3(&mut session);
    assert_eq!(session.civil_date(), plan.payment_start_on);
    session.step().unwrap();
    let before: Vec<_> = RETAIL_HOLDERS
        .iter()
        .map(|account| (*account, cash(&session, *account)))
        .collect();
    session.end_civil_day().unwrap();
    let book = &session.corporate_actions().unwrap().rights_offerings[0];
    assert_eq!(book.subscriptions().len(), RETAIL_HOLDERS.len());
    for record in book.subscriptions() {
        let HolderId::Account(account) = record.holder else {
            panic!("预期账户持有人")
        };
        let opening = before.iter().find(|row| row.0 == account).unwrap().1;
        // 同日现金分红先入账并代扣 10%，再以本人真实余额缴款。
        assert_eq!(
            cash(&session, account).cents(),
            opening.cents() + 180 - record.paid_amount.cents()
        );
    }
    let after = session.save().unwrap();
    let restored = restore_with_three_groups(&after);
    assert_eq!(
        restored.corporate_actions().unwrap().rights_offerings[0],
        session.corporate_actions().unwrap().rights_offerings[0]
    );
    for account in RETAIL_HOLDERS {
        assert_eq!(cash(&restored, account), cash(&session, account));
    }
}

#[test]
fn failed_rights_refunds_real_debits_and_restores_without_repayment() {
    use crate::company::rights_offering::{
        RightsOfferingBook, RightsOfferingDeclaration, RightsOfferingEventPlan, RightsOfferingMode,
        RightsSubscriptionStrategy,
    };
    let initial = payment_session(CashDividendTaxMode::FlatWithholding);
    let price = RETAIL_HOLDERS
        .iter()
        .map(|account| cash(&initial, *account).cents())
        .max()
        .unwrap()
        / 2
        + 181;
    let mut saved = initial.save().unwrap();
    saved.setup.rights_offering_enabled = true;
    let company = CompanyId(0);
    let capital = initial
        .company_system()
        .unwrap()
        .dividend_legal_facts(&company)
        .unwrap()
        .unwrap()
        .registered_capital;
    let mut plan = RightsOfferingEventPlan {
        event_id: "真实退款测试".into(),
        approval_reference: "测试批准".into(),
        issuer: company,
        stock: crate::StockCode(STOCK.into()),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: date(START),
        announced_on: date(START),
        registered_on: date(START),
        payment_start_on: date(START),
        payment_deadline_on: date(START),
        ex_rights_on: date(START),
        settlement_on: date(START),
        listing_on: date(START),
        price_per_share: Money::from_cents(price),
        mode: RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 100_000,
        },
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    };
    plan.derive_schedule(initial.civil_clock().calendar(), plan.exchange, 1)
        .unwrap();
    saved
        .company_state
        .system_for_test_mut()
        .declare_rights_offering(
            &company,
            RightsOfferingDeclaration {
                event_id: plan.event_id.clone(),
                approval_reference: plan.approval_reference.clone(),
                approved_on: plan.approved_on,
                price_per_share: plan.price_per_share,
                par_value_per_share: Money::from_cents(100),
                registered_capital_at_approval: capital,
            },
        )
        .unwrap();
    saved
        .company_state
        .actions_for_test_mut()
        .rights_offerings
        .push(RightsOfferingBook::new(plan.clone()).unwrap());
    let mut session = restore_with_three_groups(&saved);
    register_on_june_3(&mut session);
    assert_eq!(session.civil_date(), plan.payment_start_on);
    session.step().unwrap();
    let before: Vec<_> = RETAIL_HOLDERS
        .iter()
        .map(|account| (*account, cash(&session, *account)))
        .collect();
    session.end_civil_day().unwrap();
    let book = &session.corporate_actions().unwrap().rights_offerings[0];
    assert_eq!(book.subscriptions().len(), RETAIL_HOLDERS.len());
    for record in book.subscriptions() {
        let HolderId::Account(account) = record.holder else {
            panic!("预期账户持有人")
        };
        let opening = before.iter().find(|row| row.0 == account).unwrap().1;
        // 同日现金分红先入账并代扣 10%，再以本人真实余额缴款。
        assert_eq!(
            cash(&session, account).cents(),
            opening.cents() + 180 - record.paid_amount.cents()
        );
    }
    let paid: Vec<_> = book
        .subscriptions()
        .iter()
        .filter_map(|record| {
            let HolderId::Account(account) = record.holder else {
                return None;
            };
            (record.paid_amount > Money::ZERO).then_some((account, record.paid_amount))
        })
        .collect();
    assert!(!paid.is_empty());
    advance_to_date(&mut session, plan.settlement_on);
    session.step().unwrap();
    let before_refund: Vec<_> = paid
        .iter()
        .map(|(account, amount)| (*account, *amount, cash(&session, *account)))
        .collect();
    session.end_civil_day().unwrap();
    assert!(
        session.corporate_actions().unwrap().rights_offerings[0]
            .settlement()
            .unwrap()
            .failed
    );
    for (account, amount, before) in &before_refund {
        assert_eq!(
            cash(&session, *account).cents(),
            before.cents() + amount.cents()
        );
    }
    let saved = session.save().unwrap();
    let mut restored = restore_with_three_groups(&saved);
    for (account, _, _) in &before_refund {
        assert_eq!(cash(&restored, *account), cash(&session, *account));
    }
    advance_to_next_trading_day(&mut restored, plan.settlement_on);
    restored.step().unwrap();
    restored.end_civil_day().unwrap();
    for (account, _, _) in &before_refund {
        assert_eq!(cash(&restored, *account), cash(&session, *account));
    }
}

fn rights_session_at_ex_date() -> (
    GameSession,
    crate::company::rights_offering::RightsOfferingEventPlan,
) {
    use crate::company::rights_offering::{
        RightsOfferingBook, RightsOfferingDeclaration, RightsOfferingEventPlan, RightsOfferingMode,
        RightsSubscriptionStrategy,
    };
    let initial = payment_session(CashDividendTaxMode::Exempt);
    let mut saved = initial.save().unwrap();
    saved.setup.rights_offering_enabled = true;
    let company = CompanyId(0);
    let capital = initial
        .company_system()
        .unwrap()
        .dividend_legal_facts(&company)
        .unwrap()
        .unwrap()
        .registered_capital;
    let mut plan = RightsOfferingEventPlan {
        event_id: "真实缴款测试".into(),
        approval_reference: "测试批准".into(),
        issuer: company,
        stock: crate::StockCode(STOCK.into()),
        exchange: crate::calendar::CalendarExchange::Sse,
        approved_on: date(START),
        announced_on: date(START),
        registered_on: date(START),
        payment_start_on: date(START),
        payment_deadline_on: date(START),
        ex_rights_on: date(START),
        settlement_on: date(START),
        listing_on: date(START),
        price_per_share: Money::from_cents(100),
        mode: RightsOfferingMode::RightsToAllShareholders {
            shares_per_existing_share_micros: 100_000,
        },
        npc_subscription_strategy: RightsSubscriptionStrategy::FullByDefault,
    };
    plan.derive_schedule(initial.civil_clock().calendar(), plan.exchange, 1)
        .unwrap();
    saved
        .company_state
        .system_for_test_mut()
        .declare_rights_offering(
            &company,
            RightsOfferingDeclaration {
                event_id: plan.event_id.clone(),
                approval_reference: plan.approval_reference.clone(),
                approved_on: plan.approved_on,
                price_per_share: plan.price_per_share,
                par_value_per_share: Money::from_cents(100),
                registered_capital_at_approval: capital,
            },
        )
        .unwrap();
    saved
        .company_state
        .actions_for_test_mut()
        .rights_offerings
        .push(RightsOfferingBook::new(plan.clone()).unwrap());
    let mut session = restore_with_three_groups(&saved);
    register_on_june_3(&mut session);
    assert_eq!(session.civil_date(), plan.payment_start_on);
    session.step().unwrap();
    let before: Vec<_> = RETAIL_HOLDERS
        .iter()
        .map(|account| (*account, cash(&session, *account)))
        .collect();
    session.end_civil_day().unwrap();
    let book = &session.corporate_actions().unwrap().rights_offerings[0];
    assert_eq!(book.subscriptions().len(), RETAIL_HOLDERS.len());
    for record in book.subscriptions() {
        let HolderId::Account(account) = record.holder else {
            panic!("预期账户持有人")
        };
        let opening = before.iter().find(|row| row.0 == account).unwrap().1;
        // 同日现金分红先入账，再以本人真实余额缴款。
        assert_eq!(
            cash(&session, account).cents(),
            opening.cents() + 200 - record.paid_amount.cents()
        );
    }
    advance_to_date(&mut session, plan.ex_rights_on);
    assert_eq!(session.civil_date(), plan.ex_rights_on);
    (session, plan)
}

#[test]
fn exempt_rights_ex_anchor_is_committed_to_desk_and_restored() {
    use crate::company::rights_offering::{RightsOfferingBook, RightsOfferingDeclaration};
    let (session, plan) = rights_session_at_ex_date();
    let company = CompanyId(0);
    let group = session
        .corporate_actions()
        .unwrap()
        .applied_ex_reference_groups
        .last()
        .unwrap();
    assert_eq!(group.rights_event_ids, vec![plan.event_id.clone()]);
    assert_eq!(
        session.state.markets[&StockId(0)].last_close(),
        group.reference.reference_price
    );
    let after = session.save().unwrap();
    let mut restored = restore_with_three_groups(&after);
    assert_eq!(
        restored.runtime_market_seeds()[&StockId(0)].last_close,
        group.reference.reference_price.cents()
    );
    let mut traded = after.clone();
    traded
        .snapshot
        .markets
        .get_mut(&StockId(0))
        .unwrap()
        .last_price = Money::from_cents(group.reference.reference_price.cents() + 2);
    assert!(super::super::persistence::rights_ex_reference::validate(
        &traded,
        &session.state.stocks
    )
    .is_ok());
    let mut changed_opening_float = after.clone();
    changed_opening_float.setup.stocks[0].float_shares += 1;
    assert!(super::super::persistence::rights_ex_reference::validate(
        &changed_opening_float,
        &session.state.stocks
    )
    .is_ok());
    let mut changed_anchor = traded.clone();
    changed_anchor
        .company_state
        .actions_for_test_mut()
        .applied_ex_reference_groups
        .last_mut()
        .unwrap()
        .reference
        .reference_price = Money::from_cents(group.reference.reference_price.cents() + 1);
    assert!(super::super::persistence::rights_ex_reference::validate(
        &changed_anchor,
        &session.state.stocks
    )
    .is_err());
    let mut changed_pre_close = after.clone();
    changed_pre_close
        .company_state
        .actions_for_test_mut()
        .applied_ex_reference_groups
        .last_mut()
        .unwrap()
        .pre_ex_close = Money::from_cents(1);
    assert!(GameSession::restore(&changed_pre_close).is_err());
    let mut tampered = after.clone();
    tampered
        .snapshot
        .markets
        .get_mut(&StockId(0))
        .unwrap()
        .last_close = Money::from_cents(1);
    assert!(GameSession::restore(&tampered).is_err());
    assert_eq!(
        restored.corporate_actions().unwrap().rights_offerings[0],
        session.corporate_actions().unwrap().rights_offerings[0]
    );
    for account in RETAIL_HOLDERS {
        assert_eq!(cash(&restored, account), cash(&session, account));
    }
    advance_to_date(&mut restored, plan.settlement_on);
    assert_eq!(restored.civil_date(), plan.settlement_on);
    restored.step().unwrap();
    let before: Vec<_> = RETAIL_HOLDERS
        .iter()
        .map(|id| {
            let account = restored.account(*id).unwrap();
            (
                *id,
                account.cash(),
                account.position(&StockId(0)).unwrap().clone(),
            )
        })
        .collect();
    restored.end_civil_day().unwrap();
    let book = &restored.corporate_actions().unwrap().rights_offerings[0];
    assert!(!book.settlement().unwrap().failed);
    for record in book.subscriptions() {
        let HolderId::Account(account) = record.holder else {
            panic!("预期账户持有人")
        };
        let (_, old_cash, old_position) = before.iter().find(|row| row.0 == account).unwrap();
        let actual = restored.account(account).unwrap();
        assert_eq!(actual.cash(), *old_cash);
        let position = actual.position(&StockId(0)).unwrap();
        assert_eq!(
            u64::from(position.qty()),
            u64::from(old_position.qty()) + record.paid_shares
        );
        assert_eq!(
            position.invested_cents(),
            old_position.invested_cents() + record.paid_amount.cents()
        );
    }
    let saved = restored.save().unwrap();
    let mut continued = restore_with_three_groups(&saved);
    advance_to_next_trading_day(&mut continued, plan.settlement_on);
    continued.step().unwrap();
    continued.end_civil_day().unwrap();
    for account in RETAIL_HOLDERS {
        let before = restored.account(account).unwrap();
        let after = continued.account(account).unwrap();
        assert_eq!(after.cash(), before.cash());
        assert_eq!(
            after.position(&StockId(0)).unwrap().qty(),
            before.position(&StockId(0)).unwrap().qty()
        );
        assert_eq!(
            after.position(&StockId(0)).unwrap().invested_cents(),
            before.position(&StockId(0)).unwrap().invested_cents()
        );
    }
    let next_registration = continued
        .civil_clock()
        .calendar()
        .next_trading_day(
            crate::calendar::CalendarExchange::Sse,
            continued.civil_date(),
        )
        .unwrap();
    advance_to_date(&mut continued, next_registration);
    let mut second = plan.clone();
    second.event_id = "第二次真实配股".into();
    second.approved_on = continued.civil_date();
    second.announced_on = second.approved_on;
    second.registered_on = second.approved_on;
    second
        .derive_schedule(continued.civil_clock().calendar(), second.exchange, 1)
        .unwrap();
    let capital = continued
        .company_system()
        .unwrap()
        .dividend_legal_facts(&company)
        .unwrap()
        .unwrap()
        .registered_capital;
    let mut second_save = continued.save().unwrap();
    second_save
        .company_state
        .system_for_test_mut()
        .declare_rights_offering(
            &company,
            RightsOfferingDeclaration {
                event_id: second.event_id.clone(),
                approval_reference: second.approval_reference.clone(),
                approved_on: second.approved_on,
                price_per_share: second.price_per_share,
                par_value_per_share: Money::from_cents(100),
                registered_capital_at_approval: capital,
            },
        )
        .unwrap();
    second_save
        .company_state
        .actions_for_test_mut()
        .rights_offerings
        .push(RightsOfferingBook::new(second.clone()).unwrap());
    let mut second_session = restore_with_three_groups(&second_save);
    advance_to_date(&mut second_session, second.ex_rights_on);
    let second_group = second_session
        .corporate_actions()
        .unwrap()
        .applied_ex_reference_groups
        .last()
        .unwrap();
    assert_eq!(second_group.pre_ex_close, Money::from_cents(918));
    assert_eq!(
        second_group.reference.reference_price,
        Money::from_cents(844)
    );
    assert_eq!(
        second_session.state.markets[&StockId(0)].last_price(),
        Money::from_cents(1000)
    );
    let second_save = second_session.save().unwrap();
    let second_restored = restore_with_three_groups(&second_save);
    assert_eq!(
        second_restored.state.markets[&StockId(0)].last_close(),
        Money::from_cents(844)
    );
}

#[test]
fn rights_ex_day_close_after_trade_restores() {
    let (session, _) = rights_session_at_ex_date();
    let group = session
        .corporate_actions()
        .unwrap()
        .applied_ex_reference_groups
        .last()
        .unwrap();
    let after = session.save().unwrap();
    let mut closed = restore_with_three_groups(&after);
    let trade_price = Money::from_cents(group.reference.reference_price.cents() + 2);
    closed.seed_order_for_test(
        AccountId(1),
        Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Sell,
            price: LimitPrice::Fixed(trade_price),
            qty: 100,
        },
        &mut Vec::new(),
    );
    closed
        .enqueue_player_intent(
            AccountId(0),
            Intent::PlaceLimit {
                code: StockId(0),
                side: Side::Buy,
                price: LimitPrice::Fixed(trade_price),
                qty: 100,
            },
        )
        .unwrap();
    let events = closed.step().unwrap();
    assert!(events
        .iter()
        .any(|event| matches!(event, Event::Trade { price, .. } if *price == trade_price)));
    let closed_save = closed.save().unwrap();
    assert_eq!(
        closed_save.snapshot.markets[&StockId(0)].last_close,
        trade_price
    );
    let closed_restored = restore_with_three_groups(&closed_save);
    assert_eq!(
        closed_restored.state.markets[&StockId(0)].last_close(),
        trade_price
    );
}

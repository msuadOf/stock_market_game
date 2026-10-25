//! #118 回归：回购专户即使持有可用现金和股份，也不能像 NPC 一样自主交易。

use super::bridge_tests::setup_fixture;
use super::drive::RuntimeDriver;
use crate::account::Position;
use crate::{AccountId, AccountKind, Event, GameSession, Money, StockId};

include!("../../test-support/simple_company.rs");

const RETAIL_COUNT: u32 = 8;
const REPURCHASE_ACCOUNT: AccountId = AccountId(RETAIL_COUNT as u64 + 1);
const STOCK: StockId = StockId(0);
const REPURCHASE_CASH_CENTS: i64 = 20_000_000;
const REPURCHASE_SHARES: u32 = 100;
const CONTINUOUS_TICKS: u64 = 32;

fn funded_repurchase_session() -> GameSession {
    let mut setup = setup_fixture();
    setup.stocks.truncate(1);
    setup.stocks[0].float_shares = 10_000;
    setup.npcs.retail_count = RETAIL_COUNT;
    setup.npcs.retail_cash_median = Money::from_cents(REPURCHASE_CASH_CENTS);
    setup.strategy_params.retail.arrival_rate = 1.0;
    setup.company_system = Some(simple_company_fixture!(crate; ["000001"]));
    setup.dividend_tax_mode = Some(crate::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1_000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.issuer_repurchase_enabled = true;
    let mut session = GameSession::new(setup, 42).expect("含回购专户的真实会话构造成功");

    // 从已有 NPC 转移股份，避免以零余额掩盖自主买卖，也不额外制造流通股份。
    let donor = (1..=u64::from(RETAIL_COUNT))
        .map(AccountId)
        .find(|id| {
            session
                .account(*id)
                .and_then(|account| account.position(&STOCK))
                .is_some_and(|position| position.sellable() >= REPURCHASE_SHARES)
        })
        .expect("至少一个散户有足额可卖股份供夹具转移");
    let position = session
        .account(donor)
        .and_then(|account| account.position(&STOCK))
        .expect("出让散户持仓存在")
        .clone();
    let transferred_cost = i64::from(REPURCHASE_SHARES) * 1_000;
    session
        .fixture_account_mut(&donor)
        .expect("散户账户存在")
        .fixture_insert_position(
            STOCK,
            Position::from_restored_parts(
                position.qty() - REPURCHASE_SHARES,
                0,
                position.invested_cents() - transferred_cost,
                position.recovered_cents(),
            ),
        );
    let account = session
        .fixture_account_mut(&REPURCHASE_ACCOUNT)
        .expect("回购专户由新局正式开立");
    assert_eq!(account.kind(), AccountKind::IssuerRepurchase);
    assert!(account.strategy().is_none(), "回购专户没有自主策略");
    account.fixture_set_cash(Money::from_cents(REPURCHASE_CASH_CENTS));
    account.fixture_insert_position(
        STOCK,
        Position::from_restored_parts(REPURCHASE_SHARES, 0, transferred_cost, 0),
    );
    session
}

#[test]
fn repurchase_profile_preserves_kind_and_disables_autonomous_schedule() {
    let session = funded_repurchase_session();
    let ids = session.account_roster();
    let profiles =
        RuntimeDriver::profiles_from_session(&session, &session.export_setup_for_runtime(), &ids);
    for (id, profile) in ids.iter().zip(&profiles) {
        let account = session.account(*id).expect("档案账户存在");
        if account.kind() == AccountKind::IssuerRepurchase {
            assert_eq!(
                profile.strategy.kind,
                AccountKind::IssuerRepurchase,
                "专户不得被投影成零售策略"
            );
            assert_eq!(profile.next_due, u64::MAX, "专户永不进入自主调度");
            assert!(profile.retail_style.is_none(), "专户没有零售风格");
        } else if account.kind() == AccountKind::Retail {
            let expected_strategy = RuntimeDriver::individual_strategy_data(
                account.strategy().expect("普通散户保有个体策略"),
            )
            .expect("普通散户个体策略可投影");
            assert_eq!(
                format!("{:?}", profile.strategy),
                format!("{expected_strategy:?}")
            );
            let (_, due, rng) = session
                .runtime_attention_state()
                .into_iter()
                .find(|(owner, _, _)| owner == id)
                .expect("普通散户 attention 存在");
            assert_eq!(profile.next_due, due, "普通 NPC 的到期状态不受影响");
            assert_eq!(profile.seed, rng, "普通 NPC 的注意力随机流不受影响");
        }
    }
}

#[test]
fn repurchase_attention_entry_cannot_override_disabled_schedule() {
    let mut session = funded_repurchase_session();
    let personal = session
        .runtime_personal_states()
        .into_values()
        .next()
        .expect("普通 NPC 的个人态可用于异常注入");
    let mut snapshot = super::world::BeliefParticipantSnapshot {
        attention: personal.attention,
        watchlist: personal.watchlist,
        price_memory: personal.price_memory,
        history_reads: personal.history_reads,
        information: personal.information,
        belief: personal.belief,
        retail_experience: None,
    };
    snapshot.attention.next_attention_candidate_tick = 1;
    snapshot.attention.base_probability = 1.0;
    // 此处人为注入异常 attention，单独保护 Some 分支优先级，不冒充新局状态。
    session.set_runtime_personal_state(REPURCHASE_ACCOUNT, &snapshot);
    assert!(
        session
            .runtime_attention_state()
            .iter()
            .any(|(id, due, _)| { *id == REPURCHASE_ACCOUNT && *due == 1 }),
        "异常注入确实建立了下一拍到期的专户 attention"
    );
    let ids = session.account_roster();
    let profiles =
        RuntimeDriver::profiles_from_session(&session, &session.export_setup_for_runtime(), &ids);
    let index = ids
        .iter()
        .position(|id| *id == REPURCHASE_ACCOUNT)
        .expect("专户在名册中");
    assert_eq!(
        profiles[index].next_due,
        u64::MAX,
        "attention Some 不得覆盖专户禁调度"
    );
    assert_eq!(profiles[index].strategy.kind, AccountKind::IssuerRepurchase);
}

#[test]
fn repurchase_account_still_accepts_explicit_external_plan() {
    let mut session = funded_repurchase_session();
    let opening_ticks = session.export_setup_for_runtime().auction_ticks;
    let mut driver = RuntimeDriver::from_session(&mut session).expect("驱动器构造成功");
    driver.adopt_session_seq(&session);
    let retail_ids: Vec<_> = (1..=u64::from(RETAIL_COUNT)).map(AccountId).collect();
    driver.defer_due_for_test_at(&retail_ids, u64::MAX);
    for _ in 0..opening_ticks {
        driver
            .step_frame_equivalent(&mut session)
            .expect("推进至连续交易");
    }
    let upper: Vec<_> = driver
        .world_public_views()
        .iter()
        .map(|view| view.daily_upper_limit)
        .collect();
    // 此处只验证既有 ExternalPlan 运输，不伪装已接入完整回购方案执行器。
    driver
        .inject_player_intents(
            &[crate::session::ReceiptBearingIntent {
                owner: REPURCHASE_ACCOUNT,
                intent: crate::Intent::PlaceLimit {
                    code: STOCK,
                    side: crate::Side::Buy,
                    price: crate::strategy::LimitPrice::Fixed(Money::from_cents(1_000)),
                    qty: 100,
                },
                account_ordinal: 0,
                stock_ordinal: 0,
            }],
            &upper,
        )
        .expect("专户显式委托经 ExternalPlan 投递");
    let events = driver
        .step_frame_equivalent(&mut session)
        .expect("显式委托拍成功提交");
    assert_eq!(
        events
            .iter()
            .filter(|event| matches!(
                event,
                Event::OrderAccepted { account, side: crate::Side::Buy, remaining_qty: 100, .. }
                    if *account == REPURCHASE_ACCOUNT
            ))
            .count(),
        1,
        "禁自主调度不能阻断专户的唯一显式买入委托"
    );
    let index = driver
        .id_map
        .iter()
        .position(|id| *id == REPURCHASE_ACCOUNT)
        .expect("专户在驱动器名册");
    let attention = driver.collect_attention().expect("读取组权威 attention");
    let (_, due, _) = attention[0]
        .1
        .iter()
        .find(|(local, _, _)| *local as usize == index)
        .expect("专户组档案存在");
    assert_eq!(*due, u64::MAX, "显式委托不激活自主调度");
}

fn is_account_order_event(event: &Event, owner: AccountId) -> bool {
    match event {
        Event::OrderAccepted { account, .. }
        | Event::OrderCanceled { account, .. }
        | Event::IntentRejected { account, .. }
        | Event::SettlementError { account, .. } => *account == owner,
        Event::Trade { maker, taker, .. } => *maker == owner || *taker == owner,
        _ => false,
    }
}

#[test]
fn funded_repurchase_account_never_trades_autonomously_across_due_cycles() {
    assert!(
        super::step_runtime_enabled(),
        "测试必须走真实组线程生产路径"
    );
    let mut session = funded_repurchase_session();
    assert!(
        session
            .runtime_attention_state()
            .iter()
            .all(|(id, _, _)| *id != REPURCHASE_ACCOUNT),
        "正式新局专户没有 attention，不得靠人为覆盖制造复现"
    );
    assert!(
        session
            .corporate_actions()
            .expect("公司行为容器存在")
            .issuer_repurchases
            .is_empty(),
        "测试前没有任何回购方案"
    );
    let opening_ticks = session.export_setup_for_runtime().auction_ticks;
    let total_ticks = opening_ticks + CONTINUOUS_TICKS;
    let mut repurchase_reports_per_tick = Vec::new();
    let mut retail_orders = 0;

    // 不注入任何外部委托或回购方案；完整跑过开盘窗和 32 个连续交易拍。
    for expected_tick in 1..=total_ticks {
        let frame = session.step_frame().expect("真实交易拍成功提交");
        assert_eq!(session.tick(), expected_tick, "不得用空转冒充真实交易拍");
        let events: Vec<&Event> = frame.events().collect();
        repurchase_reports_per_tick.push(
            events
                .iter()
                .filter(|event| is_account_order_event(event, REPURCHASE_ACCOUNT))
                .count(),
        );
        retail_orders += events
            .iter()
            .filter(|event| {
                matches!(event, Event::OrderAccepted { account, .. } if (1..=u64::from(RETAIL_COUNT)).contains(&account.0))
            })
            .count();
    }

    eprintln!(
        "真实交易拍={total_ticks}，连续交易拍={CONTINUOUS_TICKS}，普通 NPC 受理单={retail_orders}，回购专户逐拍上报={repurchase_reports_per_tick:?}"
    );
    assert!(retail_orders > 0, "普通 NPC 正控必须产生真实受理委托");
    assert_eq!(
        repurchase_reports_per_tick,
        vec![0; total_ticks as usize],
        "无回购方案时，非零现金和可卖股份不能使回购专户在任何一拍产生自主委托或成交"
    );
    let account = session.account(REPURCHASE_ACCOUNT).expect("回购专户仍存在");
    assert_eq!(account.cash().cents(), REPURCHASE_CASH_CENTS);
    assert_eq!(
        account.position(&STOCK).expect("专户持仓仍在").qty(),
        REPURCHASE_SHARES
    );
}

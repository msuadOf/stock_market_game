//! #119：真实生产日终账户交付与 R1 名册候选的短接缝验证。
//! R1 尚未接入生产公司日终链；这里显式传递生产事实，不宣称 R4 已接线。

use super::*;
use crate::account::{Account, AccountKind};
use crate::company::share_registry::{AcquisitionSource, ShareHolding, ShareLot, ShareRestriction};
use crate::company::CompanyId;
use crate::runtime::world::DayEndCandidate;
use crate::session::{GameSession, ReceiptBearingIntent};
use crate::{Event, Intent, Money, Side, TradingPhase};
use std::sync::{Arc, Mutex};

const TARGET: AccountId = AccountId(30106);
const STOCK: StockId = StockId(0);
const INITIAL_QTY: u32 = 1200;
const ORDINARY_QTY: u32 = 100;
const CLOSING_QTY: u32 = 300;
const PRICE_CENTS: i64 = 1000;
const IDS: [AccountId; 5] = [
    AccountId(0),
    AccountId(1),
    AccountId(2),
    AccountId(3),
    TARGET,
];
type Positions = BTreeMap<AccountId, BTreeMap<StockId, u64>>;
type Confirmations = BTreeMap<AccountId, Vec<PersonalTradeConfirmation>>;

fn fixture() -> (GameSession, SessionCorporateActions) {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(1);
    setup.npcs.retail_count = 3;
    setup.npcs.inst_count = 0;
    setup.ticks_per_day = 6;
    setup.closing_auction_ticks = 2;
    setup.stocks[0].total_shares = u64::from(INITIAL_QTY) * IDS.len() as u64;
    setup.stocks[0].float_shares = INITIAL_QTY * IDS.len() as u32;
    let mut session = GameSession::new_with_runtime_groups(setup, 119, 3).unwrap();
    // 临时上界只引导现有生产装配器生成稀疏 ID，不创建三万个空账户。
    let sentinel = AccountId(TARGET.0 - 1);
    session.mark_external_mutation();
    session.accounts_insert_with_history_reads(
        sentinel,
        Account::new(sentinel, AccountKind::Player, Money::ZERO),
    );
    session.state.setup.npcs.retail_count = 1;
    session.populate_npcs(AccountKind::Retail).unwrap();
    session.state.accounts = session
        .state
        .accounts
        .iter()
        .filter(|(id, _)| **id != sentinel)
        .map(|(id, account)| (*id, account.clone()))
        .collect();
    assert!(session.state.history_reads.remove(&sentinel).is_some());
    session.state.setup.npcs.retail_count = 4;
    // 只装配开局事实；两笔成交后的持仓与确认均由真实生产链产生。
    for id in IDS {
        let account = session.fixture_account_mut(&id).unwrap();
        account.fixture_clear_positions();
        account.fixture_set_cash(Money::from_cents(10_000_000));
        account
            .grant_position(STOCK, INITIAL_QTY, Money::from_cents(PRICE_CENTS))
            .unwrap();
    }
    session.initialize_retail_experience().unwrap();
    assert_eq!(session.account_count(), IDS.len());
    assert_eq!(
        session
            .state
            .accounts
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        IDS.into()
    );
    assert_eq!(
        session
            .state
            .history_reads
            .keys()
            .copied()
            .collect::<BTreeSet<_>>(),
        IDS.into()
    );
    assert_eq!(
        session
            .state
            .accounts
            .values()
            .filter(|a| a.kind() == AccountKind::Retail)
            .count(),
        session.state.setup.npcs.retail_count as usize
    );
    for id in IDS.into_iter().filter(|id| id.0 != 0) {
        let account = session.account(id).unwrap();
        assert_eq!(account.kind(), AccountKind::Retail);
        assert!(account.strategy().is_some());
        session
            .state
            .npc_attention
            .get_mut(&id)
            .unwrap()
            .next_attention_candidate_tick = 100;
        let participant = session.state.belief_participants.get(&id).unwrap();
        assert_eq!(
            participant.information().owner(),
            id,
            "生产装配的个人身份必须保真"
        );
        assert!(session.runtime_retail_experience_has(id, &STOCK));
    }
    session.state.runtime_heap_dirty = true;
    let settled_on = CivilDate::from_iso("2030-01-01").unwrap();
    let registry = ShareRegistry::new(
        StockCode(session.stock_catalog().code(STOCK).unwrap().to_owned()),
        CompanyId(0),
        u64::from(INITIAL_QTY) * IDS.len() as u64,
        settled_on,
        IDS.into_iter()
            .map(|id| ShareHolding {
                holder: HolderId::Account(id),
                lots: vec![ShareLot {
                    id: format!("initial:{}", id.0),
                    qty: u64::from(INITIAL_QTY),
                    acquired_on: settled_on,
                    source: AcquisitionSource::InitialAllocation {
                        evidence: "#119 短 fixture 开局分配".into(),
                    },
                    restriction: ShareRestriction::Unrestricted,
                }],
            })
            .collect(),
    )
    .unwrap();
    (
        session,
        SessionCorporateActions {
            registries: vec![registry],
            ..Default::default()
        },
    )
}

fn inject_pair(session: &GameSession, seller: AccountId, buyer: AccountId, qty: u32) {
    let handle = session.runtime_driver_for_test().unwrap();
    let mut driver = handle.lock().unwrap();
    let upper: Vec<_> = driver
        .world_public_views()
        .iter()
        .map(|v| v.daily_upper_limit)
        .collect();
    let intents: Vec<_> = [(seller, Side::Sell), (buyer, Side::Buy)]
        .into_iter()
        .map(|(owner, side)| ReceiptBearingIntent {
            owner,
            intent: Intent::PlaceLimit {
                code: STOCK,
                side,
                price: crate::strategy::LimitPrice::Fixed(Money::from_cents(PRICE_CENTS)),
                qty,
            },
            account_ordinal: 0,
            stock_ordinal: 0,
        })
        .collect();
    driver.inject_player_intents(&intents, &upper).unwrap();
}

fn positions(session: &GameSession) -> Positions {
    // pre-D0 的 runtime_accounts_snapshot 只遍历连续 ID；该既有缺陷由 D0 同批修复。
    // 这里沿生产 bridge 使用的真实名册读取，隔离稀疏快照缺陷以触达收盘镜像断言。
    let roster = session.account_roster();
    assert_eq!(roster.iter().copied().collect::<BTreeSet<_>>(), IDS.into());
    assert_eq!(roster.len(), IDS.len());
    roster
        .into_iter()
        .map(|id| {
            let account = session.account(id).expect("名册中的真实账户必须存在");
            (
                id,
                account
                    .positions()
                    .iter()
                    .map(|(stock, pos)| (*stock, u64::from(pos.qty())))
                    .collect(),
            )
        })
        .collect()
}

fn confirmations(session: &GameSession) -> Confirmations {
    IDS.into_iter()
        .filter_map(|id| {
            session
                .personal_trade_confirmations(id)
                .map(|rows| (id, rows))
        })
        .collect()
}

fn assert_trade(
    events: &[Event],
    seller: AccountId,
    buyer: AccountId,
    qty: u32,
) -> u64 {
    // Event::Trade 的 maker/taker 已是按生产 id_map 投影出的真实 AccountId；
    // 稀疏名册断言直接核对真实卖方与买方，防止把 Owner 编码当账户身份。
    let matches: Vec<_> = events
        .iter()
        .filter_map(|event| match event {
            Event::Trade {
                seq,
                code,
                price,
                qty: actual,
                maker,
                taker,
            } if *code == STOCK
                && *maker == seller
                && *taker == buyer
                && *actual == qty
                && price.cents() == PRICE_CENTS =>
            {
                Some(*seq)
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        matches.len(),
        1,
        "真实成交必须恰有一笔，不能靠手写确认制造名册增量"
    );
    matches[0]
}

fn assert_confirmation(
    session: &GameSession,
    owner: AccountId,
    side: Side,
    qty: u32,
    seq: u64,
    day: CivilDate,
) {
    let rows = session.personal_trade_confirmations(owner).unwrap();
    assert_eq!(rows.len(), 1, "每个成交账户只有一腿确认");
    let row = &rows[0];
    assert_eq!(row.receipt_id, seq * 2 + u64::from(side == Side::Buy));
    assert_eq!(row.civil_date, day, "closing 确认必须属于刚结束的自然日");
    assert_eq!(row.stock, STOCK);
    assert_eq!(row.side, side);
    assert_eq!(row.quantity_shares, qty);
    assert_eq!(row.price_cents, PRICE_CENTS);
    assert_eq!(row.gross_cents, i64::from(qty) * PRICE_CENTS);
}

fn assert_final_accounts(session: &GameSession, candidate: &DayEndCandidate) {
    let handle = session.runtime_driver_for_test().unwrap();
    let driver = handle.lock().unwrap();
    assert_eq!(candidate.groups.len(), 3);
    assert_eq!(driver.accounts_per_group(), 2);
    let mut seen = BTreeSet::new();
    for (group, accounts) in &candidate.groups {
        assert_eq!(accounts.len(), 2);
        for (local, authority) in accounts.iter().enumerate() {
            let id = driver.account_by_owner((*group, local as u32));
            if (*group, local) == (2, 1) {
                assert_eq!(id, None, "非满尾组空槽没有账户身份");
                assert_eq!(authority.cash, 0);
                assert_eq!(authority.shares[0], 0);
                continue;
            }
            let id = id.expect("五个真实账户均有组内映射");
            assert!(seen.insert(id), "不能重复映射账户");
            let account = session.account(id).unwrap();
            assert_eq!(
                account.cash().cents(),
                authority.cash + authority.buy_purse_held_cents,
                "{id:?} 现金"
            );
            assert_eq!(authority.pending_sell_qty[0], 0, "Drain 后没有在途卖股");
            assert_eq!(authority.buy_purse_held_cents, 0, "Drain 后没有在途买款");
            let pos = account.position(&STOCK).unwrap();
            assert_eq!(
                pos.qty(),
                authority.shares[0],
                "{id:?} 日终返回时镜像必须立即等于 GroupDayDone 最终账户"
            );
            assert_eq!(pos.t1_locked(), authority.frozen_today[0], "{id:?} T+1");
            assert_eq!(pos.t1_locked(), 0, "日终统一解冻，不把冻结股再加一次");
            assert_eq!(pos.invested_cents(), authority.invested_cents[0]);
            assert_eq!(pos.recovered_cents(), authority.recovered_cents[0]);
        }
    }
    assert_eq!(seen, IDS.into());
    assert_eq!(driver.account_by_owner((2, 0)), Some(TARGET));
    assert_eq!(
        candidate
            .groups
            .iter()
            .flat_map(|(_, a)| a)
            .map(|a| u64::from(a.shares[0]))
            .sum::<u64>(),
        6000
    );
}

#[test]
fn issue119_closing_1200_to_1500_reconciles_real_day_end_accounts_confirmations_and_registry() {
    let (mut session, foundation) = fixture();
    let day = session.civil_date();
    assert_eq!(day, CivilDate::from_iso("2030-01-02").unwrap());
    assert_eq!(positions(&session)[&TARGET][&STOCK], 1200);
    session.step_via_runtime().unwrap();
    let candidates = Arc::new(Mutex::new(Vec::new()));
    let out = Arc::clone(&candidates);
    session
        .runtime_driver_for_test()
        .unwrap()
        .lock()
        .unwrap()
        .set_day_end_validator_for_test(Box::new(move |c| {
            out.lock().unwrap().push(c.clone());
            Ok(())
        }));
    session.step_via_runtime().unwrap();
    inject_pair(&session, AccountId(0), AccountId(1), ORDINARY_QTY);
    let ordinary = session.step_via_runtime().unwrap();
    let ordinary_seq = assert_trade(
        &ordinary,
        AccountId(0),
        AccountId(1),
        ORDINARY_QTY,
    );
    assert_eq!(
        session
            .account(AccountId(1))
            .unwrap()
            .position(&STOCK)
            .unwrap()
            .qty(),
        INITIAL_QTY + ORDINARY_QTY
    );
    assert_eq!(
        session
            .account(AccountId(1))
            .unwrap()
            .position(&STOCK)
            .unwrap()
            .t1_locked(),
        ORDINARY_QTY,
        "普通买入在日终前确实冻结，不能绕过 T+1"
    );
    session.step_via_runtime().unwrap();
    let pre_closing = positions(&session);
    assert_eq!(pre_closing[&TARGET][&STOCK], 1200);
    assert!(session.personal_trade_confirmations(TARGET).is_none());
    inject_pair(&session, AccountId(3), TARGET, CLOSING_QTY);
    let auction_entry = session.step_via_runtime().unwrap();
    assert!(
        !auction_entry
            .iter()
            .any(|event| matches!(event, Event::Trade { .. })),
        "closing 申报先入簿，不能当连续撮合"
    );
    assert_eq!(positions(&session)[&TARGET][&STOCK], 1200);
    let closing = session.step_via_runtime().unwrap();
    let closing_seq = assert_trade(&closing, AccountId(3), TARGET, CLOSING_QTY);
    assert!(closing.iter().any(|event| matches!(
        event,
        Event::AuctionCompleted {
            phase: TradingPhase::ClosingAuction,
            matched_volume: 300,
            ..
        }
    )));
    assert_eq!(candidates.lock().unwrap().len(), 1);
    let candidate = candidates.lock().unwrap()[0].clone();
    let tail = &candidate
        .groups
        .iter()
        .find(|(group, _)| *group == 2)
        .unwrap()
        .1;
    assert_eq!(
        tail[0].shares[0], 1500,
        "GroupDayDone 尾组目标账户已真实收到 closing300"
    );
    assert_eq!((candidate.day, candidate.tick), (0, 6));
    eprintln!(
        "#119 日期 {day} AccountId(30106)：GroupDayDone={}，Session={}",
        tail[0].shares[0],
        positions(&session)[&TARGET][&STOCK]
    );
    assert_eq!(
        positions(&session)[&TARGET][&STOCK],
        1500,
        "旧 #119 AccountId(30106) 的 1200/1500 差额必须在当前日结返回前消失"
    );
    assert_final_accounts(&session, &candidate);
    assert_confirmation(
        &session,
        AccountId(0),
        Side::Sell,
        ORDINARY_QTY,
        ordinary_seq,
        day,
    );
    assert_confirmation(
        &session,
        AccountId(1),
        Side::Buy,
        ORDINARY_QTY,
        ordinary_seq,
        day,
    );
    assert_confirmation(
        &session,
        AccountId(3),
        Side::Sell,
        CLOSING_QTY,
        closing_seq,
        day,
    );
    assert_confirmation(&session, TARGET, Side::Buy, CLOSING_QTY, closing_seq, day);
    let accounts = positions(&session);
    let rows = confirmations(&session);
    assert_eq!(rows.values().map(Vec::len).sum::<usize>(), 4);
    let request = |accounts| RegistryDayEndInput {
        day,
        catalog: session.stock_catalog(),
        accounts,
        confirmations: &rows,
        issuer_repurchase_account: None,
        late_holder_tax_accounts: None,
    };
    let registry_candidate = foundation
        .registry_day_end_candidate(request(&accounts))
        .unwrap();
    let registry = &registry_candidate.registries[0];
    assert_eq!(registry.settled_on(), day);
    assert_eq!(registry.holdings().len(), IDS.len());
    for holding in registry.holdings() {
        let HolderId::Account(id) = &holding.holder else {
            panic!("短 fixture 没有外部或 Treasury 股东")
        };
        assert_eq!(
            holding.lots.iter().map(|lot| lot.qty).sum::<u64>(),
            accounts[id][&STOCK],
            "{id:?} R1 名册必须对应生产最终账户"
        );
    }
    // 只替换目标账户为真实收盘前快照，其余账户保留最终事实，精确定位旧差额。
    let mut stale = accounts.clone();
    stale.insert(TARGET, pre_closing[&TARGET].clone());
    let stale_error = foundation
        .registry_day_end_candidate(request(&stale))
        .unwrap_err();
    eprintln!("#119 R1 旧镜像负控：{stale_error:?}");
    assert!(
        matches!(stale_error, SessionCorporateActionsError::RegistryPositionMismatch {
        date, stock: STOCK, account: TARGET, expected: 1500, actual: 1200,
    } if date == day),
        "旧1200镜像必须得到含日期/证券/账户的 typed 1500/1200 失配"
    );
    let installed = SessionCorporateActions {
        registries: registry_candidate.registries,
        dividend_tax_books: registry_candidate.dividend_tax_books,
        ..foundation
    };
    let repeated = installed
        .registry_day_end_candidate(request(&accounts))
        .unwrap();
    assert_eq!(
        repeated.registries, installed.registries,
        "同日重入不重复累计 closing300"
    );
    let receipts_before = rows;
    session.end_civil_day().unwrap();
    assert_eq!(
        session.civil_date(),
        day.next().unwrap(),
        "市场日终后显式推进真实自然日"
    );
    // 真实次日第一拍核对镜像未迟交或双计，第二次日结再将真实事实交给 R1。
    session.step_via_runtime().unwrap();
    assert_final_accounts(&session, &candidate);
    assert_eq!(confirmations(&session), receipts_before);
    for _ in 8..=12 {
        session.step_via_runtime().unwrap();
    }
    assert_eq!(candidates.lock().unwrap().len(), 2);
    assert_final_accounts(&session, &candidates.lock().unwrap()[1]);
    let next_accounts = positions(&session);
    let next_rows = confirmations(&session);
    assert_eq!(next_rows, receipts_before);
    let next = installed
        .registry_day_end_candidate(RegistryDayEndInput {
            day: day.next().unwrap(),
            catalog: session.stock_catalog(),
            accounts: &next_accounts,
            confirmations: &next_rows,
            issuer_repurchase_account: None,
            late_holder_tax_accounts: None,
        })
        .unwrap();
    assert_eq!(next.registries[0].settled_on(), day.next().unwrap());
    assert_eq!(
        next.registries[0].holdings(),
        installed.registries[0].holdings()
    );
    assert_eq!(next_accounts[&TARGET][&STOCK], 1500);
    session.end_civil_day().unwrap();
    assert_eq!(session.civil_date(), day.next().unwrap().next().unwrap());
}

use super::super::*;
use std::cell::RefCell;

#[derive(Clone, Debug, PartialEq, Eq)]
struct PerturbationEvidence {
    hits: usize,
    original_volume: u64,
    trade_count: u64,
    modified_volume: u64,
}

#[derive(Default)]
struct HookState {
    armed: bool,
    hits: usize,
    evidence: Option<PerturbationEvidence>,
}

thread_local! {
    static VOLUME_HOOK: RefCell<HookState> = RefCell::new(HookState::default());
}

struct HookArm;

impl HookArm {
    fn new() -> Self {
        VOLUME_HOOK.with(|hook| {
            let mut hook = hook.borrow_mut();
            assert!(!hook.armed, "retained volume hook 不允许嵌套 arm");
            hook.armed = true;
            hook.hits = 0;
            hook.evidence = None;
        });
        Self
    }

    fn evidence(&self) -> PerturbationEvidence {
        VOLUME_HOOK.with(|hook| {
            hook.borrow()
                .evidence
                .clone()
                .expect("armed retained volume hook 必须命中真实非空 bar")
        })
    }
}

impl Drop for HookArm {
    fn drop(&mut self) {
        VOLUME_HOOK.with(|hook| hook.borrow_mut().armed = false);
    }
}

pub(in crate::session) fn perturb_volume_once(
    mut bars: Vec<Option<Vec<crate::runtime::MinuteBarFact>>>,
) -> Vec<Option<Vec<crate::runtime::MinuteBarFact>>> {
    VOLUME_HOOK.with(|hook| {
        let mut hook = hook.borrow_mut();
        if !hook.armed {
            return;
        }
        let bar = bars
            .iter_mut()
            .filter_map(Option::as_mut)
            .flat_map(|bars| bars.iter_mut())
            .next()
            .expect("armed retained hook 要求真实非空 minute bar");
        let original_volume = bar.volume_shares;
        let trade_count = bar.trade_count;
        let modified_volume = original_volume
            .checked_add(1)
            .expect("retained volume hook 的真实 volume_shares 必须可加一");
        bar.volume_shares = modified_volume;
        hook.armed = false;
        hook.hits = hook.hits.checked_add(1).expect("retained hook 次数必须可计数");
        hook.evidence = Some(PerturbationEvidence {
            hits: hook.hits,
            original_volume,
            trade_count,
            modified_volume,
        });
    });
    bars
}

const TEST_STOCK: StockId = StockId(0);
const SELLER: AccountId = AccountId(1);
const BUYER: AccountId = AccountId(0);
const ORDER_PRICE_CENTS: i64 = 995;
const ORDER_SHARES: u32 = 100;

fn retained_game() -> GameSession {
    let mut setup = super::super::npc_working_quote_tests::quote_setup(0);
    setup.ticks_per_day = 2;
    setup.closing_auction_ticks = 0;
    setup.npcs.retail_count = 0;
    setup.npcs.inst_count = 0;
    setup.npcs.hot_count = 0;
    let mut game = GameSession::new(setup, 7).unwrap();
    let member = game
        .join_market(crate::session::OpaqueSubjectId::new("retained-seller".to_owned()).unwrap(), true)
        .unwrap();
    assert_eq!(member.account_id, SELLER);
    game.state.accounts.get_mut(&SELLER).unwrap().fixture_set_positions(
        std::collections::BTreeMap::from([(
            TEST_STOCK,
            crate::account::Position::from_restored_parts(ORDER_SHARES, 0, 99_500, 0),
        )]),
    );
    game.enqueue_player_intent(
        SELLER,
        Intent::PlaceLimit {
            code: TEST_STOCK,
            side: crate::Side::Sell,
            price: crate::LimitPrice::Fixed(Money::from_cents(ORDER_PRICE_CENTS)),
            qty: ORDER_SHARES,
        },
    )
    .unwrap();
    let events = game.step_via_runtime().unwrap();
    assert!(events.iter().any(|event| matches!(event, Event::OrderAccepted { .. })));
    assert!(game.causal_facts().iter().any(|fact| matches!(&fact.kind,
        crate::diagnostics::causal::CausalFactKind::Submitted(origin) if origin.account == SELLER)));
    game.enqueue_player_intent(
        BUYER,
        Intent::PlaceLimit {
            code: TEST_STOCK,
            side: crate::Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(ORDER_PRICE_CENTS)),
            qty: ORDER_SHARES,
        },
    )
    .unwrap();
    game
}

fn mismatch(error: &StepFatal) -> bool {
    matches!(error, StepFatal::InvariantViolation { location, description }
        if location == "GameSession::step_via_runtime::retained_freeze"
            && description.contains("分钟累计量/额/笔与日K不一致"))
}

fn assert_failed_surfaces_match_checkpoint(game: &GameSession, checkpoint: &GameSession) {
    assert_eq!(game.tick(), checkpoint.tick());
    assert_eq!(game.current_seq(), checkpoint.current_seq());
    assert_eq!(
        serde_json::to_value(game.causal_facts()).unwrap(),
        serde_json::to_value(checkpoint.causal_facts()).unwrap()
    );
    assert_eq!(game.state.market_minute_bars, checkpoint.state.market_minute_bars);
    assert_eq!(
        serde_json::to_value(game.retained_market_days()).unwrap(),
        serde_json::to_value(checkpoint.retained_market_days()).unwrap()
    );
    let expected = checkpoint.runtime_derive_envelopes().unwrap();
    assert_eq!(expected.len(), 1, "checkpoint 必须有真实 runtime 卖余单");
    let key = expected[0].key();
    assert_eq!((key.account, key.stock, key.order, key.side, expected[0].audit().remaining_qty),
        (SELLER, TEST_STOCK, OrderId(1), crate::Side::Sell, ORDER_SHARES));
    let project = |session: &GameSession| session.runtime_derive_envelopes().unwrap().into_iter()
        .map(|envelope| (envelope.key().clone(), envelope.audit())).collect::<Vec<_>>();
    assert_eq!(project(game), project(checkpoint));
    for account_id in [BUYER, SELLER] {
        let actual = game.account(account_id).unwrap();
        let expected = checkpoint.account(account_id).unwrap();
        assert_eq!(actual.cash(), expected.cash());
        assert_eq!(
            actual.position(&TEST_STOCK).map(|position| position.qty()),
            expected.position(&TEST_STOCK).map(|position| position.qty())
        );
        assert_eq!(actual.sellable_qty(&TEST_STOCK), expected.sellable_qty(&TEST_STOCK));
    }
}

fn assert_healthy_retry(checkpoint: &GameSession) {
    let mut retry = checkpoint.clone_for_tick_shadow().unwrap();
    let events = retry.step_via_runtime().unwrap();
    let trades = events
        .iter()
        .filter_map(|event| match event {
            Event::Trade { maker, taker, price, qty, .. } => {
                Some((*maker, *taker, price.cents(), *qty))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(trades, vec![(SELLER, BUYER, ORDER_PRICE_CENTS, ORDER_SHARES)]);
    assert!(retry.runtime_derive_envelopes().unwrap().is_empty(), "真实 runtime 双腿订单全部成交后无余单");
    assert_eq!(retry.retained_market_days().len(), 1);
    assert!(retry.state.market_minute_bars.values().all(Vec::is_empty));
    assert!(retry.account(SELLER).unwrap().position(&TEST_STOCK).is_none(), "卖腿全部成交后无剩余持仓");
    assert_eq!(retry.account(BUYER).unwrap().position(&TEST_STOCK).unwrap().qty(), ORDER_SHARES);
    let executions = retry
        .causal_facts()
        .iter()
        .filter(|fact| matches!(fact.kind,
            crate::diagnostics::causal::CausalFactKind::Execution { qty: ORDER_SHARES, .. }))
        .count();
    assert_eq!(executions, 1);
    let filled = retry
        .causal_facts()
        .iter()
        .filter(|fact| matches!(fact.kind,
            crate::diagnostics::causal::CausalFactKind::Filled { qty: ORDER_SHARES, .. }))
        .count();
    assert_eq!(filled, 2);
}

fn assert_expected_poison_hash(checkpoint: &GameSession, failed: &GameSession, error: &StepFatal) {
    let mut expected_poison = checkpoint.clone_for_tick_shadow().unwrap();
    expected_poison.poison_runtime_step(error.clone());
    assert_eq!(failed.poison_reason(), Some(error));
    assert_eq!(
        failed.session_state_hash().unwrap(),
        expected_poison.session_state_hash().unwrap(),
        "与同一健康 checkpoint 的 poison oracle 比较完整 session 状态"
    );
}

#[test]
fn ordinary_runtime_step_rejects_corrupt_retained_volume_and_checkpoint_retry_fills_once() {
    let mut game = retained_game();
    let checkpoint = game.clone_for_tick_shadow().unwrap();
    let hook = HookArm::new();
    let error = game.step_via_runtime().unwrap_err();
    let evidence = hook.evidence();
    drop(hook);

    assert!(mismatch(&error), "实际错误必须来自 retained 日K成交量对账：{error:?}");
    assert_eq!(evidence.hits, 1);
    assert_eq!((evidence.original_volume, evidence.trade_count), (100, 1));
    assert_eq!(evidence.modified_volume, 101);
    assert_failed_surfaces_match_checkpoint(&game, &checkpoint);
    assert_expected_poison_hash(&checkpoint, &game, &error);
    assert_healthy_retry(&checkpoint);
}

#[test]
fn runtime_frame_retained_failure_skips_callback_and_checkpoint_retry_fills_once() {
    let mut game = retained_game();
    let checkpoint = game.clone_for_tick_shadow().unwrap();
    let hook = HookArm::new();
    let mut callback_called = false;
    let result = game.step_frame_and_commit_via_runtime(|_, _| {
        callback_called = true;
        Ok::<(), StepFatal>(())
    });
    let error = result.unwrap_err();
    let evidence = hook.evidence();
    drop(hook);

    assert!(!callback_called, "retained freeze 失败须先于 frame project callback");
    assert!(mismatch(&error), "实际错误必须来自 retained 日K成交量对账：{error:?}");
    assert_eq!(evidence.hits, 1);
    assert_eq!((evidence.original_volume, evidence.trade_count), (100, 1));
    assert_eq!(evidence.modified_volume, 101);
    assert_failed_surfaces_match_checkpoint(&game, &checkpoint);
    assert_expected_poison_hash(&checkpoint, &game, &error);
    assert_healthy_retry(&checkpoint);
}

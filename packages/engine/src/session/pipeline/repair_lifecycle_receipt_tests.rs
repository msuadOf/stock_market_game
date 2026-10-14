//! Astra未通过项的真正行为红：只运行既有生产入口，不用缺模块/缺方法冒充运行红。
use super::*;
use crate::config::FeeHelperProbe;
use crate::session::NpcOrderLifecycle;
use crate::{AccountId, AccountKind, Intent, LimitPrice, Money, Order, OrderId, Position, Side, StockId};
use std::collections::BTreeSet;

fn order(id: u64, owner: AccountId, side: Side, price: i64, qty: u32) -> Order {
    Order { id: OrderId(id), owner, side, price: Money::from_cents(price), qty,
        original_qty: qty, filled_qty: 0, filled_value: Money::ZERO, seq: 0 }
}

fn quiet_buy() -> (GameSession, FeeHelperProbe) {
    assert!(rayon::current_num_threads() >= 2, "fixture必须真实覆盖多worker的prepare与scoped计数");
    let probe = FeeHelperProbe::scoped();
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.config.fee_probe = probe.clone();
    let mut game = GameSession::new(setup, 65).unwrap();
    game.state.markets.get_mut(&StockId(0)).unwrap()
        .place(order(91, AccountId(1), Side::Buy, 900, 100)).unwrap();
    game.state.next_order_id = 92;
    game.hydrate_or_validate_envelope_ledger().unwrap();
    // T3 修复批（Astra 入口检查点 P1）：去掉旧「空占位 Some」注入——恢复路径
    // 对一切非空 pending 显式拒绝（fault2100），本 fixture 的 save→restore 往返
    // 改用生产形状（pending 恒 None）；空占位载体随批四字段删除退役。
    game.state.attention_scheduler.clear();
    (game, probe)
}

fn bad_audit(game: &GameSession) -> Envelope {
    let key = EnvelopeKey { account: AccountId(1), stock: StockId(0), order: OrderId(91), side: Side::Buy };
    let current = game.state.envelope_ledger.get(&key).unwrap();
    let mut audit = current.audit();
    audit.nominal.commission = Money::from_cents(1);
    audit.charged.commission = Money::from_cents(1);
    // live完全正确，四个book字段也不变；不能用剩余量漂移替代财务反例。
    Envelope::tick_start_existing(key, current.live().cash, 0, audit)
}

#[test]
fn repair_zero_gross_one_cent_nominal_is_rejected_before_any_trusted_use() {
    let (game, _) = quiet_buy();
    // 构造器是结构层（restore类型化fault契约优先）；财务出生不变量在证明建立处拒绝。
    let structural = EnvelopeLedger::new(0, [bad_audit(&game)]).unwrap();
    let row = structural.iter().next().unwrap().1;
    assert!(row.verified_live(&game.state.setup.config).is_err(),
        "零成交额的nominal=charged=1分在进入任何可信使用前被拒绝");
}

#[test]
fn repair_zero_gross_one_cent_nominal_is_rejected_by_whole_prepare() {
    let (mut game, _) = quiet_buy();
    let bad = bad_audit(&game);
    game.state.envelope_ledger.envelopes.insert(bad.key().clone(), bad);
    let result = read_only_start::prepare(&game);
    assert!(result.hydration.is_err(), "合法live预留下1分伪audit被完整ordinary prepare错误接受");
}

#[test]
fn repair_same_page_raw_replacement_invalidates_financial_proof() {
    let (mut game, _) = quiet_buy();
    GameSession::validate_existing_live_projection_with_observer_on(&game.read_view(), || {}).unwrap();
    let before = game.state.envelope_ledger.envelopes.pages()
        .map(|(_, page)| std::sync::Arc::as_ptr(page) as usize).collect::<Vec<_>>();
    let bad = bad_audit(&game);
    let key = bad.key().clone();
    *game.state.envelope_ledger.envelopes.get_mut(&key).unwrap() = bad;
    let after = game.state.envelope_ledger.envelopes.pages()
        .map(|(_, page)| std::sync::Arc::as_ptr(page) as usize).collect::<Vec<_>>();
    assert_eq!(before, after, "fixture必须实际保持page地址，不能依赖地址变化作失效证据");
    let result = GameSession::validate_existing_live_projection_with_observer_on(&game.read_view(), || {});
    assert!(result.is_err(), "page地址未变的raw替换继续误用财务信任");
}

#[test]
fn repair_cloned_candidate_cannot_carry_proof_over_raw_replacement() {
    let (game, probe) = quiet_buy();
    GameSession::validate_existing_live_projection_with_observer_on(&game.read_view(), || {}).unwrap();
    let mut candidate = game.clone_for_tick_shadow().unwrap();
    let bad = bad_audit(&candidate);
    let key = bad.key().clone();
    candidate.state.envelope_ledger.envelopes.insert(key, bad);
    probe.reset();
    assert!(GameSession::validate_existing_live_projection_with_observer_on(&candidate.read_view(), || {}).is_err(),
        "clone/COW候选整行替换后误传旧proof");
    GameSession::validate_existing_live_projection_with_observer_on(&game.read_view(), || {}).unwrap();
    assert_eq!(probe.snapshot(), (0, 0), "失败候选不能使原始未变订单重新计算费用");
}

#[test]
fn repair_one_new_order_does_not_revalidate_other_unchanged_order_on_same_page() {
    let (mut game, probe) = quiet_buy();
    GameSession::validate_existing_live_projection_with_observer_on(&game.read_view(), || {}).unwrap();
    let first = game.state.envelope_ledger.iter().next().unwrap().1.clone();
    let second_order = order(92, AccountId(1), Side::Buy, 900, 100);
    game.state.markets.get_mut(&StockId(0)).unwrap().place(second_order.clone()).unwrap();
    let second = game.project_continuous_envelope(&StockId(0), &second_order).unwrap();
    game.state.envelope_ledger = EnvelopeLedger::new(0, [first, second]).unwrap();
    assert_eq!(game.state.envelope_ledger.envelopes.pages().count(), 1);
    probe.reset();
    GameSession::validate_existing_live_projection_with_observer_on(&game.read_view(), || {}).unwrap();
    assert_eq!(probe.snapshot(), (1, 1), "仅新订单需要建立费用证明；同页旧订单不能被粗失效重算");
}

#[test]
fn repair_actual_helper_probes_do_not_merge_equal_config_values() {
    let first = FeeHelperProbe::scoped();
    let second = FeeHelperProbe::scoped();
    let mut a = crate::GameConfig::proposed_defaults();
    let mut b = a.clone();
    a.fee_probe = first.clone();
    b.fee_probe = second.clone();
    assert_eq!(a, b, "观测字段不能改变业务费用等同性");
    assert_eq!(serde_json::to_vec(&a).unwrap(), serde_json::to_vec(&b).unwrap());
    crate::config::buy_order_reservation(&a.clone(), Money::from_cents(900), 100, Money::ZERO).unwrap();
    assert_eq!(first.snapshot(), (1, 0));
    assert_eq!(second.snapshot(), (0, 0), "同值但不同实例的场景不能混入计数");
}

#[test]
fn repair_raw_apply_must_not_launder_nominal_through_rebase() {
    let (mut game, _) = quiet_buy();
    GameSession::validate_existing_live_projection_with_observer_on(&game.read_view(), || {}).unwrap();
    let bad = bad_audit(&game);
    let key = bad.key().clone();
    let applied = game.state.envelope_ledger.envelopes.get_mut(&key).unwrap().apply(
        ReceiptDelta::sealed(ResVec::ZERO, ResVec::ZERO, bad.live()), bad.audit(), false);
    if applied.is_err() {
        // 在更早的财务可变入口fail closed也符合契约。
        return;
    }
    game.state.envelope_ledger.audits.insert(key, bad.audit());
    game.state.envelope_ledger.rebase_live_for_next_tick().unwrap();
    assert!(GameSession::validate_existing_live_projection_with_observer_on(&game.read_view(), || {}).is_err(),
        "原错误audit经过rebase后被洗成可信状态");
}

#[test]
fn repair_unchanged_buy_whole_ordinary_prepare_has_zero_actual_fee_helpers() {
    let (game, probe) = quiet_buy();
    // 首次建立合法财务事实不计入未变tick测量。
    let first = read_only_start::prepare(&game);
    first.hydration.unwrap();
    first.resources.unwrap();
    probe.reset();
    let prepared = read_only_start::prepare(&game);
    prepared.hydration.unwrap();
    prepared.resources.unwrap();
    let stock_shards = prepared.stock_shards.unwrap().unwrap();
    assert!(!stock_shards.is_empty());
    assert_eq!(probe.snapshot(), (0, 0), "完整prepare仍在worker初始化中重算未变Buy费用");
}

#[test]
fn repair_unchanged_buy_full_market_tick_including_finish_has_zero_actual_fee_helpers() {
    let (mut game, probe) = quiet_buy();
    read_only_start::prepare(&game).hydration.unwrap();
    probe.reset();
    market_tick_transaction::prepare_market_tick_with_evidence(&mut game, false).unwrap().commit();
    assert_eq!(game.state.markets[&StockId(0)].resting_order_count(), 1);
    assert_eq!(probe.snapshot(), (0, 0), "完整无操作market tick的finish/commit仍重算未变费用");
}

#[test]
fn repair_unchanged_buy_whole_speculative_prepare_has_zero_actual_fee_helpers() {
    let (game, probe) = quiet_buy();
    read_only_start::prepare_on(&game.read_view()).hydration.unwrap();
    probe.reset();
    let prepared = read_only_start::prepare_on(&game.read_view());
    prepared.hydration.unwrap();
    prepared.resources.unwrap();
    assert!(prepared.stock_shards.unwrap().is_ok());
    assert_eq!(probe.snapshot(), (0, 0), "投机prepare仍重算实际reservation/nominal helper");
}

#[test]
fn repair_unchanged_buy_self_view_uses_verified_reservation() {
    let (mut game, probe) = quiet_buy();
    let owner = AccountId(1);
    let attention = game.state.npc_attention.get_mut(&owner).unwrap();
    attention.next_attention_candidate_tick = 0;
    attention.rng_state = 3;
    game.state.attention_scheduler.clear();
    game.state.attention_scheduler.enqueue(0, owner);
    probe.reset();
    let capture = decision_snapshot_capture::capture_decision_snapshot(&mut game).unwrap();
    assert!(capture.snapshot.due_npc_ids().contains(&owner), "fixture必须真实观察持有Buy的NPC");
    assert_eq!(probe.snapshot(), (0, 0), "SelfView从真实工作单再次推算资源预留");
}

#[test]
fn repair_no_expiry_buy_reuses_verified_facts_in_expiry_phase() {
    let (game, probe) = quiet_buy();
    let mut shadow = TickShadowPlan { decision_output: None, session: Some(TickShadowPlan::capture_candidate(&game).unwrap()), event_outbox: Vec::new(),
        applied_receipts: Vec::new(), auction_finalizers: Vec::new(),
        expiry: ExpiryOutput::default(), expiry_applied: false, decision_resources: None,
        precomputed_stock_inputs: None, precomputed_stock_shards: None };
    probe.reset();
    let expiry = quote_expiry::plan_expiry(&mut shadow).unwrap();
    assert!(expiry.releases.is_empty());
    assert_eq!(probe.snapshot(), (0, 0), "非到期Expiry仍调用完整hydrate重算费用");
}

#[test]
fn repair_rebase_changes_no_fee_inputs_and_keeps_general_conservation_checks() {
    let (game, probe) = quiet_buy();
    let row = game.state.envelope_ledger.iter().next().unwrap().1;
    let mut created = Envelope::created_at_validation(row.key().clone(), row.live().cash, row.live().shares, row.audit());
    created.verified_live(&game.state.setup.config).unwrap();
    let before = (created.key().clone(), created.audit(), created.live(), created.pending_price());
    probe.reset();
    created.rebase_for_next_tick().unwrap();
    assert_eq!(created.origin(), EnvelopeOrigin::TickStart);
    assert_eq!((created.key().clone(), created.audit(), created.live(), created.pending_price()), before,
        "必须逐字段证实rebase不改变费用公式及其财务依赖，不能只凭‘纯rebase’命名");
    created.validate().unwrap();
    created.verified_live(&game.state.setup.config).unwrap();
    assert_eq!(probe.snapshot(), (0, 0));
}

#[test]
fn repair_restore_revalidates_raw_facts_and_then_reuses_its_own_proof() {
    let (game, _) = quiet_buy();
    let save = game.save().unwrap();
    let bytes = serde_json::to_vec(&save).unwrap();
    assert!(!String::from_utf8(bytes.clone()).unwrap().contains("financial_runtime"));
    let decoded = crate::decode_save_slot(&bytes, &crate::session::persistence::SaveDecodeLimits::default()).unwrap();
    let mut restored = GameSession::restore(&decoded).unwrap();
    let probe = FeeHelperProbe::scoped();
    restored.state.setup.config.fee_probe = probe.clone();
    restored.state.validated_config.fee_probe = probe.clone();
    probe.reset();
    GameSession::validate_existing_live_projection_with_observer_on(&restored.read_view(), || {}).unwrap();
    assert_eq!(probe.snapshot(), (0, 0), "restore建立自己的证明后，普通读取不应再重算");
}

#[test]
fn repair_raw_reconstruction_never_imports_an_existing_fee_proof() {
    let (game, probe) = quiet_buy();
    let row = game.state.envelope_ledger.iter().next().unwrap().1;
    let json = serde_json::to_value(row).unwrap();
    assert!(json.get("financial_runtime").is_none());
    let raw = Envelope::tick_start_existing(row.key().clone(), row.live().cash, row.live().shares, row.audit());
    probe.reset();
    raw.verified_live(&game.state.setup.config).unwrap();
    assert_eq!(probe.snapshot(), (1, 1), "相同业务值的raw重建仍须建立自己的证明，不能按值拼接旧信任");
}

#[test]
fn repair_producer_market_chain_normalizes_real_retail_and_institutional_fills_once() {
    let mut setup = crate::session::npc_working_quote_tests::quote_setup(0);
    setup.npcs.retail_count = 1;
    setup.npcs.inst_count = 1;
    let mut game = GameSession::new(setup, 65011).unwrap();
    let retail = *game.state.accounts.iter().find(|(_, account)| account.kind() == AccountKind::Retail).unwrap().0;
    let institution = *game.state.accounts.iter().find(|(_, account)| account.kind() == AccountKind::Inst).unwrap().0;
    assert!(game.state.retail_experience.contains_key(&retail));
    assert!(game.state.belief_participants.contains_key(&institution));
    let code = StockId(0);
    game.fixture_account_mut(&AccountId(0)).unwrap().fixture_insert_position(code,
        Position::from_restored_parts(200, 0, 200_000, 0));
    game.state.markets.get_mut(&code).unwrap().place(order(80, AccountId(0), Side::Sell, 1000, 200)).unwrap();
    // 同批真实PreSeal Release：过期NPC Buy位于卖价下方，不触发提前成交。
    game.state.markets.get_mut(&code).unwrap().place(order(81, retail, Side::Buy, 900, 100)).unwrap();
    game.state.npc_order_lifecycles.push(NpcOrderLifecycle { account: retail, code, order_id: OrderId(81),
        placed_market_minute: game.current_market_minute(), expires_market_minute: game.current_market_minute() });
    game.state.next_order_id = 82;
    game.hydrate_or_validate_envelope_ledger().unwrap();
    let intents = [retail, institution].into_iter().map(|owner| {
        game.state.ingress_receipt_cursors.receive(owner, Intent::PlaceLimit {
            code, side: Side::Buy, price: LimitPrice::Fixed(Money::from_cents(1000)), qty: 100 }).unwrap()
    }).collect();
    crate::session::pipeline::staged_decision::stage(
        crate::session::pipeline::NpcDecisionHandover::new(game.state.tick, vec![retail, institution], intents, Vec::new()),
    );
    game.state.attention_scheduler.clear();
    fill_fact::FILL_FACT_DERIVATION_COUNT.with(|count| count.set(0));
    let prepared = market_tick_transaction::prepare_market_tick_with_evidence(&mut game, true).unwrap();
    // PreSeal前缀在实际CommitEvidence审计链中；MarketTransactionOutput只含stock批，不能看错容器。
    assert!(prepared.evidence().expiry_receipts().iter().any(|receipt| matches!(receipt.local_key.source(), ReceiptSource::QuoteExpiry(_))
        && receipt.kind == ReceiptKind::Release), "fixture必须包含真实PreSeal释放收据");
    let committed = prepared.commit();
    let filled_accounts: BTreeSet<_> = committed.output.receipts.iter().filter(|r| r.kind == ReceiptKind::Fill)
        .map(|r| r.envelope.account).collect();
    assert!(filled_accounts.contains(&retail) && filled_accounts.contains(&institution));
    assert_eq!(game.state.accounts[&retail].positions()[&code].qty(), 100);
    assert_eq!(game.state.accounts[&institution].positions()[&code].qty(), 100);
    assert!(game.state.retail_experience[&retail].stocks[&code].last_buy_order_id.is_some());
    assert!(game.state.belief_participants[&institution].belief().experience().stocks[&code].last_buy_order_id.is_some());
    fill_fact::FILL_FACT_DERIVATION_COUNT.with(|count| assert_eq!(count.get(), 1,
        "真正producer→market tick→结算和两类经验链对同一Fill仍做双规范化"));
}

use super::npc_tick_preparation::prepare_npc_decisions;
use super::*;
use crate::strategy::ZiNoiseStrategy;
use crate::{AccountId, Event, Intent, Money, RejectionReason, Side};

fn due_retail(seed: u64, order_size: u32) -> (GameSession, AccountId) {
    let account = AccountId(1);
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        seed,
    )
    .unwrap();
    let mut strategy = crate::strategy::StrategyState::ZiNoise(
        ZiNoiseStrategy::new(1.0, order_size, 0.5).unwrap(),
    );
    strategy
        .set_base_observation_probability(session.state.npc_attention[&account].base_probability);
    session
        .fixture_account_mut(&account)
        .unwrap()
        .set_strategy(strategy.into_strategy().unwrap());
    let tick = session.state.tick;
    // Preserve the real profile so this fixture can also be saved and restored.
    // Choose an attention-stream position that accepts even a quiet market.
    let attention = session.state.npc_attention.get_mut(&account).unwrap();
    attention.next_attention_candidate_tick = tick;
    attention.rng_state = 3;
    let mut quiet_probe = attention.clone();
    assert!(quiet_probe.evaluate_candidate_with_signal(crate::AccountKind::Retail, 0.0, tick));
    session.state.attention_scheduler.enqueue(tick, account);
    (session, account)
}

/// T3 契约重登记（A4 失败 3 处置；批四=pending 全链删除后的终态形态）：
/// 跨 tick `pending_npc` 字段已从 SaveSlot 删除——日内/日终档一律无此键；
/// 旧格式档（JSON 含 `pending_npc` 键，无论 Some/None）在**解码边界**显式
/// 拒绝（fault2100，decode_save_slot 字节预检；结构侧 deny_unknown_fields
/// 的通用 2080 由预检先行接管为精确错误）。日内档 restore 必须成功并继续。
#[test]
fn missing_npc_batch_contract_after_entry_switch() {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        42,
    )
    .unwrap();
    let boundary = session.save().unwrap();
    GameSession::restore(&boundary).unwrap();
    session.step().unwrap();
    let intraday = session.save().unwrap();
    // 新契约：日内静点档可恢复且可继续（无跨 tick 载体）。
    let mut resumed = GameSession::restore(&intraday).unwrap();
    resumed.step().expect("新契约：日内档恢复后必须可继续");
    // 存档负例：伪造含 pending_npc 键的旧格式档 ⇒ 解码边界 fault2100 拒绝
    //（选项甲：不迁移、不静默丢键；键存在即旧格式）。
    let mut legacy = serde_json::to_value(&intraday).unwrap();
    legacy["pending_npc"] = serde_json::json!({
        "observed_tick": intraday.snapshot.tick + 1,
        "observed_accounts": [],
        "intents": [],
        "dependencies": [],
    });
    let bytes = serde_json::to_vec(&legacy).unwrap();
    assert!(matches!(
        crate::decode_save_slot(&bytes, &crate::session::persistence::SaveDecodeLimits::default()),
        Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2100
    ));
}

/// Astra 入口切换检查点 P1 修复·批四终态（ADR-0042 §9.6 选项甲；A1 §6.2）：
/// 旧格式档（含 `pending_npc` 键——含观察 tick 与快照一致的历史合法形态）
/// 在解码边界显式结构化拒绝（fault2100）。拒绝理由：旧档的 attention/RNG
/// 已含 D(a) 推进，重放/重算即双重评估；ordinal 已分配，任何静默迁移都会
/// 造成游标漂移。不重算、不迁移、不静默丢键（选项乙已否决）。
#[test]
fn restore_explicitly_rejects_legacy_pending_some_batch() {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        43,
    )
    .unwrap();
    session.step().unwrap();
    let intraday = session.save().unwrap();
    // 伪造「匹配 tick 的合法 Some 档」（旧契约日内档形态：观察 tick==快照
    // tick、依赖表/观察账户均合法——前移前该档可恢复装入）。
    let mut legacy = serde_json::to_value(&intraday).unwrap();
    legacy["pending_npc"] = serde_json::json!({
        "observed_tick": intraday.snapshot.tick,
        "observed_accounts": [],
        "intents": [],
        "dependencies": [],
    });
    let bytes = serde_json::to_vec(&legacy).unwrap();
    assert!(
        matches!(
            crate::decode_save_slot(&bytes, &crate::session::persistence::SaveDecodeLimits::default()),
            Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2100
        ),
        "含 pending_npc 键的旧格式档必须在解码边界被显式拒绝（fault2100）"
    );
    // 同档对照：不带键的新档解码成功——拒绝源于旧键而非档损坏。
    let clean_bytes = serde_json::to_vec(&intraday).unwrap();
    crate::decode_save_slot(
        &clean_bytes,
        &crate::session::persistence::SaveDecodeLimits::default(),
    )
    .expect("新格式档（无 pending_npc 键）必须可解码");
}

#[test]
fn no_due_npc_queues_an_empty_batch_without_building_a_market_view() {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        42,
    )
    .unwrap();
    let future_tick = session.state.tick + 10;
    session.state.attention_scheduler.clear();
    let accounts = session
        .state
        .npc_attention
        .keys()
        .copied()
        .collect::<Vec<_>>();
    for account in accounts {
        session
            .state
            .npc_attention
            .get_mut(&account)
            .unwrap()
            .next_attention_candidate_tick = future_tick;
        session
            .state
            .attention_scheduler
            .enqueue(future_tick, account);
    }
    let code = session.state.markets.keys().next().unwrap().clone();
    session.state.market_minute_closes.remove(&code);

    super::queue_npc_for_next_tick(&mut session).unwrap();
    let ready = super::npc_tick_preparation::staged_decision::take_any().unwrap();
    assert_eq!(ready.observed_tick(), session.state.tick);
    assert!(ready.observed_accounts().is_empty());
    assert!(ready.intents().is_empty());
    assert!(ready.dependencies().is_empty());
}

#[test]
fn pending_npc_dependencies_validate_only_explicit_same_stock_replacements() {
    let a = crate::StockId(0);
    let b = crate::StockId(1);
    let place = |code| Intent::PlaceLimit {
        code,
        side: Side::Buy,
        price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
        qty: 100,
    };
    let received =
        |owner, intent, account_ordinal, stock_ordinal| crate::session::ReceiptBearingIntent {
            owner,
            intent,
            account_ordinal,
            stock_ordinal,
        };
    // 批四：PendingNpcBatch 载体删除——依赖表语义由 NpcDecisionHandover 交接检查
    // （into_batch 检查一之依赖表合法）唯一承载，本单元测试随之改接。
    let output = |dependencies: Vec<(usize, usize)>| {
        super::npc_tick_preparation::NpcDecisionHandover::new(
            0,
            vec![AccountId(1), AccountId(2)],
            vec![
                received(
                    AccountId(1),
                    Intent::Cancel {
                        code: a.clone(),
                        id: crate::OrderId(1),
                    },
                    0,
                    0,
                ),
                received(
                    AccountId(1),
                    Intent::Cancel {
                        code: b.clone(),
                        id: crate::OrderId(2),
                    },
                    1,
                    0,
                ),
                received(AccountId(1), place(a.clone()), 2, 1),
                received(AccountId(2), place(a), 0, 2),
                received(AccountId(1), place(b), 3, 1),
            ],
            dependencies,
        )
    };
    output(vec![(0, 2), (1, 4)]).into_batch(0).unwrap();
    for invalid in [
        vec![(0, 5)],
        vec![(2, 0)],
        vec![(0, 0)],
        vec![(2, 4)],
        vec![(0, 1)],
        vec![(0, 3)],
        vec![(1, 2)],
        vec![(0, 2), (0, 2)],
    ] {
        assert!(
            output(invalid.clone()).into_batch(0).is_err(),
            "{invalid:?}"
        );
    }
    output(Vec::new()).into_batch(0).unwrap();
}

/// T3 修复批重登记·批四终态：跨 tick 载体删除后，旧格式档（含
/// `pending_npc` 键）在**解码边界**统一拒绝（fault2100）；依赖表语义由
/// staged NpcDecisionHandover 的活体消费（take_staged_npc_batch → into_batch
/// 检查一）完整承载。
#[test]
fn restore_rejects_legacy_pending_while_live_consumption_validates_dependencies() {
    let (mut session, npc) = due_retail(41, 100);
    let code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    let save = session.save().unwrap();
    // 旧格式档（任意 pending_npc 键形态）⇒ 解码边界 fault2100。
    let mut legacy = serde_json::to_value(&save).unwrap();
    legacy["pending_npc"] = serde_json::json!({
        "observed_tick": save.snapshot.tick,
        "observed_accounts": [],
        "intents": [],
        "dependencies": [],
    });
    let bytes = serde_json::to_vec(&legacy).unwrap();
    assert!(matches!(
        crate::decode_save_slot(&bytes, &crate::session::persistence::SaveDecodeLimits::default()),
        Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2100
    ));
    // 活体注入消费仍逐项校验依赖表（合法依赖 ⇒ 通过；非法 ⇒ InvariantViolation）。
    let valid = vec![
        crate::session::ReceiptBearingIntent {
            owner: npc,
            intent: Intent::Cancel {
                code: code.clone(),
                id: crate::OrderId(9),
            },
            account_ordinal: 0,
            stock_ordinal: 0,
        },
        crate::session::ReceiptBearingIntent {
            owner: npc,
            intent: Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
            account_ordinal: 1,
            stock_ordinal: 1,
        },
    ];
    // A cancellation may legitimately target an order that is already gone.
    super::npc_tick_preparation::staged_decision::stage(
        super::npc_tick_preparation::NpcDecisionHandover::new(
            session.state.tick,
            vec![npc],
            valid.clone(),
            vec![(0, 1)],
        ),
    );
    super::npc_tick_preparation::take_staged_npc_batch(&mut session).unwrap();
    // 非法依赖（下标越界）：交接检查拒绝（文本=decision output dependency）。
    super::npc_tick_preparation::staged_decision::stage(
        super::npc_tick_preparation::NpcDecisionHandover::new(
            session.state.tick,
            vec![npc],
            valid,
            vec![(0, 2)],
        ),
    );
    assert!(matches!(
        super::npc_tick_preparation::take_staged_npc_batch(&mut session),
        Err(StepFatal::InvariantViolation { description, .. })
            if description.contains("decision output dependency")
    ));
}

#[test]
fn npc_preparation_captures_one_snapshot_and_returns_it_for_plan_roots() {
    let (mut authority, npc) = due_retail(8, 100);
    super::npc_state_projection_tests::use_buy_imbalance_analysis(&mut authority, npc);
    let mut prospective = authority.clone_for_tick_shadow().unwrap();

    let prepared = prepare_npc_decisions(&mut prospective, None).unwrap();

    assert_eq!(prepared.snapshot.due_npc_ids(), &[npc]);
    assert_eq!(prepared.projection.accepted_due_npc_ids(), &[npc]);
    assert!(!prepared.candidates.candidates().is_empty());
}

#[test]
fn failed_npc_completion_receipt_discards_the_entire_tick_shadow() {
    // D5 pin+登记（旧管线专有机制）：NPC completion receipt 溢出失败路径
    // =旧管线 staged 准备域的注入面（ingress cursor 灌 u64::MAX 触发
    // fault1004——runtime 无 staged 装配段，组内决策不经过 ingress
    // cursor）。等价锚：组端 Abort/undo 可逆日志（翻转批 batch4 civil
    // 回滚全量对账）承载"失败 tick 不安装副作用"语义；本注入面在
    // runtime 不存在，非未迁移缺口。
    if std::env::var_os("ENGINE_STEP_RUNTIME").is_some() {
        return;
    }
    let (mut session, npc) = due_retail(8, 100);
    super::npc_state_projection_tests::use_buy_imbalance_analysis(&mut session, npc);
    let code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    session
        .state
        .ingress_receipt_cursors
        .next_stock_ordinal
        .insert(code, u64::MAX);
    let before = session.business_state_hash().unwrap();
    let tick_before = session.state.tick;

    let error = session
        .step()
        .expect_err("NPC receipt overflow must fail the tick");

    // fault1004：股票入场序号溢出（session 装配码段；经 ResourceLimit/StepFatal 上抛）。
    assert!(
        format!("{error:?}").contains("fault[1004]"),
        "实际错误: {error:?}"
    );
    assert_eq!(session.business_state_hash().unwrap(), before);
    assert_eq!(session.state.tick, tick_before);
    assert!(
        !super::npc_tick_preparation::staged_decision::has_any(),
        "失败 tick 不安装任何 staged 批次"
    );
    assert_eq!(
        session.state.ingress_receipt_cursors.next_stock_ordinal[&code],
        u64::MAX
    );
}

#[test]
fn next_tick_account_validation_rechecks_npc_orders_against_current_cash() {
    // D5 pin+登记（旧管线专有机制）：staged_decision 桥的"队列批在下一
    // 拍重验现金"=旧管线 NPC 准备域的检视面（runtime 下 staged 仅构造
    // 期消费一次，跨拍重验不适用）。等价锚：组端 purse 所有权模型从
    // 根上消除"挂单后现金变动致拍中透支"（下单即扣 purse）；ADR-0022
    // 尺度复核面由翻转批 batch1 现金对拍承载。
    if std::env::var_os("ENGINE_STEP_RUNTIME").is_some() {
        return;
    }
    let (mut session, npc) = due_retail(8, 100);
    super::npc_state_projection_tests::use_buy_imbalance_analysis(&mut session, npc);

    assert!(super::npc_tick_preparation::staged_decision::take_any().is_none());
    super::queue_npc_for_next_tick(&mut session).unwrap();
    // known-6 #5 处置（B 类重锚）：peek 而非 take——批须留在槽内供 step()
    // 决策入口消费（take 会破坏「探针后消费」流程，见 staged_decision 模块
    // 文档；姊妹用例 npc_request_uses_available_cash 同 peek 口径）。
    let pending = super::npc_tick_preparation::staged_decision::peek_any().unwrap();
    assert!(pending.intents().iter().any(|received| {
        received.owner == npc
            && matches!(
                &received.intent,
                Intent::PlaceLimit {
                    side: Side::Buy,
                    ..
                }
            )
    }));
    session
        .fixture_account_mut(&npc)
        .unwrap()
        .fixture_set_cash(Money::ZERO);

    let events = session.step().unwrap();

    assert!(events.iter().any(|event| matches!(
        event,
        Event::IntentRejected {
            account,
            reason: RejectionReason::InsufficientCash,
            ..
        } if *account == npc
    )));
    assert!(!events.iter().any(|event| matches!(
        event,
        Event::OrderAccepted { account, .. } if *account == npc
    )));
    assert!(!events.iter().any(|event| matches!(
        event,
        Event::Trade { maker, taker, .. } if *maker == npc || *taker == npc
    )));
    assert_eq!(session.state.accounts[&npc].cash(), Money::ZERO);
    assert!(session
        .state
        .markets
        .values()
        .all(|market| { market.resting_orders_for(npc).is_empty() }));
}

#[test]
fn due_institution_plan_roots_are_ready_with_the_npc_source() {
    let mut authority =
        GameSession::new(crate::session::npc_working_quote_tests::quote_setup(0), 8).unwrap();
    let institution = AccountId(1);
    crate::session::npc_working_quote_tests::force_attention_candidate(
        &mut authority,
        institution,
        0,
    );
    let mut prospective = authority.clone_for_tick_shadow().unwrap();

    let prepared = prepare_npc_decisions(&mut prospective, None).unwrap();

    assert_eq!(prepared.snapshot.due_npc_ids(), &[institution]);
    assert_eq!(prepared.projection.accepted_due_npc_ids(), &[institution]);
    assert!(!prepared.roots.is_empty());

    let mut injected = authority.clone_for_tick_shadow().unwrap();
    let prepared = prepare_npc_decisions(
        &mut injected,
        Some(crate::session::plan_chain_candidates::PlanChainOperationBatch::empty()),
    )
    .unwrap();
    assert!(prepared.roots.is_empty());
}

#[test]
fn npc_account_validation_quantity_rejection_reaches_one_final_event() {
    let (mut session, npc) = due_retail(8, 2_000_000);
    super::npc_state_projection_tests::use_buy_imbalance_analysis(&mut session, npc);
    // 7% 的试买目标足以提出 200 万股，由下一 tick 的 AccountValidation 执行单笔数量规则。
    session
        .fixture_account_mut(&npc)
        .unwrap()
        .fixture_set_cash(Money::from_cents(30_000_000_000));
    super::queue_npc_for_next_tick(&mut session).unwrap();
    let before_order_id = session.state.next_order_id;

    let committed = super::market_tick_transaction::prepare_market_tick(&mut session)
        .unwrap()
        .commit();
    assert!(matches!(
        committed.output.validation.results(),
        [CandidateValidationResult::Rejected {
            key: IntentCandidateKey::Npc { account, npc_local_index: 0 },
            reason: RejectionReason::InvalidQuantity,
            ..
        }] if *account == npc
    ));
    assert_eq!(
        committed
            .commit
            .tick
            .event_values()
            .iter()
            .filter(|event| matches!(
                event,
                Event::IntentRejected {
                    account,
                    reason: RejectionReason::InvalidQuantity,
                    ..
                } if *account == npc
            ))
            .count(),
        1
    );
    assert_eq!(session.state.next_order_id, before_order_id);
}

#[test]
fn downstream_failure_discards_npc_decision_and_authority_state() {
    let (authority, npc) = due_retail(8, 100);
    let authority_before = authority.session_state_hash().unwrap();
    let mut decision_probe = authority.clone_for_tick_shadow().unwrap();
    let observation =
        super::decision_snapshot_capture::capture_decision_snapshot(&mut decision_probe).unwrap();
    assert_eq!(observation.snapshot.due_npc_ids(), &[npc]);
    assert_ne!(
        decision_probe.session_state_hash().unwrap(),
        authority_before
    );
    let mut plan = plan_tick(PhaseInput { session: &authority })
    .unwrap();
    plan
        .execute(|candidate| {
            candidate.state.next_receipt_base = 1;
            Ok(())
        })
        .unwrap();
    let result =
        super::market_tick_transaction::apply_tick_shadow_market_transaction(&mut plan);
    assert!(matches!(
        result,
        Err(super::market_tick_transaction::MarketTransactionError::Finalization(
            StepFatal::InvariantViolation { location, description }
        )) if location == "pipeline::continuous_tick_finalizer"
            && description == "session and ledger receipt cursors disagree"
    ));
    assert_eq!(authority.session_state_hash().unwrap(), authority_before);
    assert!(plan.execute(|_| Ok(())).is_err());
}

#[test]
fn intent_candidates_failure_discards_the_whole_tick_candidate() {
    let (mut authority, npc) = due_retail(8, 100);
    authority
        .fixture_account_mut(&npc)
        .unwrap()
        .fixture_set_strategy(None);
    let before = authority.business_state_hash().unwrap();

    let error = super::market_tick_transaction::prepare_market_tick(&mut authority)
        .err()
        .expect("missing NPC strategy must abort preparation");

    assert!(matches!(
        error,
        super::market_tick_transaction::MarketTransactionError::Preparation(
            StepFatal::InvariantViolation { description, location }
        ) if location == "pipeline::npc_tick_preparation" && description.contains("strategy")
    ));
    assert_eq!(authority.business_state_hash().unwrap(), before);
}

#[test]
fn real_npc_working_quotes_cancel_before_one_replacement_with_contiguous_keys() {
    let (mut session, npc) = due_retail(41, 100);
    session
        .fixture_account_mut(&npc)
        .unwrap()
        .fixture_set_cash(Money::from_cents(10_000_000));
    // Seed two pre-existing quotes without reconciling the first one.
    session
        .fixture_account_mut(&npc)
        .unwrap()
        .fixture_set_kind(crate::AccountKind::Player);
    let code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    for price in [900, 901] {
        session.seed_order_for_test(
            npc,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(price)),
                qty: 1_000,
            },
            &mut Vec::new(),
        );
    }
    session
        .fixture_account_mut(&npc)
        .unwrap()
        .fixture_set_kind(crate::AccountKind::Retail);
    let mut old_ids: Vec<_> = session.state.markets[&code]
        .resting_orders_for(npc)
        .iter()
        .map(|order| order.id)
        .collect();
    old_ids.sort();
    assert_eq!(old_ids.len(), 2);
    let resources = plan_tick(PhaseInput {
        session: &session,
    })
    .unwrap()
    .decision_resources()
    .unwrap()
    .clone();
    let mut queued_session = session.clone_for_tick_shadow().unwrap();
    let prepared = prepare_npc_decisions(&mut session, None).unwrap();
    assert!(prepared.projection.reconciliation_decisions().iter().all(|decision| matches!(
        decision,
        super::npc_state_projection::NpcReconciliationDecision::Replace { account, .. } if *account == npc
    )));
    let candidates = prepared.candidates.candidates();
    assert_eq!(
        candidates.len(),
        3,
        "two old quotes share one residual replacement"
    );
    assert_eq!(
        candidates
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        (0..3)
            .map(|index| IntentCandidateKey::npc(npc, index))
            .collect::<Vec<_>>()
    );
    for (candidate, old_id) in candidates.iter().zip(&old_ids) {
        assert!(matches!(candidate.intent(), Intent::Cancel { id, .. } if id == old_id));
    }
    assert!(matches!(
        candidates[2].intent(),
        Intent::PlaceLimit {
            side: Side::Buy,
            qty: 100,
            ..
        }
    ));
    super::queue_npc_for_next_tick(&mut queued_session).unwrap();
    let expected = candidates
        .iter()
        .map(|candidate| (candidate.owner(), candidate.intent().clone()))
        .collect::<Vec<_>>();
    let staged = super::npc_tick_preparation::staged_decision::take_any().unwrap();
    let queued = staged.intents();
    assert_eq!(
        serde_json::to_value(
            queued
                .iter()
                .map(|received| (received.owner, &received.intent))
                .collect::<Vec<_>>()
        )
        .unwrap(),
        serde_json::to_value(expected).unwrap()
    );
    assert!(queued.iter().enumerate().all(|(index, received)| {
        received.account_ordinal == index as u64 && received.stock_ordinal == index as u64
    }));
    // 批四：跨 tick 载体删除——staged 批的依赖表直接核对（原「随 save 编码
    // 保留」断言随字段退役；新档结构上不含 pending_npc 键）。
    assert_eq!(
        serde_json::to_value(staged.dependencies()).unwrap(),
        serde_json::json!([[0, 2], [1, 2]]),
        "both reconciled cancellations must precede their one replacement"
    );
    let encoded = serde_json::to_value(queued_session.save().unwrap()).unwrap();
    assert!(
        encoded.get("pending_npc").is_none(),
        "新档结构上不含 pending_npc 键"
    );
    // T3 修复批（Astra 入口检查点 P1）+批四终态：旧格式档（注入 pending_npc
    // 键）解码边界显式拒绝（fault2100）；批次消费见证=staged 直接消费（前驱
    // 身份等价断言原样保留）。
    let mut legacy = encoded.clone();
    legacy["pending_npc"] = serde_json::json!({
        "observed_tick": queued_session.state.tick,
        "observed_accounts": [],
        "intents": [],
        "dependencies": [],
    });
    assert!(matches!(
        crate::decode_save_slot(
            &serde_json::to_vec(&legacy).unwrap(),
            &crate::session::persistence::SaveDecodeLimits::default()
        ),
        Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2100
    ));
    // staged 批放回后直接消费（前驱身份断言）。
    super::npc_tick_preparation::staged_decision::stage(staged);
    let (restored_candidates, _) =
        super::npc_tick_preparation::take_staged_npc_batch(&mut queued_session).unwrap();
    assert_eq!(
        serde_json::to_value(&restored_candidates.candidates()[2]).unwrap()["predecessors"],
        serde_json::to_value([
            IntentCandidateKey::npc(npc, 0),
            IntentCandidateKey::npc(npc, 1),
        ])
        .unwrap(),
        "restored replacement must retain both real predecessor identities"
    );

    let mut validator = AccountValidatorDriver::new(
        resources,
        session.state.envelope_ledger.clone(),
        session.state.next_order_id,
        session.state.setup.config.clone(),
        super::account_validation_context::build_account_validation_context(&session).unwrap(),
    )
    .unwrap();
    let mut stock_execution =
        super::continuous_matching::IncrementalContinuousStockCoordinator::from_post_expiry(
            super::continuous_matching_adapter::prepare_incremental_continuous_inputs(&session)
                .unwrap(),
        )
        .unwrap();
    let rounds = super::market_tick_transaction::apply_initial_candidate_stream_for_test(
        &mut validator,
        &mut stock_execution,
        &prepared.candidates,
    )
    .unwrap();
    assert_eq!(
        rounds.iter().map(|round| round.facts.len()).sum::<usize>(),
        3
    );
    let mut final_book = session.state.markets[&code].clone();
    for round in &rounds {
        if let Some(projection) = round.projections.get(&code) {
            final_book
                .apply_changed_orders(projection.market_delta.as_ref().unwrap().clone())
                .unwrap();
        }
    }
    assert!(final_book
        .resting_orders()
        .iter()
        .all(|order| !old_ids.contains(&order.id)));
    assert_eq!(final_book.resting_orders_for(npc).len(), 1);
}

#[test]
fn npc_replacement_keeps_the_requested_place_for_next_tick_validation() {
    let (mut session, npc) = due_retail(41, 100);
    let code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    session.seed_order_for_test(
        npc,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(900)),
            // 仍将全部资金冻结；明确预算使当前策略的试买目标足够重新报一手。
            qty: 2_000,
        },
        &mut Vec::new(),
    );
    let reserved = session.reserved_cash_for_account(npc).unwrap();
    session
        .fixture_account_mut(&npc)
        .unwrap()
        .fixture_set_cash(reserved);
    let prepared = prepare_npc_decisions(&mut session, None).unwrap();
    assert_eq!(prepared.candidates.candidates().len(), 2);
    assert!(matches!(
        prepared.candidates.candidates()[0].intent(),
        Intent::Cancel { .. }
    ));
    assert_eq!(
        prepared.candidates.candidates()[0].key(),
        &IntentCandidateKey::npc(npc, 0)
    );
    assert!(matches!(
        prepared.candidates.candidates()[1].intent(),
        Intent::PlaceLimit { qty: 100, .. }
    ));
    assert_eq!(prepared.projection.residual_intents().len(), 1);
}

#[test]
fn real_npc_review_cancels_working_quote_in_and_cleans_lifecycle() {
    let (mut session, npc) = due_retail(8, 100);
    let code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    session.seed_order_for_test(
        npc,
        Intent::PlaceLimit {
            code: code.clone(),
            side: Side::Buy,
            price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
            qty: 100,
        },
        &mut Vec::new(),
    );
    let old_id = session.state.npc_order_lifecycles[0].order_id;
    session
        .fixture_account_mut(&npc)
        .unwrap()
        .set_strategy(Box::new(ZiNoiseStrategy::new(0.0, 100, 0.5).unwrap()));
    super::queue_npc_for_next_tick(&mut session).unwrap();
    let result = super::market_tick_transaction::prepare_market_tick(&mut session)
        .unwrap()
        .commit();
    assert_eq!(result.output.candidates.candidates().len(), 1);
    assert!(
        matches!(result.output.candidates.candidates()[0].intent(), Intent::Cancel { id, .. } if *id == old_id)
    );
    assert!(result.output.event_values().iter().any(|event| matches!(event, Event::OrderCanceled { id, account, .. } if *id == old_id && *account == npc)));
    assert!(session.state.npc_order_lifecycles.is_empty());
    assert!(session.state.markets[&code]
        .resting_orders_for(npc)
        .is_empty());
}

#[test]
fn npc_reconciliation_local_indexes_restart_per_account_in_canonical_account_order() {
    let mut setup = crate::session::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    let mut session = GameSession::new(setup, 8).unwrap();
    let code = session
        .stock_id(&session.state.setup.stocks[0].code.0)
        .unwrap();
    for npc in [AccountId(2), AccountId(1)] {
        session
            .fixture_account_mut(&npc)
            .unwrap()
            .set_strategy(Box::new(ZiNoiseStrategy::new(0.0, 100, 0.5).unwrap()));
        session.seed_order_for_test(
            npc,
            Intent::PlaceLimit {
                code: code.clone(),
                side: Side::Buy,
                price: crate::LimitPrice::Fixed(Money::from_cents(1_000)),
                qty: 100,
            },
            &mut Vec::new(),
        );
        crate::session::npc_working_quote_tests::force_attention_candidate(&mut session, npc, 0);
    }
    let prepared = prepare_npc_decisions(&mut session, None).unwrap();
    assert_eq!(
        prepared
            .candidates
            .candidates()
            .iter()
            .map(|candidate| candidate.key().clone())
            .collect::<Vec<_>>(),
        vec![
            IntentCandidateKey::npc(AccountId(1), 0),
            IntentCandidateKey::npc(AccountId(2), 0)
        ]
    );
    assert!(prepared
        .projection
        .reconciliation_decisions()
        .iter()
        .all(|decision| matches!(
            decision,
            super::npc_state_projection::NpcReconciliationDecision::Cancel { .. }
        )));
}

/// T3 收口组 2（Astra 终审）：旧档识别=**结构键检查**的四形态见证——
/// 合法字符串**值**含子串（新档必须可解码，值非键）；JSON 转义键
/// （_/_ 形态）；null 与非空旧键（键存在即旧格式）；既有业务
/// 校验错误并存时 fault2100 先行（外层 schema 判定优先）。
#[test]
fn legacy_archive_identification_covers_value_escape_null_and_precedence() {
    let mut session = GameSession::new(
        crate::session::npc_working_quote_tests::retail_quote_setup(),
        7,
    )
    .unwrap();
    session.step().unwrap();
    let clean = serde_json::to_value(session.save().unwrap()).unwrap();
    let clean_bytes = serde_json::to_vec(&clean).unwrap();
    let limits = crate::session::persistence::SaveDecodeLimits::default();

    // ① 合法**值**含子串：引擎核心无自由文本（string-free 设计），以
    //    civil_clock.current_date（字符串值）追加子串为载体——值非键，旧档
    //    判定不得触发 fault2100（其后进入正常校验路径，因日期非法被拒的
    //    错误码≠2100 即为「无误报」的见证）。
    let mut with_value = clean.clone();
    with_value["civil_clock"]["current_date"] =
        serde_json::Value::String("2030-01-07-pending_npc".to_owned());
    match crate::decode_save_slot(&serde_json::to_vec(&with_value).unwrap(), &limits) {
        Err(crate::SessionError::InvalidSave(fault)) => assert_ne!(
            fault.code, 2100,
            "值含子串不得触发旧档判定（值非键）——实际拒绝须来自后续正常校验"
        ),
        Err(other) => panic!("预期被日期校验拒绝，实际 {other:?}"),
        Ok(_) => panic!("篡改日期必须被正常校验拒绝（否则载体无效）"),
    }

    // ② 转义键：任意位置 unicode 转义（_/_ 与首字符 p 形态——三轮收口
    //    解析后结构检测全覆盖）⇒ 2100（serde 解析后统一为原键，不落 2080）。
    for escaped in ["pending\\u005fnpc", "pending\\u005Fnpc", "\\u0070ending_npc"] {
        let mut legacy = clean.clone();
        legacy[escaped] = serde_json::Value::Null;
        // 以文本注入真实转义字节：serde_json::Value 键不支持转义形态——
        // 直接在字节流上拼接（保持合法 JSON）。
        let mut bytes = serde_json::to_vec(&legacy).unwrap();
        bytes.pop(); // '}'
        bytes.extend_from_slice(format!(",\"{escaped}\":null}}").as_bytes());
        assert!(
            matches!(
                crate::decode_save_slot(&bytes, &limits),
                Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2100
            ),
            "转义键形态 {escaped} 必须按 fault2100 拒绝"
        );
    }

    // ③ null 与非空旧键：键存在即旧格式（无论值形态）。
    for value in [
        serde_json::Value::Null,
        serde_json::json!({
            "observed_tick": 0,
            "observed_accounts": [],
            "intents": [],
            "dependencies": [],
        }),
    ] {
        let mut legacy = clean.clone();
        legacy["pending_npc"] = value;
        assert!(
            matches!(
                crate::decode_save_slot(&serde_json::to_vec(&legacy).unwrap(), &limits),
                Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2100
            ),
            "旧键（null/非空）必须 fault2100"
        );
    }

    // ④ 优先级：既有业务校验错误（civil_clock 日期篡改）并存时，旧键判定
    //    先行——外层 schema 判定优先于对旧格式的字段级验错。
    let mut both = clean.clone();
    both["pending_npc"] = serde_json::Value::Null;
    both["civil_clock"] = serde_json::json!({"current_date": "1999-99-99"});
    assert!(
        matches!(
            crate::decode_save_slot(&serde_json::to_vec(&both).unwrap(), &limits),
            Err(crate::SessionError::InvalidSave(fault)) if fault.code == 2100
        ),
        "旧键 + 业务日期错误并存 ⇒ fault2100 先行（schema 判定优先）"
    );

    // 对照：干净新档可解码。
    crate::decode_save_slot(&clean_bytes, &limits).expect("干净新档可解码");
}

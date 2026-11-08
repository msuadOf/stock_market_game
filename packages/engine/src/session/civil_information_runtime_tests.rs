//! runtime 自然日 INFO 的真实生产入口契约；不手动投递、不构造 fixture clock 或 proof。
use super::*;
use crate::{LimitPrice, Side};

#[cfg(feature = "simulation-diagnostics")]
#[path = "civil_information_acquisition_tests.rs"]
mod native_acquisition_tests;

#[path = "civil_driver_ownership_tests.rs"]
mod driver_ownership_tests;

fn runtime_information_session(start: &str) -> GameSession {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::CivilDate::from_iso(start).unwrap();
    setup.ticks_per_day = 2;
    let mut game = GameSession::new(setup, 42).unwrap();
    let account = AccountId(1);
    let stock = StockId(0);
    game.state
        .belief_participants
        .get_mut(&account)
        .unwrap()
        .watchlist_mut()
        .record_attention(&stock, 0, 0)
        .unwrap();
    let attention = game.state.npc_attention.get_mut(&account).unwrap();
    attention.next_attention_candidate_tick = u64::MAX;
    let company = game.state.issuer_of_stock(stock).unwrap();
    let date = game.civil_date();
    std::sync::Arc::make_mut(&mut game.state.companies.legacy_mut().unwrap().operations)
        .apply_company_shock(
            &company,
            crate::company::ActiveShock {
                kind: crate::company::ShockKind::ContractWon,
                amplitude_bp: 500,
                starts_on: date,
                expires_on: date.next().unwrap(),
            },
        )
        .unwrap();
    game.construct_runtime_driver_lazy_for_test().unwrap();
    assert!(game.state.runtime_information_authority);
    assert_eq!(game.tick(), 0, "真实静点接管不造 market tick");
    game
}

#[test]
fn first_closed_civil_day_reads_each_full_projection_once() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::CivilDate::from_iso("2030-01-05").unwrap();
    setup.ticks_per_day = 2;
    let mut game = GameSession::new(setup, 42).unwrap();
    assert!(
        game.runtime_driver.is_none(),
        "必须从未接管的真实 Session 开始"
    );
    let before_cash = game.account(AccountId(0)).unwrap().cash();
    let before_date = game.civil_date();
    let scope = crate::session::calendar_entry_cache_tests::EntryScope::enter();
    let report = game.end_civil_day().expect("首个休市日必须真实完成");
    let counts = scope.full_read_counts();
    drop(scope);
    assert_eq!(report.settled_date, before_date);
    assert_eq!(game.civil_date(), before_date.next().unwrap());
    assert_eq!(game.tick(), 0, "休市日不得推进市场拍");
    assert_eq!(game.account(AccountId(0)).unwrap().cash(), before_cash);
    assert_eq!(
        counts.full_accounts, 1,
        "首次接管与行政基线应复用同源完整账户"
    );
    assert_eq!(
        counts.full_personals, 1,
        "首次接管与行政基线应复用同源完整个人态"
    );
}

fn closed_runtime_information_session() -> GameSession {
    runtime_information_session("2030-01-05")
}

fn actual_group_personal(game: &GameSession) -> crate::runtime::world::BeliefParticipantSnapshot {
    game.runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .collect_personal_full()
        .unwrap()
        .groups
        .into_iter()
        .flat_map(|(_, rows)| rows.into_iter())
        .filter_map(|(_, _, personal)| personal)
        .find(|personal| personal.information.owner() == AccountId(1))
        .expect("必须直接读取真实 Group 个人态")
}

#[test]
fn runtime_closed_disclosure_updates_group_belief_cause_and_session_without_market_work() {
    let mut game = closed_runtime_information_session();
    let account = AccountId(1);
    let stock = StockId(0);
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: stock,
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(995)),
            qty: 100,
        },
    )
    .unwrap();
    let before = game.save().unwrap();
    let day_before = game.day();
    let personal_before = actual_group_personal(&game);
    assert_eq!(personal_before.information.acquired_count(), 0);
    let report = game.end_civil_day().unwrap();
    let actual = actual_group_personal(&game);
    assert!(
        actual.information.acquired_count() > 0,
        "休市披露必须在真实 Group 内获知"
    );
    assert!(actual
        .information
        .companies()
        .flat_map(|(_, records)| records)
        .all(|record| record.observed_at == report.disclosure_instant));
    let publication = report
        .events
        .iter()
        .find_map(|event| match event {
            Event::CompanyDisclosurePublished { publication_id, .. } => Some(*publication_id),
            _ => None,
        })
        .expect("fixture 必须有当日真实新披露");
    assert_eq!(
        actual.information.observed_at_of(publication),
        Some(report.disclosure_instant),
        "休市获知必须包含当日本次新披露"
    );
    let entry = actual
        .belief
        .entry(&stock)
        .expect("本人获知必须产生非空 BeliefEntry");
    assert_eq!(
        entry.anchor,
        crate::observation::information_clock::InformationTradingAnchor::NoPriorSession
    );
    assert_eq!(
        entry.last_cause.as_ref().unwrap().as_of_anchor,
        entry.anchor
    );
    assert!(!entry.used_report_ids.is_empty());
    let mirror = &game.state.belief_participants[&account];
    assert_eq!(mirror.information(), &actual.information);
    assert_eq!(mirror.belief(), &actual.belief);
    assert_eq!(&game.state.npc_attention[&account], &actual.attention);
    let after = game.save().unwrap();
    assert_eq!(after.information_states[&account], actual.information);
    assert_eq!(after.belief_books[&account], actual.belief);
    assert_eq!(after.npc_attention[&account], actual.attention);
    assert_eq!(
        (game.tick(), game.day()),
        (before.snapshot.tick, day_before)
    );
    assert_eq!(after.rng_state, before.rng_state);
    assert_eq!(
        serde_json::to_value(&after.snapshot.accounts).unwrap(),
        serde_json::to_value(&before.snapshot.accounts).unwrap()
    );
    assert_eq!(
        serde_json::to_value(&after.pending_player).unwrap(),
        serde_json::to_value(&before.pending_player).unwrap(),
        "civil INFO 不消费交易 intent"
    );
    assert_eq!(
        serde_json::to_value(&after.ingress_receipt_cursors).unwrap(),
        serde_json::to_value(&before.ingress_receipt_cursors).unwrap()
    );
    assert_eq!(after.next_order_id, before.next_order_id);
    assert_eq!(actual.history_reads, personal_before.history_reads);
    assert_eq!(
        actual.attention.rng_state,
        personal_before.attention.rng_state
    );
    assert_eq!(
        actual.attention.next_attention_candidate_tick,
        personal_before.attention.next_attention_candidate_tick
    );
    let second = game
        .end_civil_day()
        .expect("连续休市日仍须认证本次真实 civil 日期");
    assert_eq!(second.settled_date, report.next_date);
    assert_eq!(
        (game.tick(), game.day()),
        (before.snapshot.tick, day_before)
    );
    assert_eq!(
        actual_group_personal(&game)
            .belief
            .entry(&stock)
            .unwrap()
            .anchor,
        entry.anchor
    );
}

#[test]
fn runtime_closed_information_candidate_is_visible_before_commit_and_callback_failure_publishes_nothing(
) {
    let mut game = closed_runtime_information_session();
    let before = game.business_state_hash().unwrap();
    let group_before = actual_group_personal(&game);
    let publication = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication()
        .load();
    let fatal = StepFatal::InvariantViolation {
        location: "civil_info_production::precommit".to_owned(),
        description: "同版 INFO 候选最后投影拒绝".to_owned(),
    };
    let mut reached = false;
    let result: Result<(), SessionError> = game.with_prepared_civil_day(|candidate, report| {
        reached = true;
        let participant = &candidate.state.belief_participants[&AccountId(1)];
        assert!(
            participant.information().acquired_count() > 0,
            "publication callback 必须看到同版 INFO 候选"
        );
        let book = participant.belief();
        let entry = book.entry(&StockId(0)).unwrap();
        assert_eq!(
            entry.last_cause.as_ref().unwrap().as_of_anchor,
            entry.anchor
        );
        assert!(participant
            .information()
            .companies()
            .flat_map(|(_, records)| records)
            .all(|record| record.observed_at == report.disclosure_instant));
        let prepared_save = candidate.save_committed_projection()?;
        assert_eq!(
            &prepared_save.belief_books[&AccountId(1)],
            book,
            "存档投影必须同版"
        );
        Err(SessionError::Step(fatal.clone()))
    });
    assert!(reached);
    assert!(matches!(result, Err(SessionError::Step(error)) if error == fatal));
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(actual_group_personal(&game), group_before);
    assert!(
        game.poison_reason().is_none(),
        "未安装市场日终的可逆 prepare 失败不 poison"
    );
    let after = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication()
        .load();
    assert_eq!(
        (after.tick, after.generation),
        (publication.tick, publication.generation)
    );
    assert!(
        std::sync::Arc::ptr_eq(&after, &publication),
        "拒绝候选不得重发布相同内容"
    );
}

#[test]
fn runtime_civil_information_failure_after_real_market_close_keeps_installed_close_and_terminal_rule(
) {
    let mut game = runtime_information_session("2030-01-04");
    game.step_via_runtime().unwrap();
    game.step_via_runtime().unwrap();
    assert_eq!((game.tick(), game.day()), (2, 1), "必须先真实安装市场日终");
    let before = game.business_state_hash().unwrap();
    let publication = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication()
        .load();
    let fatal = StepFatal::InvariantViolation {
        location: "civil_info_production::after_market_close".to_owned(),
        description: "真实市场日终安装后的 civil INFO 投影拒绝".to_owned(),
    };
    let result: Result<(), SessionError> = game.with_prepared_civil_day(|candidate, report| {
        let publication = report
            .events
            .iter()
            .find_map(|event| match event {
                Event::CompanyDisclosurePublished { publication_id, .. } => Some(*publication_id),
                _ => None,
            })
            .expect("fixture 必须有市场收盘后的真实新披露");
        assert_eq!(
            candidate.state.belief_participants[&AccountId(1)]
                .information()
                .observed_at_of(publication),
            Some(report.disclosure_instant),
            "必须先准备真实 INFO 再命中最后拒绝"
        );
        Err(SessionError::Step(fatal.clone()))
    });
    assert!(matches!(result, Err(SessionError::Step(error)) if error == fatal));
    assert_eq!(
        game.business_state_hash().unwrap(),
        before,
        "已安装收盘不回滚、civil候选不发布"
    );
    assert_eq!(game.poison_reason(), Some(&fatal));
    assert!(matches!(game.save(), Err(SessionError::Step(actual)) if actual == fatal));
    assert_eq!(game.step_via_runtime().unwrap_err(), fatal);
    assert!(matches!(game.end_civil_day(), Err(SessionError::Step(error)) if error == fatal));
    let after = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .publication()
        .load();
    assert!(
        std::sync::Arc::ptr_eq(&after, &publication),
        "失败不改真实已发布收盘"
    );
}

#[test]
fn runtime_closed_civil_prunes_only_two_memories_in_the_same_group_and_session_candidate() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::CivilDate::from_iso("2030-01-05").unwrap();
    setup.ticks_per_day = 2;
    let prototype = setup.stocks[0].clone();
    setup.stocks = (0..9)
        .map(|index| {
            let mut stock = prototype.clone();
            stock.code = crate::StockCode(format!("60088{index}"));
            stock
        })
        .collect();
    let mut session = crate::session::protocol::ProtocolSession::new(setup, 42).unwrap();
    let game = session.game_mut();
    let participant = game
        .state
        .belief_participants
        .get_mut(&AccountId(1))
        .unwrap();
    for index in 0..9 {
        let stock = StockId(index);
        participant
            .watchlist_mut()
            .record_attention(&stock, 0, 0)
            .unwrap();
        participant
            .price_memory_mut()
            .observe_price(&stock, Money::from_cents(1000), 0)
            .unwrap();
    }
    participant
        .price_memory_mut()
        .record_public_history_read(
            &StockId(0),
            0,
            game.state.history_reads.get_mut(&AccountId(1)).unwrap(),
        )
        .unwrap();
    game.construct_runtime_driver_lazy_for_test().unwrap();
    let before = actual_group_personal(game);
    assert_eq!(
        before.watchlist.stock_count(),
        9,
        "静点接管不替 civil 提前裁剪"
    );
    assert_eq!(before.price_memory.stock_count(), 9);
    assert_eq!(
        before.history_reads.stocks[&StockId(0)].read_count,
        1,
        "被两类记忆驱逐的股票必须已有非空历史读取账"
    );
    let update = session.end_civil_day_update().unwrap();
    update.validate().unwrap();
    let game = session.game();
    let after = actual_group_personal(game);
    let expected: std::collections::BTreeSet<_> = (1..9).map(StockId).collect();
    assert_eq!(
        after
            .watchlist
            .stocks
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        expected,
        "civil 必须在 Group 同一 INFO 候选中裁 watchlist"
    );
    assert_eq!(
        after
            .price_memory
            .stocks
            .keys()
            .copied()
            .collect::<std::collections::BTreeSet<_>>(),
        expected,
        "civil 必须在 Group 同一 INFO 候选中裁 price_memory"
    );
    assert_eq!(
        after.history_reads, before.history_reads,
        "civil 不增添 history_reads 裁剪语义"
    );
    for index in 0..9 {
        assert!(
            after.belief.entry(&StockId(index)).is_some(),
            "先按原订阅获知，再仅裁两类记忆"
        );
    }
    let mirror = &game.state.belief_participants[&AccountId(1)];
    assert_eq!(mirror.watchlist(), &after.watchlist);
    assert_eq!(mirror.price_memory(), &after.price_memory);
    assert_eq!(mirror.belief(), &after.belief);
    let save = session.save().unwrap();
    assert_eq!(save.watchlists[&AccountId(1)], after.watchlist);
    assert_eq!(save.price_memories[&AccountId(1)], after.price_memory);
    assert_eq!(save.history_reads[&AccountId(1)], after.history_reads);
}

#[test]
fn runtime_closed_numeric_projection_failure_keeps_original_fault_and_retryability() {
    let mut game = closed_runtime_information_session();
    let before = game.business_state_hash().unwrap();
    let before_personal = actual_group_personal(&game);
    let result: Result<(), SessionError> = game.with_prepared_civil_day(|_, _| {
        Err(SessionError::InvalidSave(ValidationFault::new(
            1034,
            vec![71, 9],
        )))
    });
    assert!(
        matches!(result, Err(SessionError::InvalidSave(fault)) if fault == ValidationFault::new(1034, vec![71, 9]))
    );
    assert!(game.poison_reason().is_none());
    assert_eq!(game.business_state_hash().unwrap(), before);
    assert_eq!(actual_group_personal(&game), before_personal);
    game.end_civil_day().expect("可逆数值投影拒绝后应允许重试");
}

#[test]
fn runtime_after_market_close_numeric_failure_keeps_original_fault_and_terminal_state() {
    let mut game = runtime_information_session("2030-01-04");
    game.step_via_runtime().unwrap();
    game.step_via_runtime().unwrap();
    let before = game.business_state_hash().unwrap();
    let result: Result<(), SessionError> = game.with_prepared_civil_day(|_, _| {
        Err(SessionError::InvalidSave(ValidationFault::new(
            1034,
            vec![72, 10],
        )))
    });
    assert!(
        matches!(result, Err(SessionError::InvalidSave(fault)) if fault == ValidationFault::new(1034, vec![72, 10]))
    );
    assert_eq!(game.business_state_hash().unwrap(), before);
    let fatal = game
        .poison_reason()
        .expect("真实收盘安装后的失败必须终结")
        .clone();
    assert!(matches!(game.save(), Err(SessionError::Step(actual)) if actual == fatal));
    assert_eq!(game.step_via_runtime().unwrap_err(), fatal);
}

#[test]
fn runtime_first_civil_candidate_save_matches_committed_authority_after_lazy_handover() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.start_date = crate::CivilDate::from_iso("2030-01-05").unwrap();
    setup.ticks_per_day = 2;
    let mut game = GameSession::new(setup, 42).unwrap();
    assert!(game.runtime_driver.is_none());
    let candidate_save = game
        .with_prepared_civil_day(|candidate, _| candidate.save())
        .unwrap();
    assert!(game.runtime_driver.is_some());
    assert_eq!(
        serde_json::to_value(candidate_save).unwrap(),
        serde_json::to_value(game.save().unwrap()).unwrap(),
        "首次接管后的 callback 存档必须与固定尾提交后的完整权威版本一致"
    );
}

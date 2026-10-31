//! 直接调用真实 Session 接缝；空实现必须保留真实目标红，不以镜像补钱通过。

use super::*;
use crate::{AccountId, Intent, LimitPrice, Money, Side, StockId};

#[test]
fn civil_information_error_preserves_runtime_and_date_causes() {
    use std::error::Error;

    let date = crate::CivilDate::from_iso("2030-01-04").unwrap();
    let error = SessionError::CivilInformation(CivilInformationError::new(
        date,
        RuntimeError::ProtocolInvariant {
            detail: "private-runtime-detail".to_owned(),
        },
    ));
    let opaque = error.source().unwrap();
    assert!(opaque.is::<CivilInformationError>());
    assert!(matches!(
        opaque.source().unwrap().downcast_ref::<RuntimeError>(),
        Some(RuntimeError::ProtocolInvariant { detail }) if detail == "private-runtime-detail"
    ));
    assert!(opaque.source().unwrap().source().is_none());

    let next_date = date.next().unwrap();
    let error = SessionError::CivilInformation(CivilInformationError::from_mirror(
        date,
        AdministrativeMirrorError::CivilDateMismatch {
            expected: date,
            actual: next_date,
        },
    ));
    let source = error.source().unwrap().source().unwrap();
    assert!(matches!(
        source.downcast_ref::<AdministrativeMirrorError>(),
        Some(AdministrativeMirrorError::CivilDateMismatch { expected, actual })
            if *expected == date && *actual == next_date
    ));
}

fn game() -> GameSession {
    let setup = crate::runtime::administrative_mirror_setup_for_test();
    let mut game = GameSession::new_with_runtime_groups(setup, 7, 3).unwrap();
    game.construct_runtime_driver_lazy().unwrap();
    game.enqueue_player_intent(
        AccountId(0),
        Intent::PlaceLimit {
            code: StockId(0),
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(1000)),
            qty: 100,
        },
    )
    .unwrap();
    game
}

fn raw(game: &GameSession) -> Vec<(GroupIdx, Vec<crate::runtime::group::GroupAccount>)> {
    let mut driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
    assert_eq!(driver.account_by_owner((0, 0)), Some(AccountId(0)));
    driver.world_accounts_snapshot().unwrap()
}

fn credit(amount: i64) -> AdminOperation {
    AdminOperation::CashDividend {
        local: 0,
        stock: 0,
        amount,
    }
}

#[test]
fn administrative_mirror_01_same_prepare_updates_raw_and_session_without_routing_ingress() {
    let mut game = game();
    let before = game.save().unwrap();
    let before_raw = raw(&game);
    let before_cache = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .administrative_cache_snapshot_for_test();
    let revision = game.external_revision();
    let mut inspected = false;
    let id = game
        .apply_administrative_operations(
            vec![
                (
                    0,
                    vec![
                        credit(100),
                        AdminOperation::WithholdingTax {
                            local: 0,
                            stock: 0,
                            amount: 30,
                        },
                    ],
                ),
                (
                    2,
                    vec![
                        credit(31),
                        AdminOperation::WithholdingTax {
                            local: 0,
                            stock: 0,
                            amount: 11,
                        },
                    ],
                ),
            ],
            None,
            |candidate| {
                inspected = true;
                assert_eq!(candidate.groups.len(), 2);
                assert_eq!(candidate.groups[0].group, 0);
                assert_eq!(candidate.groups[0].accounts[0].0, 0);
                assert_eq!(
                    candidate.groups[0].accounts[0].1.cash,
                    before_raw[0].1[0].cash + 70
                );
                Ok(candidate.id)
            },
        )
        .expect("必须由同prepare实际候选安装raw与Session镜像");
    assert!(inspected);
    let after = game.save().unwrap();
    assert_eq!(game.tick(), before.snapshot.tick);
    assert_eq!(game.external_revision(), revision + 1);
    assert_eq!(
        serde_json::to_value(&after.pending_player).unwrap(),
        serde_json::to_value(&before.pending_player).unwrap()
    );
    assert_eq!(after.runtime_state, before.runtime_state);
    assert_eq!(after.civil_clock, before.civil_clock);
    assert_eq!(
        after.snapshot.accounts[&AccountId(0)].cash,
        before.snapshot.accounts[&AccountId(0)]
            .cash
            .add(Money::from_cents(70))
            .unwrap()
    );
    let after_raw = raw(&game);
    let mut expected_raw = before_raw;
    expected_raw[0].1[0].cash += 70;
    expected_raw[2].1[0].cash += 20;
    assert_eq!(after_raw, expected_raw);
    let _same_transaction_id = id;
    let driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
    assert_eq!(driver.account_by_owner((2, 0)), Some(AccountId(2)));
    assert_eq!(
        after.snapshot.accounts[&AccountId(2)].cash,
        before.snapshot.accounts[&AccountId(2)]
            .cash
            .add(Money::from_cents(20))
            .unwrap()
    );
    let mut expected_save = before.clone();
    for (account, delta) in [(AccountId(0), 70), (AccountId(2), 20)] {
        let saved = expected_save.snapshot.accounts.get_mut(&account).unwrap();
        saved.cash = saved.cash.add(Money::from_cents(delta)).unwrap();
    }
    assert_eq!(
        serde_json::to_value(&after).unwrap(),
        serde_json::to_value(&expected_save).unwrap(),
        "行政数值接缝仅改变同候选的净cash，不得改其它账户、持仓成本或个人/公开态"
    );
    let after_cache = driver.administrative_cache_snapshot_for_test();
    let mut expected_source = before_cache.source;
    expected_source.non_tick_revision += 1;
    assert_eq!(after_cache.source, expected_source);
    assert_eq!(after_cache.personals, before_cache.personals);
    assert_eq!(after_cache.pending_counts, before_cache.pending_counts);
    assert_eq!(after_cache.sync_state, before_cache.sync_state);
    assert!(
        driver.pending_mirror_domains().is_empty(),
        "同次完整镜像安装后才确认捕获域"
    );
}

#[test]
fn administrative_mirror_02_real_inspect_rejection_preserves_raw_cache_session_and_retry() {
    let mut game = game();
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    let before_raw = raw(&game);
    let before_cache = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .administrative_cache_snapshot_for_test();
    let revision = game.external_revision();
    let mut inspected = false;
    let error = game
        .apply_administrative_operations(vec![(0, vec![credit(100)])], None, |_candidate| {
            inspected = true;
            Err::<(), _>(RuntimeError::ProtocolInvariant {
                detail: "测试行政inspect拒绝".into(),
            })
        })
        .unwrap_err();
    assert!(
        inspected,
        "必须触达真实prepare后的inspect，不能用占位拒绝冒业务回滚"
    );
    assert!(error.to_string().contains("测试行政inspect拒绝"));
    assert_eq!(raw(&game), before_raw);
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
    assert_eq!(game.external_revision(), revision);
    assert_eq!(
        game.runtime_driver
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .administrative_cache_snapshot_for_test(),
        before_cache
    );
    assert!(game.require_healthy().is_ok());
    game.apply_administrative_operations(vec![(0, vec![credit(100)])], None, |_| Ok(()))
        .unwrap();
    assert_eq!(raw(&game)[0].1[0].cash, before_raw[0].1[0].cash + 100);
}

#[test]
fn administrative_mirror_03_late_group_prepare_failure_installs_no_earlier_credit() {
    let mut game = game();
    let before = serde_json::to_value(game.save().unwrap()).unwrap();
    let before_raw = raw(&game);
    let before_cache = game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .administrative_cache_snapshot_for_test();
    let revision = game.external_revision();
    let last_group = before_raw.last().unwrap().0;
    let available = before_raw.last().unwrap().1[0].cash;
    let error = game
        .apply_administrative_operations(
            vec![
                (0, vec![credit(100)]),
                (
                    last_group,
                    vec![AdminOperation::PaymentDebit {
                        local: 0,
                        stock: 0,
                        amount: available.checked_add(1).unwrap(),
                    }],
                ),
            ],
            None,
            |_| -> Result<(), RuntimeError> {
                panic!("尾组真实prepare拒绝不得到达inspect")
            },
        )
        .unwrap_err();
    let detail = match &error {
        AdministrativeMirrorError::Runtime(RuntimeError::ProtocolInvariant { detail }) => detail,
        other => panic!("必须保真实尾组现金短缺错误类别：{other:?}"),
    };
    assert!(detail.contains("PaymentDebit") && detail.contains("stock=0"));
    assert!(
        detail.contains(&format!(
            "InsufficientCash {{ requested: {}, available: {available} }}",
            available + 1
        )),
        "必须保真实尾组现金短缺原因：{detail}"
    );
    assert_eq!(raw(&game), before_raw);
    assert_eq!(serde_json::to_value(game.save().unwrap()).unwrap(), before);
    assert_eq!(game.external_revision(), revision);
    assert_eq!(
        game.runtime_driver
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .administrative_cache_snapshot_for_test(),
        before_cache
    );
    assert!(game.require_healthy().is_ok());
}

#[test]
fn administrative_mirror_04_information_observation_preserves_session_only_retail_stock() {
    let mut setup = crate::runtime::administrative_mirror_setup_for_test();
    setup.float_allocation.within_kind = WithinKindDistribution::EqualPercentage;
    let mut game = GameSession::new_with_runtime_groups(setup, 7, 3).unwrap();
    let owner = AccountId(1);
    let code = StockId(0);
    assert!(game.account(owner).unwrap().position(&code).unwrap().qty() > 0);
    // 真实组基线没有此键；Session/cache 保留同拍组外持仓经历的合法补充。
    game.state
        .retail_experience
        .get_mut(&owner)
        .unwrap()
        .stocks
        .remove(&code);
    game.construct_runtime_driver_lazy().unwrap();
    game.state
        .retail_experience
        .get_mut(&owner)
        .unwrap()
        .initialize_holding(
            &code,
            Some(Money::from_cents(1000)),
            Money::from_cents(1000),
            0,
        )
        .unwrap();
    let expected = game.state.retail_experience[&owner].clone();
    {
        let mut driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
        let mut cached = driver.administrative_cache_snapshot_for_test().personals[1]
            .clone()
            .unwrap();
        cached.retail_experience = Some(expected.clone());
        driver.install_cached_personal_at(1, owner, cached.into());
    }
    let through = crate::CivilInstant::from_hms(game.civil_date(), 23, 59, 0).unwrap();
    let (day, _) = game.prepare_market_calendar_projection().unwrap();
    let setup = game.export_setup_for_runtime();
    let layout = crate::session::exchange_calendar_model::SessionTickLayout {
        ticks_per_day: setup.ticks_per_day,
        auction_ticks: setup.auction_ticks,
        closing_auction_ticks: setup.closing_auction_ticks,
    };
    let input =
        crate::session::calendar_clock_input(&day, layout, game.tick(), u64::from(game.day()))
            .unwrap();
    let clock =
        crate::runtime::InformationClockContext::fixture_static(day, layout, input, through);
    let facts = Arc::new(game.state.company_read_facts().unwrap());
    let payload = InformationPreparePayload {
        price_unit_changes: Vec::new(),
        library: Arc::clone(&game.state.library),
        current_facts: Arc::clone(&facts),
        observation: Some(crate::runtime::world::InformationObservation {
            as_of_anchors: clock.anchors().unwrap(),
            clock,
            through,
            market: Arc::new(game.build_market_view()),
            company_facts: facts,
            memory_retention: crate::runtime::world::InformationMemoryRetention::Preserve,
        }),
    };
    let mut reached = false;
    game.apply_administrative_operations(Vec::new(), Some(payload), |candidate| {
        let raw = candidate
            .groups
            .iter()
            .flat_map(|group| &group.information.as_ref().unwrap().personals)
            .find(|(_, id, _)| *id == owner)
            .expect("真实INFO观察必须返回该retail本人候选");
        assert!(!raw
            .2
            .retail_experience
            .as_ref()
            .unwrap()
            .stocks
            .contains_key(&code));
        reached = true;
        Ok(())
    })
    .unwrap();
    assert!(reached);
    assert_eq!(
        game.state.retail_experience[&owner], expected,
        "INFO行政镜像不得丢Session-only retail键"
    );
    let driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
    assert_eq!(
        driver.administrative_cache_snapshot_for_test().personals[1]
            .as_ref()
            .unwrap()
            .retail_experience,
        Some(expected)
    );
}

#[test]
fn administrative_mirror_05_inspect_panic_returns_driver_and_preserves_original_payload() {
    let mut game = game();
    let cash = game.account(AccountId(0)).unwrap().cash();
    let pending = serde_json::to_value(&game.save().unwrap().pending_player).unwrap();
    let payload = Arc::new("行政Session原始panic".to_owned());
    let thrown = Arc::clone(&payload);
    let error = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = game.apply_administrative_operations(
            vec![(0, vec![credit(100)])],
            None,
            |_| -> Result<(), RuntimeError> {
                std::panic::panic_any(thrown);
            },
        );
    }))
    .unwrap_err();
    assert!(Arc::ptr_eq(
        error.downcast_ref::<Arc<String>>().unwrap(),
        &payload
    ));
    assert!(game.runtime_driver.is_some());
    assert_eq!(game.account(AccountId(0)).unwrap().cash(), cash);
    assert_eq!(
        serde_json::to_value(&game.state.pending_player).unwrap(),
        pending
    );
    let first = game.poison_reason().unwrap().clone();
    assert_eq!(game.require_healthy(), Err(first.clone()));
    assert!(matches!(game.save(), Err(crate::SessionError::Step(actual)) if actual == first));
    assert!(game
        .apply_administrative_operations(Vec::new(), None, |_| Ok(()))
        .is_err());
    assert_eq!(game.poison_reason(), Some(&first));
}

#[test]
fn administrative_mirror_06_baseline_disconnection_is_terminal_and_returns_driver() {
    let mut game = game();
    let cash = game.account(AccountId(0)).unwrap().cash();
    let pending = serde_json::to_value(&game.save().unwrap().pending_player).unwrap();
    game.runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .disconnect_company_approval_for_test();
    let mut inspected = false;
    let error = game
        .apply_administrative_operations(vec![(0, vec![credit(100)])], None, |_| {
            inspected = true;
            Ok(())
        })
        .unwrap_err();
    assert!(!inspected);
    assert!(error.to_string().contains("EchoAccounts"), "{error}");
    assert!(game.runtime_driver.is_some());
    assert!(game
        .runtime_driver
        .as_ref()
        .unwrap()
        .lock()
        .unwrap()
        .administrative_world_is_terminal());
    assert_eq!(game.account(AccountId(0)).unwrap().cash(), cash);
    assert_eq!(
        serde_json::to_value(&game.state.pending_player).unwrap(),
        pending
    );
    let first = game.poison_reason().unwrap().clone();
    assert!(matches!(game.save(), Err(crate::SessionError::Step(actual)) if actual == first));
    assert!(game
        .apply_administrative_operations(Vec::new(), None, |_| Ok(()))
        .is_err());
    assert_eq!(game.poison_reason(), Some(&first));
}

#[test]
fn administrative_mirror_07_same_tick_with_stale_world_civil_date_rejects_before_prepare() {
    let mut game = game();
    let current = game.civil_date();
    game.state.civil_clock.end_day(current).unwrap();
    assert_ne!(game.civil_date(), current);
    let before_raw = raw(&game);
    let revision = game.external_revision();
    let mut inspected = false;
    let error = game
        .apply_administrative_operations(vec![(0, vec![credit(100)])], None, |_| {
            inspected = true;
            Ok(())
        })
        .unwrap_err();
    assert!(!inspected, "异civil_date必须在Prepare前拒绝");
    assert!(error.to_string().contains("civil_date"), "{error}");
    assert!(matches!(
        error,
        AdministrativeMirrorError::CivilDateMismatch { expected, actual }
            if expected == game.civil_date() && actual == current
    ));
    assert_eq!(raw(&game), before_raw);
    assert_eq!(game.external_revision(), revision);
    assert!(game.require_healthy().is_ok());
}

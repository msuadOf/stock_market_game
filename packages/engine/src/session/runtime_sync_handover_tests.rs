use super::*;
use crate::runtime::drive::RuntimeDriver;

fn setup(policy: crate::RuntimeSyncPolicy) -> SessionSetup {
    let mut setup = super::npc_working_quote_tests::retail_quote_setup();
    setup.npcs.retail_count = 2;
    setup.start_date = crate::CivilDate::from_iso("2026-01-05").unwrap();
    setup.ticks_per_day = 8;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    setup.runtime_sync_policy = policy;
    setup
}

#[test]
fn direct_driver_entries_commit_one_progress_and_keep_each_policy_mirror_source() {
    for policy in [
        crate::RuntimeSyncPolicy::EveryTick,
        crate::RuntimeSyncPolicy::DayEnd,
        crate::RuntimeSyncPolicy::OnDemand,
    ] {
        let mut game = GameSession::new(setup(policy), 42).unwrap();
        let mut driver = RuntimeDriver::from_session_grouped(&mut game, 2).unwrap();
        driver.defer_due_for_test_at(&[AccountId(1), AccountId(2)], u64::MAX);
        let mut previous_seq = game.seq();
        for tick in 1..=2 {
            let events = if tick == 1 {
                driver.step(&mut game).unwrap()
            } else {
                driver.step_frame_equivalent(&mut game).unwrap()
            };
            assert_eq!(
                game.tick(),
                tick,
                "两个公开 driver 入口均提交一次真实 progress"
            );
            assert_eq!(driver.world_committed_tick(), tick);
            assert!(game.seq() >= previous_seq);
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, Event::PriceTick { .. }))
                    .count(),
                1
            );
            game.public_runtime_state().unwrap();
            let expected = if policy == crate::RuntimeSyncPolicy::EveryTick {
                tick
            } else {
                0
            };
            assert_eq!(
                game.runtime_mirror_origin().source_tick,
                expected,
                "延迟策略不走局部镜像安装"
            );
            if policy != crate::RuntimeSyncPolicy::EveryTick {
                assert_eq!(driver.account_writes_for_test(), 0);
                assert_eq!(driver.personal_installs_for_test(), 0);
            }
            previous_seq = game.seq();
        }
        let input = driver.prepare_flush_driver_projection(&game).unwrap();
        assert!(
            input.static_read.unwrap().refresh_market,
            "延迟策略首个真实 tick 后即使还未安装镜像，也必须使用真实行情"
        );
        let expected_markets: Vec<_> = driver
            .world_public_views()
            .iter()
            .map(|view| (view.last_price, view.public_cutoff_serial))
            .collect();
        let expected_histories: Vec<_> = driver
            .world_public_views()
            .iter()
            .map(|view| view.recent_prices.clone())
            .collect();
        driver.flush_now(&mut game).unwrap();
        assert_eq!(game.runtime_mirror_origin().source_tick, 2);
        assert_eq!(game.tick(), 2);
        assert_eq!(game.seq(), previous_seq, "静点 flush 不重复产生 PriceTick");
        let installed_markets: Vec<_> = (0..expected_markets.len())
            .map(|stock| {
                game.runtime_static_market_seed(StockId(stock as u32))
                    .unwrap()
            })
            .collect();
        assert_eq!(
            installed_markets, expected_markets,
            "首个延迟 flush 安装真实行情与公开截点"
        );
        assert_eq!(
            game.runtime_price_histories(),
            expected_histories,
            "行情历史不滞留初始种子"
        );
    }
    let mut game = GameSession::new(setup(crate::RuntimeSyncPolicy::EveryTick), 43).unwrap();
    let mut driver = RuntimeDriver::from_session_grouped(&mut game, 2).unwrap();
    driver.step_frame_equivalent(&mut game).unwrap();
    let origin = game.runtime_mirror_origin();
    let progress = (game.tick(), game.seq());
    game.inject_runtime_install_failure();
    let first = driver.step_frame_equivalent(&mut game).unwrap_err();
    assert_eq!(
        (game.tick(), game.seq()),
        progress,
        "candidate 失败零 progress 安装"
    );
    assert_eq!(game.runtime_mirror_origin(), origin);
    assert_eq!(driver.world_committed_tick(), 2, "真实已发布 World 不倒退");
    assert_eq!(driver.step_frame_equivalent(&mut game).unwrap_err(), first);
    assert_eq!(driver.step(&mut game).unwrap_err(), first.to_string());
    assert_eq!(driver.world_committed_tick(), 2, "terminal 后不再 Start");
    initial_market_seed_survives_static_install_and_closed_retained_republication();
}

/// 初始静点与闭市 retained 都不能让尚无真实 tick 的 v0 行情覆盖原始种子。
fn initial_market_seed_survives_static_install_and_closed_retained_republication() {
    let mut config = setup(crate::RuntimeSyncPolicy::OnDemand);
    config.start_date = crate::CivilDate::from_iso("2026-01-04").unwrap();
    let date = config.start_date;
    let mut game = GameSession::new(config, 45).unwrap();
    game.state
        .markets
        .get_mut(&StockId(0))
        .unwrap()
        .fixture_set_last_price(Money::from_cents(1095));
    let mut driver = RuntimeDriver::from_session_grouped(&mut game, 2).unwrap();
    driver.flush_now(&mut game).unwrap();
    assert_eq!(game.runtime_static_market_seed(StockId(0)).unwrap().0, 1095);
    let origin = game.runtime_mirror_origin();
    driver.flush_now(&mut game).unwrap();
    assert_eq!(
        game.runtime_mirror_origin(),
        origin,
        "初始静点重复 flush 幂等"
    );
    driver.freeze_retained_closed_day(&mut game, date).unwrap();
    let input = driver.prepare_flush_driver_projection(&game).unwrap();
    let read = input.static_read.unwrap();
    assert!(read.source.generation > 0, "真实闭市 retained 已经重发布");
    assert!(!read.refresh_market, "重发布仍没有真实市场 tick");
    assert_eq!(read.market_views[0].0, 1095);
    driver.flush_now(&mut game).unwrap();
    assert_eq!(game.runtime_static_market_seed(StockId(0)).unwrap().0, 1095);
    assert_eq!(game.tick(), 0, "静点操作不能构造市场拍");
}

fn accounts(
    driver: &mut RuntimeDriver,
) -> BTreeMap<AccountId, crate::runtime::group::GroupAccount> {
    let mut result = BTreeMap::new();
    for (group, rows) in driver.world_accounts_snapshot().unwrap() {
        for (local, value) in rows.into_iter().enumerate() {
            if let Some(owner) = driver.account_by_owner((group, local as u32)) {
                result.insert(owner, value);
            }
        }
    }
    result
}

fn personals(
    driver: &mut RuntimeDriver,
) -> BTreeMap<AccountId, Option<crate::runtime::world::BeliefParticipantSnapshot>> {
    driver
        .collect_personal_full()
        .unwrap()
        .groups
        .into_iter()
        .flat_map(|(_, rows)| rows.into_iter().map(|(_, owner, value)| (owner, value)))
        .collect()
}

fn attention(driver: &mut RuntimeDriver) -> BTreeMap<AccountId, (u64, u64)> {
    let mut result = BTreeMap::new();
    for (group, rows) in driver.collect_attention().unwrap() {
        for (local, due, seed) in rows {
            let mix = u64::from(group)
                .wrapping_mul(0x9E3779B97F4A7C15)
                .wrapping_add(u64::from(local));
            result.insert(
                driver.account_by_owner((group, local)).unwrap(),
                (due, seed ^ mix),
            );
        }
    }
    result
}

fn resting(driver: &mut RuntimeDriver) -> Vec<(usize, AccountId, crate::runtime::RestingFact)> {
    let mut result = Vec::new();
    for (stock, rows) in driver
        .collect_resting_facts_live()
        .unwrap()
        .into_iter()
        .enumerate()
    {
        for mut value in rows {
            let owner = driver.account_by_owner(value.owner).unwrap();
            // 比较 AccountId 权威与资源；旧 (group, local) 不能跨新名册解释。
            value.owner = (0, 0);
            result.push((stock, owner, value));
        }
    }
    result.sort_by_key(|(stock, owner, value)| (*stock, *owner, value.order_serial));
    result
}

#[test]
fn join_prepares_complete_current_authority_before_registering_new_player() {
    let mut game =
        GameSession::new_with_runtime_groups(setup(crate::RuntimeSyncPolicy::OnDemand), 42, 2)
            .unwrap();
    let stock = StockId(0);
    game.fixture_account_mut(&AccountId(0))
        .unwrap()
        .fixture_set_positions(BTreeMap::from([(
            stock,
            crate::account::Position::from_restored_parts(100, 100, 100_000, 10_000),
        )]));
    let mut seeded = Vec::new();
    game.seed_order_for_test(
        AccountId(1),
        Intent::PlaceLimit {
            code: stock,
            side: Side::Buy,
            price: LimitPrice::Fixed(Money::from_cents(900)),
            qty: 100,
        },
        &mut seeded,
    );
    let order = seeded
        .iter()
        .find_map(|event| match event {
            Event::OrderAccepted { id, .. } => Some(*id),
            _ => None,
        })
        .expect("真实旧簿种子须已受理");
    game.state.plans = crate::plans::PlanBook::from_parts(
        crate::plans::PlanPolicy::default(),
        vec![
            crate::plans::OwnerSequence {
                owner: AccountId(0),
                next_sequence: Some(40),
            },
            crate::plans::OwnerSequence {
                owner: AccountId(1),
                next_sequence: Some(90),
            },
            crate::plans::OwnerSequence {
                owner: AccountId(2),
                next_sequence: None,
            },
        ],
        vec![],
    )
    .unwrap();
    let plan = game
        .state
        .plans
        .create(crate::plans::PlanOpen {
            account: AccountId(1),
            code: stock,
            direction: Side::Buy,
            target: crate::plans::PlanTarget::ShareCount(100),
            opinion: crate::plans::PlanOpinion {
                signal_score_bp: 3000,
                source: crate::plans::OpinionSource::Blended,
            },
            confidence_bp: 8000,
            urgency: crate::plans::Urgency::Normal,
            horizon_trading_days: 5,
            created_trading_day: 0,
        })
        .unwrap();
    game.state
        .plans
        .install_execution(plan, 100, Money::from_cents(900), 240)
        .unwrap();
    game.state
        .plans
        .record_child_submitted(AccountId(1), &stock, Side::Buy, order, 100, 0)
        .unwrap();
    game.construct_runtime_driver_lazy_for_test().unwrap();
    let rng_before = {
        let mut driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
        driver.defer_due_for_test_at(&[AccountId(1)], u64::MAX);
        driver.force_due_for_test_at(&[AccountId(2)], 1);
        attention(&mut driver)[&AccountId(2)].1
    };
    game.step_frame().unwrap();
    let rng_after =
        attention(&mut game.runtime_driver.as_ref().unwrap().lock().unwrap())[&AccountId(2)].1;
    assert_ne!(
        rng_after, rng_before,
        "真实 due 决策已经推进 profile RNG，不能只比较构造种子"
    );
    let progress = (game.tick(), game.seq());
    let old_origin = game.runtime_mirror_origin();
    assert_eq!(old_origin.source_tick, 0, "真实旧镜像尚未安装首拍");
    let ingress = game.shared_ingress();
    ingress
        .enqueue_player_intent(
            AccountId(0),
            Intent::Cancel {
                code: stock,
                id: OrderId(999),
            },
        )
        .unwrap();
    let receipts = ingress.receipts_probe();
    for exhaust_epoch in [true, false] {
        let saved_epoch = game.runtime_authority_epoch;
        let saved_sequence = game.runtime_mirror_install_sequence;
        if exhaust_epoch {
            game.runtime_authority_epoch = u64::MAX;
        } else {
            game.runtime_mirror_install_sequence = u64::MAX;
        }
        assert!(game
            .join_market(OpaqueSubjectId::new("exhausted".into()).unwrap(), true)
            .is_err());
        game.runtime_authority_epoch = saved_epoch;
        game.runtime_mirror_install_sequence = saved_sequence;
        assert!(
            game.account(AccountId(3)).is_none(),
            "耗尽必须早于新账户与 receiver 登记"
        );
        assert!(matches!(
            ingress.enqueue_player_intent(
                AccountId(3),
                Intent::Cancel {
                    code: stock,
                    id: OrderId(1000)
                }
            ),
            Err(SessionError::UnknownPlayer(AccountId(3)))
        ));
        assert_eq!(
            ingress.receipts_probe().next_account_ordinal,
            receipts.next_account_ordinal
        );
    }
    let (old_accounts, old_personals, old_attention, old_resting, old_serials, old_plans) = {
        let mut driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
        let accounts = accounts(&mut driver);
        assert!(
            accounts[&AccountId(1)].buy_purse_held_cents > 0,
            "必须携带真实在途 purse"
        );
        let personals = personals(&mut driver);
        let attention = attention(&mut driver);
        let resting = resting(&mut driver);
        assert!(!resting.is_empty());
        let serials = driver.desk_next_serials().to_vec();
        let plans = driver.collect_plans().unwrap();
        (accounts, personals, attention, resting, serials, plans)
    };
    let member = game
        .join_market(OpaqueSubjectId::new("new-tail".into()).unwrap(), true)
        .unwrap();
    assert_eq!(member.account_id, AccountId(3));
    assert_eq!((game.tick(), game.seq()), progress);
    assert!(game.runtime_mirror_origin().authority_epoch > old_origin.authority_epoch);
    assert_eq!(game.runtime_mirror_origin().source_tick, 1);
    assert_eq!(
        ingress.receipts_probe().next_account_ordinal,
        receipts.next_account_ordinal,
        "移交不重新分配已受理 receipt"
    );
    assert_eq!(
        ingress.receipts_probe().next_stock_ordinal,
        receipts.next_stock_ordinal
    );
    let mut driver = game
        .runtime_driver
        .as_ref()
        .expect("正常 join 在注册前已准备并安装新 driver")
        .lock()
        .unwrap();
    assert_eq!(driver.accounts_per_group(), 2, "实际两组拓扑必须保留");
    let mut next_accounts = accounts(&mut driver);
    assert_eq!(
        next_accounts.remove(&member.account_id).unwrap().cash,
        member.admission_funding.external_cash.cents()
    );
    assert_eq!(next_accounts, old_accounts);
    let mut next_personals = personals(&mut driver);
    assert_eq!(
        next_personals.remove(&member.account_id),
        Some(None),
        "新 Player 没有虚造本人能力"
    );
    assert_eq!(next_personals, old_personals);
    let mut next_attention = attention(&mut driver);
    next_attention.remove(&member.account_id);
    assert_eq!(next_attention, old_attention);
    assert_eq!(resting(&mut driver), old_resting);
    assert_eq!(driver.desk_next_serials(), old_serials);
    let next_plans = driver.collect_plans().unwrap();
    for (_, old) in old_plans {
        for id in old.plan_ids() {
            let next = next_plans
                .iter()
                .find_map(|(_, book)| book.entry(id).ok())
                .unwrap();
            assert_eq!(
                next,
                old.entry(id).unwrap(),
                "child、terminal 与 PlanId 完整保留"
            );
        }
        for cursor in old.owner_sequences() {
            assert!(next_plans
                .iter()
                .any(|(_, book)| book.owner_sequences().any(|next| next == cursor)));
        }
    }
    drop(driver);
    let (before_accounts, before_personals, before_attention, before_resting) = {
        let mut driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
        (
            accounts(&mut driver),
            personals(&mut driver),
            attention(&mut driver),
            resting(&mut driver),
        )
    };
    let before_second = game.runtime_mirror_origin();
    let second = game
        .join_market(
            OpaqueSubjectId::new("repartition-tail".into()).unwrap(),
            true,
        )
        .unwrap();
    assert_eq!(second.account_id, AccountId(4));
    assert_eq!(
        game.runtime_mirror_origin().authority_epoch,
        before_second.authority_epoch + 1
    );
    let mut driver = game.runtime_driver.as_ref().unwrap().lock().unwrap();
    assert_eq!(
        driver.accounts_per_group(),
        3,
        "第二次加入必须真实改变旧 owner 的 group/local"
    );
    let mut after_accounts = accounts(&mut driver);
    after_accounts.remove(&second.account_id);
    let mut after_personals = personals(&mut driver);
    after_personals.remove(&second.account_id);
    let mut after_attention = attention(&mut driver);
    after_attention.remove(&second.account_id);
    assert_eq!(after_accounts, before_accounts);
    assert_eq!(after_personals, before_personals);
    assert_eq!(
        after_attention, before_attention,
        "重分组精确保留内部已推进 RNG，不能 double xor"
    );
    assert_eq!(resting(&mut driver), before_resting);
    drop(driver);
    ingress
        .enqueue_player_intent(
            member.account_id,
            Intent::Cancel {
                code: stock,
                id: OrderId(1000),
            },
        )
        .unwrap();
    game.step_frame().unwrap();
    assert_eq!(game.tick(), 2, "新旧 receiver 的已受理输入在同一后继拍消费");
    assert_eq!(
        ingress.receipts_probe().next_account_ordinal[&AccountId(0)],
        receipts.next_account_ordinal[&AccountId(0)]
    );
    let mut rejected =
        GameSession::new_with_runtime_groups(setup(crate::RuntimeSyncPolicy::OnDemand), 44, 2)
            .unwrap();
    rejected.step_frame().unwrap();
    let receiver = rejected.shared_ingress();
    receiver.close().unwrap();
    let old_origin = rejected.runtime_mirror_origin();
    assert!(matches!(
        rejected.join_market(
            OpaqueSubjectId::new("closed-receiver".into()).unwrap(),
            true
        ),
        Err(MembershipError::IngressRegistration(_))
    ));
    assert_eq!(
        rejected.runtime_mirror_origin(),
        old_origin,
        "注册拒绝并完成 prepared 回收后保原 authority"
    );
    assert!(rejected.account(AccountId(3)).is_none());
    rejected.require_healthy().unwrap();
    assert_eq!(
        rejected
            .runtime_driver
            .as_ref()
            .unwrap()
            .lock()
            .unwrap()
            .world_committed_tick(),
        1
    );
    let mut spawn_rejected =
        GameSession::new_with_runtime_groups(setup(crate::RuntimeSyncPolicy::OnDemand), 46, 2)
            .unwrap();
    spawn_rejected.step_frame().unwrap();
    let receiver = spawn_rejected.shared_ingress();
    let origin = spawn_rejected.runtime_mirror_origin();
    let before_receipts = receiver.receipts_probe();
    crate::runtime::world::fail_next_spawn_after_for_test(1);
    let error = spawn_rejected
        .join_market(OpaqueSubjectId::new("spawn-failure".into()).unwrap(), true)
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("注入 spawn rt-group-1 失败"),
        "必须失败于真实第一组已spawn后的第二组：{error}"
    );
    assert!(
        error.contains("已 join 1 个 worker"),
        "部分构造的真实 handle 必须全部回收：{error}"
    );
    assert_eq!(spawn_rejected.runtime_mirror_origin(), origin);
    assert!(spawn_rejected.account(AccountId(3)).is_none());
    assert!(matches!(
        receiver.enqueue_player_intent(
            AccountId(3),
            Intent::Cancel {
                code: stock,
                id: OrderId(111)
            }
        ),
        Err(SessionError::UnknownPlayer(AccountId(3)))
    ));
    assert_eq!(
        receiver.receipts_probe().next_account_ordinal,
        before_receipts.next_account_ordinal
    );
    spawn_rejected.require_healthy().unwrap();
    spawn_rejected.step_frame().unwrap();
    assert_eq!(
        spawn_rejected.tick(),
        2,
        "清理完成后原 authority 可真实继续"
    );
    let mut cleanup_rejected =
        GameSession::new_with_runtime_groups(setup(crate::RuntimeSyncPolicy::OnDemand), 47, 2)
            .unwrap();
    cleanup_rejected.step_frame().unwrap();
    let receiver = cleanup_rejected.shared_ingress();
    let origin = cleanup_rejected.runtime_mirror_origin();
    let receipts = receiver.receipts_probe();
    crate::runtime::world::fail_next_spawn_with_cleanup_error_for_test(1);
    let error = cleanup_rejected
        .join_market(
            OpaqueSubjectId::new("spawn-cleanup-failure".into()).unwrap(),
            true,
        )
        .unwrap_err()
        .to_string();
    assert!(
        error.contains("注入 spawn rt-group-1 失败")
            && error.contains("Continue 错序")
            && error.contains("已 join 1 个 worker"),
        "主错误与全部 join 错误不能扁平丢失：{error}"
    );
    assert_eq!(cleanup_rejected.runtime_mirror_origin(), origin);
    assert!(cleanup_rejected.account(AccountId(3)).is_none());
    assert_eq!(
        receiver.receipts_probe().next_account_ordinal,
        receipts.next_account_ordinal
    );
    let fatal = cleanup_rejected.require_healthy().unwrap_err();
    assert_eq!(cleanup_rejected.step_frame().unwrap_err(), fatal);
    assert!(
        matches!(receiver.enqueue_player_intent(AccountId(0), Intent::Cancel { code: stock, id: OrderId(111) }), Err(SessionError::Step(first)) if first == fatal)
    );
    for prepared_failure in [true, false] {
        let mut game =
            GameSession::new_with_runtime_groups(setup(crate::RuntimeSyncPolicy::OnDemand), 45, 2)
                .unwrap();
        game.step_frame().unwrap();
        let receiver = game.shared_ingress();
        if prepared_failure {
            receiver.close().unwrap();
        }
        let origin = game.runtime_mirror_origin();
        let old_publication = game.runtime_publication().unwrap();
        let old_tick = old_publication.load();
        super::runtime_handover::fail_next_stop_for_test(prepared_failure);
        let error = game
            .join_market(OpaqueSubjectId::new("stop-failure".into()).unwrap(), true)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Continue 错序"),
            "须保原 worker 错误：{error}"
        );
        assert!(
            error.contains("joined_workers: 3"),
            "全部真实 worker 必须回收：{error}"
        );
        let fatal = game.require_healthy().unwrap_err();
        assert_eq!(game.step_frame().unwrap_err(), fatal);
        assert!(
            std::sync::Arc::ptr_eq(&old_tick, &old_publication.load()),
            "旧 authority 停止不重发历史或 Final"
        );
        if prepared_failure {
            assert_eq!(game.runtime_mirror_origin(), origin);
            assert!(
                game.account(AccountId(3)).is_none(),
                "注册失败不能安装新成员"
            );
            assert!(error.contains("receiver 注册失败"));
        } else {
            assert_eq!(
                game.runtime_mirror_origin().authority_epoch,
                origin.authority_epoch + 1
            );
            assert!(
                game.account(AccountId(3)).is_some(),
                "注册后退役失败保新状态，不能回滚成员"
            );
            assert!(
                matches!(receiver.enqueue_player_intent(AccountId(3), Intent::Cancel { code: stock, id: OrderId(111) }), Err(SessionError::Step(first)) if first == fatal)
            );
            let next = game.runtime_publication().unwrap().load();
            assert_eq!(
                (next.tick, next.generation),
                (1, 0),
                "新初值未执行额外市场拍或重发"
            );
        }
    }
}

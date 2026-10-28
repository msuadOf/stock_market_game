use super::*;
#[path = "../../information_fixture.rs"]
mod fixture;
use crate::runtime::group::TickPlan;
use crate::runtime::protocol::{AdminFault, InformationPreparePayload};
use crate::runtime::skeleton_tests::basic_spec;
use crate::{AccountId, StockId};
use fixture::*;

fn world() -> World {
    world_with_cadence(crate::session::notices::NpcInformationCadence::Daily { checks: 1 })
}

fn world_with_cadence(cadence: crate::session::notices::NpcInformationCadence) -> World {
    let ids = [11, 37, 51, 79, 113].map(AccountId);
    let mut spec = basic_spec(3, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 100;
    spec.root_projection = Some(projection(ids.to_vec()));
    spec.belief_seeds = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let mut p = personal(id);
            p.attention.information_cadence = cadence;
            p.watchlist
                .stocks
                .insert(StockId(if i == 4 { 1 } else { 0 }), Default::default());
            Some(p)
        })
        .collect();
    spec.watchdog = WatchdogConfig {
        normal: Duration::from_millis(300),
        long: Duration::from_millis(300),
    };
    World::spawn(spec).unwrap()
}
fn snapshot(
    w: &mut World,
    p: InformationPreparePayload,
) -> Vec<crate::runtime::protocol::AdminGroupCandidate> {
    w.with_information_transaction(Vec::new(), p, |c| Ok(c.groups.clone()))
        .unwrap()
}
#[test]
fn info_world_01_library_and_facts_only_include_all_three_live_groups_with_tail() {
    let mut w = world();
    let mut p = payload(false);
    announcement(&mut p, 0);
    for shares in [10_000, 20_000] {
        p.current_facts = facts(shares);
        let groups = snapshot(&mut w, p.clone());
        assert_eq!(
            groups.iter().map(|g| g.group).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        for g in groups {
            assert!(g.accounts.is_empty());
            let info = g.information.unwrap();
            assert!(info.personals.is_empty());
            assert!(Arc::ptr_eq(&info.library, &p.library));
            assert!(Arc::ptr_eq(&info.current_facts, &p.current_facts));
        }
    }
    p.observation = payload(true).observation;
    p.current_facts = p.observation.as_ref().unwrap().company_facts.clone();
    let groups = snapshot(&mut w, p.clone());
    let owners: Vec<_> = groups
        .into_iter()
        .flat_map(|g| {
            g.information
                .unwrap()
                .personals
                .into_iter()
                .map(move |(local, account, _)| (g.group, local, account))
        })
        .collect();
    assert_eq!(
        owners,
        [
            (0, 0, AccountId(11)),
            (0, 1, AccountId(37)),
            (1, 0, AccountId(51)),
            (1, 1, AccountId(79)),
            (2, 0, AccountId(113))
        ]
    );
    w.shutdown().unwrap();
}
#[test]
fn info_world_02_last_group_rejection_aborts_prepared_private_rows_and_assets() {
    let mut w = world();
    let mut p = payload(true);
    let id = announcement(&mut p, 0);
    // 全局上下文合法，仅后组 owner 错位；先组必须已完成本 id prepare。
    w.group_txs[2]
        .send(GroupIn::InstallPersonal {
            local: 0,
            snapshot: personal(AccountId(999)),
        })
        .unwrap();
    let error = w
        .with_information_transaction(
            vec![(
                0,
                vec![AdminOperation::CashDividend {
                    local: 0,
                    stock: 0,
                    amount: 101,
                }],
            )],
            p.clone(),
            |_| Ok(()),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        RuntimeError::InformationPrepare {
            group: 2,
            error: crate::runtime::protocol::InformationPrepareError::Identity { .. }
        }
    ));
    assert!(w.dead_reason.is_none());
    let mut restored = personal(AccountId(113));
    restored
        .watchlist
        .stocks
        .insert(StockId(1), Default::default());
    w.group_txs[2]
        .send(GroupIn::InstallPersonal {
            local: 0,
            snapshot: restored,
        })
        .unwrap();
    let groups = snapshot(&mut w, p.clone());
    assert_eq!(groups.len(), 3);
    assert_eq!(
        groups
            .iter()
            .map(|g| g.information.as_ref().unwrap().personals.len())
            .collect::<Vec<_>>(),
        [2, 2, 1],
        "后组拒绝不能推进先组 cadence 或丢个人候选"
    );
    for g in groups {
        for (_, owner, personal) in g.information.unwrap().personals {
            assert_eq!(personal.information.owner(), owner);
            if owner != AccountId(113) {
                assert_eq!(personal.information.observed_at_of(id), Some(at(19, 0)));
            }
        }
    }
    assert_eq!(w.global.committed_tick, 0);
    let report = w.shutdown().unwrap();
    assert_eq!(report.group_finals[0].accounts[0].cash, 1_000_000);
}
#[test]
fn info_world_03_panic_and_partial_commit_transport_follow_admin_fatal_join() {
    for panic_worker in [false, true] {
        let mut w = world();
        let original = Arc::new("INFO-P 原始 panic".to_string());
        if panic_worker {
            w.group_txs[1]
                .send(GroupIn::AdminFaultForTest {
                    phase: AdminPhase::Prepare,
                    fault: AdminFault::Panic(original.clone()),
                })
                .unwrap();
        } else {
            w.admin_send_fault_for_test = Some((AdminPhase::Commit, 1));
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            w.with_information_transaction(Vec::new(), payload(true), |_| Ok(()))
        }));
        if panic_worker {
            let payload = result.unwrap_err().downcast::<Arc<String>>().unwrap();
            assert!(Arc::ptr_eq(&payload, &original));
        } else {
            assert!(result.unwrap().is_err());
        }
        assert!(w.dead_reason.is_some());
        assert!(w.handles.is_empty());
    }
}

#[test]
fn info_world_04_inspect_rejection_and_panic_keep_prepared_private_rows_unpublished() {
    for panic_inspect in [false, true] {
        let mut w = world();
        let mut p = payload(true);
        announcement(&mut p, 0);
        let original = Arc::new("INFO inspect payload".to_string());
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            w.with_information_transaction(Vec::new(), p.clone(), |_| {
                if panic_inspect {
                    std::panic::panic_any(original.clone());
                }
                Err::<(), _>(invalid("INFO inspect业务拒绝"))
            })
        }));
        if panic_inspect {
            assert!(Arc::ptr_eq(
                &result.unwrap_err().downcast::<Arc<String>>().unwrap(),
                &original
            ));
            assert!(w.dead_reason.is_some());
            assert!(w.handles.is_empty());
        } else {
            assert!(result.unwrap().is_err());
            assert!(w.dead_reason.is_none());
            let groups = snapshot(&mut w, p);
            assert_eq!(
                groups
                    .iter()
                    .map(|g| g.information.as_ref().unwrap().personals.len())
                    .collect::<Vec<_>>(),
                [2, 2, 1]
            );
            w.shutdown().unwrap();
        }
    }
}
#[test]
fn info_world_05_duplicate_and_wrong_id_receipts_follow_original_fatal_contract() {
    for fault in [
        AdminFault::Duplicate,
        AdminFault::WrongId,
        AdminFault::WrongGroup,
        AdminFault::Missing,
    ] {
        let mut w = world();
        w.group_txs[1]
            .send(GroupIn::AdminFaultForTest {
                phase: AdminPhase::Prepare,
                fault,
            })
            .unwrap();
        assert!(w
            .with_information_transaction(Vec::new(), payload(true), |_| Ok(()))
            .is_err());
        assert!(w.dead_reason.is_some());
        assert!(w.handles.is_empty());
    }
}

#[test]
fn info_world_06_candidate_validator_rejects_bad_local_account_and_snapshot_owner() {
    let mut w = world();
    let p = payload(true);
    let groups = snapshot(&mut w, p.clone());
    for invalid in 0..5 {
        let mut c = groups[0].clone();
        let info = c.information.as_mut().unwrap();
        match invalid {
            0 => info.personals[0].0 = 999,
            1 => info.personals[0].1 = AccountId(999),
            2 => info.personals[0].2 = personal(AccountId(999)),
            3 => info.personals.swap(0, 1),
            4 => info.current_facts = facts(30_000),
            _ => unreachable!(),
        }
        assert!(w.validate_information_candidate(&c, Some(&p)).is_err());
    }
    w.shutdown().unwrap();
}

/// None 观察的材料变化也要刷新；真实 no-op 不造 revision，过期 ack 不清旧原因。
#[test]
fn info_material_only_revision_noop_and_exact_ack_follow_actual_write_set() {
    let mut w = world();
    let mut p = payload(false);
    announcement(&mut p, 0);
    let source0 = w.mirror_source();
    let groups = snapshot(&mut w, p.clone());
    assert!(groups
        .iter()
        .all(|group| group.information.as_ref().unwrap().personals.is_empty()));
    let source1 = w.mirror_source();
    assert_eq!(
        (source1.tick, source1.generation),
        (source0.tick, source0.generation)
    );
    assert_eq!(source1.non_tick_revision, source0.non_tick_revision + 1);
    let dirty1 = w.pending_mirror_domains();
    assert!(
        dirty1.personal_full,
        "无personals但library/facts真改也需要Full"
    );
    snapshot(&mut w, p.clone());
    assert_eq!(
        w.mirror_source(),
        source1,
        "全材料相等的真实no-op不推进成功游标"
    );
    assert_eq!(w.pending_mirror_domains(), dirty1);
    w.with_admin_transaction(
        vec![(
            0,
            vec![AdminOperation::CashDividend {
                local: 0,
                stock: 0,
                amount: 1,
            }],
        )],
        |_| Ok(()),
    )
    .unwrap();
    let source2 = w.mirror_source();
    assert_eq!(source2.non_tick_revision, source1.non_tick_revision + 1);
    let dirty2 = w.pending_mirror_domains();
    assert!(
        dirty2.accounts_full && dirty2.personal_full,
        "连续同tick多域写集OR保留"
    );
    assert!(
        w.prepare_mirror_ack(source1, dirty1).is_err(),
        "旧来源不能ack较新写集"
    );
    assert_eq!(w.pending_mirror_domains(), dirty2, "失败ack不清任何旧原因");
    let ack = w.prepare_mirror_ack(source2, dirty2).unwrap();
    w.ack_prepared_mirror(ack);
    assert!(
        w.pending_mirror_domains().is_empty(),
        "精确成功ack才清捕获原因"
    );
    snapshot(&mut w, p);
    assert_eq!(w.mirror_source(), source2);
    assert!(w.pending_mirror_domains().is_empty());
    w.shutdown().unwrap();
}

/// I2：Immediate 的 next_after(now)=now，只有完整签名才能拒绝重复本人计算。
#[test]
fn same_signature_immediate_information_does_not_recompute_personal() {
    let mut w = world_with_cadence(crate::session::notices::NpcInformationCadence::Immediate);
    let mut p = payload(true);
    announcement(&mut p, 0);
    let first = snapshot(&mut w, p.clone());
    let first_count: usize = first
        .iter()
        .map(|group| group.information.as_ref().unwrap().personals.len())
        .sum();
    assert_eq!(
        first_count, 5,
        "首次三个真实组的五人都必须完成观察，不以空名册自证去重"
    );
    let source = w.mirror_source();
    let pending = w.pending_mirror_domains();
    let before = w.collect_group_belief().unwrap();
    let second = snapshot(&mut w, p);
    assert!(
        second
            .iter()
            .all(|group| group.information.as_ref().unwrap().personals.is_empty()),
        "Immediate 的同through/library/facts/market/day/owner/codes签名不得重复本人计算"
    );
    assert_eq!(
        w.collect_group_belief().unwrap(),
        before,
        "相同观察不能改七字段或丢获知"
    );
    assert_eq!(
        w.mirror_source(),
        source,
        "自己的revision/check回写不能变成新观察输入"
    );
    assert_eq!(w.pending_mirror_domains(), pending);
    w.shutdown().unwrap();
}

#[test]
fn library_only_install_does_not_consume_same_through_observation_eligibility() {
    let mut w = world_with_cadence(crate::session::notices::NpcInformationCadence::Immediate);
    let mut some = payload(true);
    announcement(&mut some, 0);
    let mut none = some.clone();
    none.observation = None;
    let materials = snapshot(&mut w, none);
    assert!(materials
        .iter()
        .all(|group| group.information.as_ref().unwrap().personals.is_empty()));
    let observed = snapshot(&mut w, some.clone());
    assert_eq!(
        observed
            .iter()
            .map(|group| group.information.as_ref().unwrap().personals.len())
            .sum::<usize>(),
        5,
        "None没有本人观察，不能占用随后Some同through的资格"
    );
    let source = w.mirror_source();
    let pending = w.pending_mirror_domains();
    let repeated = snapshot(&mut w, some);
    assert!(
        repeated
            .iter()
            .all(|group| group.information.as_ref().unwrap().personals.is_empty()),
        "第一次Some成功后，同签名必须去重"
    );
    assert_eq!(w.mirror_source(), source);
    assert_eq!(w.pending_mirror_domains(), pending);
    w.shutdown().unwrap();
}

/// 即使命中先前观察，仍先验证完整上下文；不能让版本/签名缓存绕过目录缺项。
#[test]
fn repeated_information_validates_full_context_before_cache_shortcut() {
    let mut w = world_with_cadence(crate::session::notices::NpcInformationCadence::Immediate);
    let mut p = payload(true);
    announcement(&mut p, 0);
    let first = snapshot(&mut w, p.clone());
    assert_eq!(
        first
            .iter()
            .map(|group| group.information.as_ref().unwrap().personals.len())
            .sum::<usize>(),
        5
    );
    let source = w.mirror_source();
    let pending = w.pending_mirror_domains();
    Arc::make_mut(&mut p.observation.as_mut().unwrap().market)
        .stocks
        .remove(&StockId(0));
    let rejected = w.with_information_transaction(Vec::new(), p, |_| Ok(()));
    assert!(
        matches!(
            &rejected,
            Err(RuntimeError::InformationPrepare {
                error: crate::runtime::protocol::InformationPrepareError::MissingMarket {
                    stock: StockId(0)
                },
                ..
            })
        ),
        "相同through/library仍要精确拒绝缺market字段：{rejected:?}"
    );
    assert_eq!(w.mirror_source(), source);
    assert_eq!(w.pending_mirror_domains(), pending);
    w.shutdown().unwrap();
}

#[test]
fn signature_uses_working_owner_codes_after_same_through_admin() {
    let mut w = world_with_cadence(crate::session::notices::NpcInformationCadence::Immediate);
    let mut p = payload(true);
    announcement(&mut p, 0);
    let first = snapshot(&mut w, p.clone());
    assert_eq!(
        first
            .iter()
            .map(|group| group.information.as_ref().unwrap().personals.len())
            .sum::<usize>(),
        5
    );
    // 真实 working 操作移走非 watchlist 的股1，group0/local0关注集合从{0,1}变{0}。
    let updated = w
        .with_information_transaction(
            vec![(
                0,
                vec![AdminOperation::ShareReduction {
                    local: 0,
                    stock: 1,
                    qty: 1000,
                }],
            )],
            p.clone(),
            |candidate| Ok(candidate.groups.clone()),
        )
        .unwrap();
    let rows = updated.iter().find(|group| group.group == 0).unwrap();
    assert_eq!(
        rows.accounts[0].1.shares[1], 0,
        "同一prepared事务的最终working持仓是签名输入"
    );
    assert_eq!(
        rows.information.as_ref().unwrap().personals.len(),
        2,
        "group0完整owner/codes签名变化，不能误用旧same-through缓存"
    );
    assert!(
        updated
            .iter()
            .filter(|group| group.group != 0)
            .all(|group| group.information.as_ref().unwrap().personals.is_empty()),
        "其余组实际观察输入未变，保同签名去重"
    );
    let repeated = snapshot(&mut w, p);
    assert!(repeated
        .iter()
        .all(|group| group.information.as_ref().unwrap().personals.is_empty()));
    w.shutdown().unwrap();
}

#[test]
fn rejected_information_prepare_does_not_consume_successful_signature() {
    let mut w = world_with_cadence(crate::session::notices::NpcInformationCadence::Immediate);
    let mut p = payload(true);
    announcement(&mut p, 0);
    let source = w.mirror_source();
    let pending = w.pending_mirror_domains();
    let rejected = w.with_information_transaction(Vec::new(), p.clone(), |_| {
        Err::<(), _>(invalid("纯inspect拒绝首个签名"))
    });
    assert!(rejected.is_err());
    assert_eq!(w.mirror_source(), source);
    assert_eq!(w.pending_mirror_domains(), pending);
    let accepted = snapshot(&mut w, p.clone());
    assert_eq!(
        accepted
            .iter()
            .map(|group| group.information.as_ref().unwrap().personals.len())
            .sum::<usize>(),
        5,
        "失败prepare不能占签名，下一真实成功观察仍执行五人"
    );
    let mut changed = p.clone();
    Arc::make_mut(&mut changed.observation.as_mut().unwrap().market)
        .stocks
        .get_mut(&StockId(0))
        .unwrap()
        .last_price = crate::Money::from_cents(1001);
    let source = w.mirror_source();
    let pending = w.pending_mirror_domains();
    let rejected = w.with_information_transaction(Vec::new(), changed, |_| {
        Err::<(), _>(invalid("纯inspect拒绝新行情签名"))
    });
    assert!(rejected.is_err());
    assert_eq!(w.mirror_source(), source);
    assert_eq!(w.pending_mirror_domains(), pending);
    let original = snapshot(&mut w, p);
    assert!(
        original
            .iter()
            .all(|group| group.information.as_ref().unwrap().personals.is_empty()),
        "Abort后最后成功签名保原值，不被失败的新行情污染"
    );
    w.shutdown().unwrap();
}

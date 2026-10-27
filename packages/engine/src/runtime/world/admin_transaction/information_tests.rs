use super::*;
#[path = "../../information_fixture.rs"]
mod fixture;
use crate::runtime::group::TickPlan;
use crate::runtime::protocol::{AdminFault, InformationPreparePayload};
use crate::runtime::skeleton_tests::basic_spec;
use crate::{AccountId, StockId};
use fixture::*;

fn world() -> World {
    let ids = [11, 37, 51, 79, 113].map(AccountId);
    let mut spec = basic_spec(3, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 100;
    spec.root_projection = Some(projection(ids.to_vec()));
    spec.belief_seeds = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let mut p = personal(id);
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

use super::*;
use crate::runtime::group::{GroupScript, TickPlan};
use crate::runtime::wait_contract::DeadlineConfig;
use crate::runtime::world::{DeskStockSpec, World, WorldSpec};
use std::sync::Arc;
use std::time::Duration;
fn basic_spec(
    n_groups: usize,
    n_stocks: usize,
    accounts_per_group: usize,
    script_for: Box<dyn Fn(GroupIdx) -> GroupScript>,
) -> WorldSpec {
    let shares = vec![vec![vec![1_000u32; n_stocks]; accounts_per_group]; n_groups];
    WorldSpec {
        initial_plan_books: Vec::new(),
        initial_repurchase_projections: Default::default(),
        admin_treasury_bindings: Vec::new(),
        initial_desk_orders: Vec::new(),
        initial_desk_next_serial: Vec::new(),
        n_groups,
        accounts_per_group,
        initial_cash: 1_000_000,
        initial_shares: shares,
        initial_invested: Vec::new(),
        initial_recovered: Vec::new(),
        initial_ever_held: Vec::new(),
        initial_frozen_today: Vec::new(),
        initial_purse_held: Vec::new(),
        initial_pending_sell: Vec::new(),
        root_projection: None,
        civil_start_date: crate::calendar::CivilDate::from_ymd(2026, 1, 5).expect("测试日期合法"),
        calendar_start_date: crate::calendar::CivilDate::from_ymd(2026, 1, 5)
            .expect("测试计数起日合法"),
        script_for,
        chunk: 8,
        deadline: DeadlineConfig {
            normal: Duration::from_secs(5),
            long: Duration::from_secs(10),
        },
        rounds_cap: 4,
        stocks: (0..n_stocks)
            .map(|_| DeskStockSpec::main_board(1000))
            .collect(),
        config: std::sync::Arc::new(crate::config::GameConfig::proposed_defaults()),
        ticks_per_day: 30,
        auction_ticks: 0,
        closing_auction_ticks: 3,
        order_ttl_ticks: 1_000_000,
        history_len: 16,
        initial_committed_tick: 0,
        initial_last_day_end_tick: None,
        belief_seeds: Vec::new(),
        library: None,
        decision: None,
        rights_computer: None,
        initial_cash_override: None,
        plan_factory: None,
    }
}

fn world() -> World {
    let mut spec = basic_spec(2, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.initial_cash = 100;
    spec.deadline = DeadlineConfig {
        normal: Duration::from_millis(500),
        long: Duration::from_millis(500),
    };
    World::spawn(
        spec,
        crate::calendar::TradingCalendar::current_default_calendar()
            .expect("显式 raw Calendar 政策合法"),
    )
    .unwrap()
}
fn ops() -> Vec<(GroupIdx, Vec<AdminOperation>)> {
    vec![
        (
            1,
            vec![
                AdminOperation::WithholdingTax {
                    local: 1,
                    stock: 1,
                    amount: 30,
                },
                AdminOperation::WithholdingTax {
                    local: 1,
                    stock: 0,
                    amount: 20,
                },
            ],
        ),
        (
            0,
            vec![AdminOperation::WithholdingTax {
                local: 0,
                stock: 0,
                amount: 10,
            }],
        ),
    ]
}
#[test]
fn real_tax_only_commit_preserves_id_operation_order_and_complete_accounts() {
    let mut w = world();
    let before = w.snapshot_accounts().unwrap();
    let grouped = ops();
    let prepared = w
        .with_admin_transaction(grouped.clone(), |candidate| {
            let value = inspect_tax_only(&before, &grouped, candidate).expect("真实税扣款应通过");
            assert_eq!(value.id, candidate.id);
            Ok(value)
        })
        .unwrap();
    // 发布只发生在 with_admin_transaction 成功返回之后。
    assert_eq!(
        prepared
            .operations
            .iter()
            .map(|p| (
                p.group_index,
                p.operation_index,
                p.group,
                p.local,
                p.stock,
                p.amount
            ))
            .collect::<Vec<_>>(),
        vec![
            (0, 0, 1, 1, 1, 30),
            (0, 1, 1, 1, 0, 20),
            (1, 0, 0, 0, 0, 10)
        ]
    );
    let after = w.snapshot_accounts().unwrap();
    let mut expected = before;
    expected[1].1[1].cash = 50;
    expected[0].1[0].cash = 90;
    assert_eq!(after, expected);
    w.shutdown().unwrap();
}

#[test]
fn complete_eq_rejects_every_non_cash_account_field_and_wrong_cash() {
    let mut w = world();
    let before = w.snapshot_accounts().unwrap();
    let grouped = ops();
    w.with_admin_transaction(grouped.clone(), |candidate| {
        let original = format!("{candidate:?}");
        for field in 0..9 {
            let mut changed = candidate.clone();
            let account = Arc::make_mut(
                &mut changed
                    .groups
                    .iter_mut()
                    .find(|g| g.group == 1)
                    .unwrap()
                    .accounts[0]
                    .1,
            );
            match field {
                0 => account.cash += 1,
                1 => account.buy_purse_held_cents += 1,
                2 => account.shares[0] += 1,
                3 => account.frozen_today[0] += 1,
                4 => account.foreign_active[0] += 1,
                5 => account.invested_cents[0] += 1,
                6 => account.recovered_cents[0] += 1,
                7 => account.ever_held[0] = !account.ever_held[0],
                8 => account.pending_sell_qty[0] += 1,
                _ => unreachable!(),
            }
            let error = inspect_tax_only(&before, &grouped, &changed)
                .expect_err("任何非现金变更或错税额都必须拒绝");
            assert_eq!((error.group, error.local), (Some(1), Some(1)));
        }
        assert_eq!(
            format!("{candidate:?}"),
            original,
            "只读 inspect 不修改真实候选"
        );
        Ok(())
    })
    .unwrap();
    w.shutdown().unwrap();
}

#[test]
fn candidate_coordinates_must_exactly_match_original_tax_operations() {
    let mut w = world();
    let before = w.snapshot_accounts().unwrap();
    let grouped = ops();
    w.with_admin_transaction(grouped.clone(), |candidate| {
        for mutation in 0..7 {
            let mut changed = candidate.clone();
            match mutation {
                0 => {
                    changed.groups.pop();
                }
                1 => changed.groups.push(changed.groups[0].clone()),
                2 => changed.groups[0].group = 9,
                3 => changed.groups[0].accounts.clear(),
                4 => {
                    let duplicate = changed.groups[0].accounts[0].clone();
                    changed.groups[0].accounts.push(duplicate);
                }
                5 => changed.groups[0].accounts[0].0 = 9,
                6 => changed.groups[0]
                    .accounts
                    .push((1, Arc::new(before[0].1[1].clone()))),
                _ => unreachable!(),
            }
            assert!(
                inspect_tax_only(&before, &grouped, &changed).is_err(),
                "坐标篡改 {mutation}"
            );
        }
        for mutation in 0..5 {
            let mut changed = grouped.clone();
            match mutation {
                0 => changed.clear(),
                1 => {
                    changed[1].0 = changed[0].0;
                }
                2 => changed[0].1.clear(),
                3 => changed[0].0 = 9,
                4 => {
                    changed.pop();
                }
                _ => unreachable!(),
            }
            assert!(
                inspect_tax_only(&before, &changed, candidate).is_err(),
                "原操作集篡改 {mutation}"
            );
        }
        let mut repeated_before = before.clone();
        repeated_before.push(before[0].clone());
        assert!(inspect_tax_only(&repeated_before, &grouped, candidate).is_err());
        assert!(inspect_tax_only(&before[0..1], &grouped, candidate).is_err());
        Ok(())
    })
    .unwrap();
    w.shutdown().unwrap();
}

#[test]
fn typed_positive_amounts_stock_bounds_and_shared_raw_cash_are_checked_in_order() {
    let mut w = world();
    let before = w.snapshot_accounts().unwrap();
    let grouped = ops();
    w.with_admin_transaction(grouped.clone(), |candidate| {
        for amount in [0, -1, i64::MIN] {
            let mut changed = grouped.clone();
            changed[0].1[0] = AdminOperation::WithholdingTax {
                local: 1,
                stock: 1,
                amount,
            };
            let error = inspect_tax_only(&before, &changed, candidate).unwrap_err();
            assert_eq!(
                (error.group, error.local, error.operation_index),
                (Some(1), Some(1), Some(0))
            );
        }
        for op in [
            AdminOperation::CashDividend {
                local: 1,
                stock: 1,
                amount: 30,
            },
            AdminOperation::ShareSplit {
                local: 1,
                stock: 1,
                bonus_qty: 30,
            },
            AdminOperation::ShareReduction {
                local: 1,
                stock: 1,
                qty: 30,
            },
            AdminOperation::WithholdingTax {
                local: 1,
                stock: 2,
                amount: 30,
            },
            AdminOperation::WithholdingTax {
                local: 2,
                stock: 1,
                amount: 30,
            },
        ] {
            let mut changed = grouped.clone();
            changed[0].1[0] = op;
            assert!(inspect_tax_only(&before, &changed, candidate).is_err());
        }
        let mut changed = grouped.clone();
        changed[0].1 = vec![
            AdminOperation::WithholdingTax {
                local: 1,
                stock: 1,
                amount: 80,
            },
            AdminOperation::WithholdingTax {
                local: 1,
                stock: 0,
                amount: 30,
            },
        ];
        let error = inspect_tax_only(&before, &changed, candidate).unwrap_err();
        assert_eq!(
            (error.group, error.local, error.operation_index),
            (Some(1), Some(1), Some(1)),
            "同账户跨证券共享现金，不能每股各算100"
        );
        assert!(error.detail.contains("raw cash"));
        let mut bad_before = before.clone();
        bad_before[1].1[1].cash = i64::MIN;
        assert!(inspect_tax_only(&bad_before, &grouped, candidate).is_err());
        Ok(())
    })
    .unwrap();
    w.shutdown().unwrap();
}

#[test]
fn real_inspect_rejection_preserves_original_error_accounts_and_no_publication() {
    let mut w = world();
    let before = w.snapshot_accounts().unwrap();
    let grouped = ops();
    // 用真实 World 产生协议错误原件，避免复制私有 RuntimeError 类型。
    let rejection = w
        .with_admin_transaction(vec![(u16::MAX, vec![])], |_| Ok(()))
        .unwrap_err();
    let original_error = rejection.to_string();
    let mut detail = None;
    let result = w.with_admin_transaction(grouped.clone(), |candidate| {
        let mut changed = candidate.clone();
        Arc::make_mut(&mut changed.groups[0].accounts[0].1).cash += 1;
        detail = Some(inspect_tax_only(&before, &grouped, &changed).unwrap_err());
        Err::<PreparedTaxProjection, _>(rejection)
    });
    let mut published = Vec::new();
    match result {
        Ok(prepared) => published.push(prepared),
        Err(error) => assert_eq!(error.to_string(), original_error),
    }
    assert!(detail.is_some());
    assert!(published.is_empty());
    assert_eq!(w.snapshot_accounts().unwrap(), before);
    let prepared = w
        .with_admin_transaction(grouped.clone(), |candidate| {
            Ok(inspect_tax_only(&before, &grouped, candidate).unwrap())
        })
        .unwrap();
    published.push(prepared);
    assert_eq!(published.len(), 1, "仅唯一外层 commit 成功返回后发布");
    w.shutdown().unwrap();
}

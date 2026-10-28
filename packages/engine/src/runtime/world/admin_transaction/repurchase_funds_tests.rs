//! P2 真实多组行政事务；方案资金来源仍由同一次 inspect 勾稽。
use super::*;
#[path = "../../information_fixture.rs"]
mod fixture;
use crate::plans::{PlanId, RepurchaseExecutionOpen};
use crate::runtime::group::{GroupAccount, TickPlan};
use crate::runtime::protocol::AdminTreasuryBinding;
use crate::runtime::skeleton_tests::basic_spec;
use crate::{AccountId, StockId};
use crate::company::CompanyId;

fn spec() -> (WorldSpec, [PlanId; 2]) {
    let ids = [11, 37, 51, 79, 113].map(AccountId);
    let mut spec = basic_spec(3, 2, 2, Box::new(|_| Box::new(|_, _| TickPlan::default())));
    spec.ticks_per_day = 100;
    spec.root_projection = Some(fixture::projection(ids.to_vec()));
    spec.initial_cash_override = Some(vec![vec![700; 2]; 3]);
    spec.initial_shares = vec![vec![vec![0; 2]; 2]; 3];
    let mut book = crate::plans::PlanBook::default();
    let plans = [(AccountId(79), 0), (AccountId(113), 1)].map(|(owner, stock)| {
        book.create_issuer_repurchase(RepurchaseExecutionOpen {
            owner, issuer: CompanyId(stock), stock: StockId(stock),
        }).unwrap()
    });
    spec.initial_plan_books = crate::runtime::plan_ownership::split(book, &ids, 3, 2).unwrap();
    spec.admin_treasury_bindings = vec![
        AdminTreasuryBinding { owner: (1, 1), account: ids[3], issuer: CompanyId(0), stock: 0 },
        AdminTreasuryBinding { owner: (2, 0), account: ids[4], issuer: CompanyId(1), stock: 1 },
    ];
    spec.watchdog = WatchdogConfig { normal: Duration::from_millis(300), long: Duration::from_millis(300) };
    (spec, plans)
}
fn accounts(w: &mut World) -> Vec<Vec<GroupAccount>> {
    for tx in &w.group_txs { tx.send(GroupIn::EchoAccounts).unwrap(); }
    let mut out = vec![Vec::new(); w.n_groups];
    for _ in 0..w.n_groups { match recv_deadline(&w.up_rx, Phase::Seal, &w.watchdog).unwrap() {
        UpMsg::GroupEcho { group, accounts } => out[group as usize] = accounts,
        other => panic!("P2真实账户回声错位：{other:?}"),
    }} out
}
fn credit(local: u32, stock: u16, id: PlanId, amount: i64) -> AdminOperation {
    AdminOperation::SyntheticBudgetCredit { local, stock, execution_plan: id, amount }
}
#[test]
fn funds_p2_world_01_commit_two_real_groups_keeps_tick_and_next_start_reads_credit() {
    let (spec, ids) = spec(); let mut w = World::spawn(spec, crate::calendar::TradingCalendar::current_default_calendar().unwrap()).unwrap();
    let before = accounts(&mut w); let mut expected = before.clone();
    let source_before = w.mirror_source(); let published = w.publication.load();
    expected[1][1].cash += 101; expected[2][0].cash += 203;
    w.with_admin_transaction(vec![(1, vec![credit(1, 0, ids[0], 101)]), (2, vec![credit(0, 1, ids[1], 203)])], |c| {
        assert_eq!(c.groups.len(), 2);
        assert!(c.groups.iter().all(|group| group.mirror_domains.accounts_full));
        for g in &c.groups { for (local, actual) in &g.accounts {
            assert_eq!(actual.as_ref(), &expected[g.group as usize][*local as usize]);
        }} Ok(())
    }).unwrap();
    assert_eq!(w.global.committed_tick, 0); assert_eq!(accounts(&mut w), expected);
    let source_after = w.mirror_source();
    assert_eq!((source_after.tick, source_after.generation), (source_before.tick, source_before.generation));
    assert_eq!(source_after.non_tick_revision, source_before.non_tick_revision + 1);
    assert!(w.pending_mirror_domains().accounts_full);
    assert!(Arc::ptr_eq(&published, &w.publication.load()), "行政信用不能发布新的市场 tick");
    w.run_ticks(1).unwrap(); assert_eq!(accounts(&mut w), expected); w.shutdown().unwrap();
}
#[test]
fn funds_p2_world_02_late_group_reject_and_inspect_reject_abort_earlier_credit() {
    let (spec, ids) = spec(); let mut w = World::spawn(spec, crate::calendar::TradingCalendar::current_default_calendar().unwrap()).unwrap(); let before = accounts(&mut w);
    let source = w.mirror_source(); let pending = w.pending_mirror_domains();
    assert!(w.with_admin_transaction(vec![(1, vec![credit(1, 0, ids[0], 101)]),
        (2, vec![credit(0, 0, ids[1], 203)])], |_| -> Result<(), RuntimeError> { panic!("错stock不得进入inspect") }).is_err());
    assert_eq!(accounts(&mut w), before);
    let mut inspected = false;
    assert!(w.with_admin_transaction(vec![(1, vec![credit(1, 0, ids[0], 101)]),
        (2, vec![credit(0, 1, ids[1], 203)])], |_| {
            inspected = true; Err::<(), _>(invalid("公司预算与申请金额不一致"))
        }).is_err());
    assert!(inspected, "合法数值候选必须到达同一次公司预算inspect");
    assert_eq!(accounts(&mut w), before); assert!(w.dead_reason.is_none());
    assert_eq!(w.mirror_source(), source); assert_eq!(w.pending_mirror_domains(), pending);
    w.shutdown().unwrap();
}
#[test]
fn funds_p2_world_03_treasury_binding_preflight_rejects_duplicate_conflicting_and_tail_coordinates() {
    for mode in 0..7 {
        let (mut spec, _) = spec(); let b = spec.admin_treasury_bindings[0];
        match mode {
            0 => spec.admin_treasury_bindings.push(b),
            1 => spec.admin_treasury_bindings[0].account = AccountId(51),
            2 => spec.admin_treasury_bindings[0].owner = (3, 0),
            3 => spec.admin_treasury_bindings[1].owner = (2, 1),
            4 => spec.admin_treasury_bindings[0].stock = 2,
            5 => spec.admin_treasury_bindings.push(AdminTreasuryBinding { issuer: CompanyId(99), ..b }),
            _ => spec.root_projection = None,
        }
        match World::spawn(spec, crate::calendar::TradingCalendar::current_default_calendar().unwrap()) {
            Ok(w) => { w.shutdown().unwrap(); panic!("错误专户 binding 必须在构造时拒绝：mode={mode}"); },
            Err(e) => assert!(e.to_string().contains("binding"), "{e}"),
        }
    }
}

#[test]
fn funds_p2_world_04_exhausted_success_source_rejects_before_first_credit_commit() {
    let (spec, ids) = spec(); let mut w = World::spawn(spec, crate::calendar::TradingCalendar::current_default_calendar().unwrap()).unwrap();
    w.non_tick_revision = u64::MAX;
    let before = accounts(&mut w); let source = w.mirror_source(); let pending = w.pending_mirror_domains();
    let mut inspected = false;
    let error = w.with_admin_transaction(vec![(1, vec![credit(1, 0, ids[0], 101)])], |_| { inspected = true; Ok(()) }).unwrap_err();
    assert!(inspected, "合法信用必须先到达真实 candidate 后由成功 source 耗尽拒绝 commit");
    assert!(error.to_string().contains("游标已耗尽"), "{error}");
    assert_eq!(accounts(&mut w), before); assert_eq!(w.mirror_source(), source);
    assert_eq!(w.pending_mirror_domains(), pending); assert!(w.dead_reason.is_none());
    w.shutdown().unwrap();
}

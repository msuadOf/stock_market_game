//! 正常自然日日终保存后改变 native 组数，稳定身份与账户游标持续分配。

use super::*;
use crate::plans::{OwnerSequence, PlanBook, PlanEvent, PlanOpen, PlanOpinion, PlanPolicy, PlanTarget, TerminationReason, Urgency};

fn open(owner: AccountId, day: u64) -> PlanOpen {
    PlanOpen { account: owner, code: StockId(0), direction: Side::Buy,
        target: PlanTarget::ShareCount(100),
        opinion: PlanOpinion { signal_score_bp: 3000, source: crate::plans::OpinionSource::Blended },
        confidence_bp: 8000, urgency: Urgency::Normal, horizon_trading_days: 5, created_trading_day: day }
}

#[test]
fn plan_identity_day_end_restores_one_two_three_groups_and_continues_cursors() {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.npcs.inst_count = 0;
    setup.npcs.retail_count = 2;
    setup.start_date = crate::CivilDate::from_iso("2030-01-07").unwrap();
    setup.ticks_per_day = 4;
    setup.auction_ticks = 0;
    setup.closing_auction_ticks = 0;
    let mut session = GameSession::new_with_runtime_groups(setup, 93, 1).unwrap();
    let policy = PlanPolicy { review_signal_delta_bp: 777, ..PlanPolicy::default() };
    session.state.plans = PlanBook::from_parts(policy, vec![
        OwnerSequence { owner: AccountId(0), next_sequence: Some(40) },
        OwnerSequence { owner: AccountId(1), next_sequence: None },
        OwnerSequence { owner: AccountId(2), next_sequence: Some(90) },
    ], vec![]).unwrap();
    let mut original_ids = Vec::new();
    for groups in 1..=3 {
        session.runtime_group_count = groups;
        session.step().unwrap();
        let day = u64::from(session.day());
        {
            let mut driver = session.runtime_driver.as_mut().unwrap().lock().unwrap();
            for owner in [AccountId(0), AccountId(2)] {
                driver.mutate_plan_book_for_test(owner, Box::new(move |book| {
                    let id = book.create(open(owner, day)).unwrap();
                    book.apply(id, PlanEvent::Terminated { reason: TerminationReason::Cancelled, trading_day: day }).unwrap();
                }));
            }
        }
        for _ in 1..4 { session.step().unwrap(); }
        session.end_civil_day().expect("正常自然日日终完成");
        let save = session.save().expect("真实GameSession保存出口");
        assert_eq!(*save.plans.policy(), policy);
        assert!(save.plans.plan_ids().all(|id| save.plans.plan(id).unwrap().execution().is_none()), "公共日终档没有execution");
        let ids: Vec<_> = save.plans.plan_ids().collect();
        for id in &original_ids { assert!(ids.contains(id), "改变组数不改旧身份/terminal"); }
        assert_eq!(ids.len(), groups * 2);
        assert!(ids.contains(&crate::plans::PlanId { owner: AccountId(0), sequence: 39 + groups as u64 }));
        assert!(ids.contains(&crate::plans::PlanId { owner: AccountId(2), sequence: 89 + groups as u64 }));
        let cursors: Vec<_> = save.plans.owner_sequences().collect();
        assert!(cursors.contains(&OwnerSequence { owner: AccountId(0), next_sequence: Some(40 + groups as u64) }));
        assert!(cursors.contains(&OwnerSequence { owner: AccountId(1), next_sequence: None }));
        assert!(cursors.contains(&OwnerSequence { owner: AccountId(2), next_sequence: Some(90 + groups as u64) }));
        original_ids = ids;
        let restored = GameSession::restore(&save).expect("真实日终恢复");
        assert_eq!(restored.state.plans, save.plans);
        session = restored;
    }
}

#[test]
fn plan_identity_bad_child_startup_preserves_old_book_resources_and_cursors() {
    let mut session = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 19).unwrap();
    session.seed_order_for_test(AccountId(0), Intent::PlaceLimit {
        code: StockId(0), side: Side::Buy,
        price: LimitPrice::Fixed(Money::from_cents(1_000)), qty: 100,
    }, &mut Vec::new());
    let id = session.state.plans.create(open(AccountId(1), 0)).unwrap();
    session.state.plans.install_execution(id, 100, Money::from_cents(1_000), 240).unwrap();
    session.state.plans.record_child_submitted(AccountId(1), &StockId(0), Side::Buy, OrderId(999), 100, 0).unwrap();
    let before = serde_json::to_value(session.save().unwrap()).unwrap();
    let error = crate::runtime::drive::RuntimeDriver::from_session_grouped(&mut session, 2)
        .err().expect("不存在的原child必须显式拒绝");
    assert!(error.contains("计划子单缺真实 desk 种子"), "具体归属错误：{error}");
    let after = serde_json::to_value(session.save().unwrap()).unwrap();
    assert_eq!(after, before, "坏child预检失败，不得清旧簿或修改资源/计划/游标");
    assert!(session.runtime_driver.is_none());
}

#[test]
fn plan_identity_joined_player_plan_and_events_use_tail_group_writer() {
    let mut session = GameSession::new_with_runtime_groups(super::npc_working_quote_tests::quote_setup(0), 20, 3).unwrap();
    let member = session.join_market(OpaqueSubjectId::new("tail-player".to_owned()).unwrap(), false).unwrap();
    assert_eq!(member.account_id, AccountId(2));
    session.step().unwrap();
    let owner = member.account_id;
    {
        let mut driver = session.runtime_driver.as_mut().unwrap().lock().unwrap();
        driver.mutate_plan_book_for_test(owner, Box::new(move |book| { book.create(open(owner, 0)).unwrap(); }));
    }
    session.step().unwrap();
    let id = session.state.plans.active_investor_plan(owner, &StockId(0)).unwrap().plan_id();
    {
        let mut driver = session.runtime_driver.as_mut().unwrap().lock().unwrap();
        driver.apply_plan_events_for_test(vec![(id, PlanEvent::Terminated {
            reason: TerminationReason::Cancelled, trading_day: 0,
        })]);
    }
    session.step().unwrap();
    assert!(session.state.plans.plan(id).unwrap().is_terminal());
    let mut driver = session.runtime_driver.as_mut().unwrap().lock().unwrap();
    for (group, book) in driver.collect_plans().unwrap() {
        assert_eq!(book.plan(id).is_ok(), group == 2, "新Player仅由所属末组持有计划");
    }
}

#[test]
fn origin_diagnostics_counts_all_entries_and_personal_view_remains_explicit() {
    let mut session = GameSession::new(super::npc_working_quote_tests::quote_setup(0), 95).unwrap();
    let personal = session.state.plans.create(open(AccountId(1), 0)).unwrap();
    let company = session.state.plans.create_issuer_repurchase(crate::plans::RepurchaseExecutionOpen {
        owner: AccountId(17), issuer: crate::company::CompanyId(0), stock: StockId(0),
    }).unwrap();
    let diagnostic = session.decision_chain_diagnostics();
    assert_eq!(diagnostic.plan_count, 2);
    assert_eq!(diagnostic.plans_per_stock[&StockId(0)], 2);
    assert_eq!(session.investor_plans_debug().len(), 1);
    let entries = session.plan_entries_debug();
    assert_eq!(entries.iter().map(|entry| entry.plan_id()).collect::<Vec<_>>(), vec![personal, company]);
}

include!("../../test-support/simple_company.rs");

fn origin_treasury_setup() -> SessionSetup {
    let mut setup = super::npc_working_quote_tests::quote_setup(0);
    setup.company_system = Some(simple_company_fixture!(crate; ["600888"]));
    setup.dividend_tax_mode = Some(crate::company::CashDividendTaxMode::FlatWithholding);
    setup.flat_withholding_bp = Some(1_000);
    setup.par_value_per_share = Some(Money::from_cents(100));
    setup.issuer_repurchase_enabled = true;
    setup
}

#[test]
fn origin_session_restore_rejects_company_without_committed_funding_binding() {
    let mut session = GameSession::new(origin_treasury_setup(), 96).unwrap();
    let owner = session.state.accounts.iter().find_map(|(id, account)| {
        (account.kind() == AccountKind::IssuerRepurchase).then_some(*id)
    }).unwrap();
    session.state.plans.create_issuer_repurchase(crate::plans::RepurchaseExecutionOpen {
        owner, issuer: crate::company::CompanyId(0), stock: StockId(0),
    }).unwrap();
    let saved = session.save().unwrap();
    let error = GameSession::restore(&saved).err().expect("低层公司条目不等于已提交资金授权");
    assert!(error.to_string().contains("缺少已提交的资金与公司授权 binding"), "{error}");
}

#[test]
fn origin_session_restore_rejects_investor_plan_on_real_treasury_account() {
    let mut session = GameSession::new(origin_treasury_setup(), 97).unwrap();
    let owner = session.state.accounts.iter().find_map(|(id, account)| {
        (account.kind() == AccountKind::IssuerRepurchase).then_some(*id)
    }).unwrap();
    session.state.plans.create(open(owner, 0)).unwrap();
    let saved = session.save().unwrap();
    let error = GameSession::restore(&saved).err().expect("真实专户不能伪造个人判断");
    assert!(error.to_string().contains("不能持有 Investor 个人计划"), "{error}");
}

#[test]
fn origin_real_next_tick_keeps_unfunded_company_value_out_of_personal_execution() {
    // 仅验证共同 records 的真实移交；直接创建的低层值没有批准或注资权限。
    let mut session = GameSession::new(origin_treasury_setup(), 98).unwrap();
    let owner = session.state.accounts.iter().find_map(|(id, account)| {
        (account.kind() == AccountKind::IssuerRepurchase).then_some(*id)
    }).unwrap();
    let personal = session.state.plans.create(open(AccountId(1), 0)).unwrap();
    let company = session.state.plans.create_issuer_repurchase(crate::plans::RepurchaseExecutionOpen {
        owner, issuer: crate::company::CompanyId(0), stock: StockId(0),
    }).unwrap();
    let authorization = session.state.plans.entry(company).unwrap().clone();
    let mut driver = crate::runtime::drive::RuntimeDriver::from_session_grouped(&mut session, 2).unwrap();
    for _ in 0..2 {
        driver.step_frame_equivalent(&mut session).unwrap();
        let mirror = session.take_runtime_plans();
        assert_eq!(mirror.entry(company).unwrap(), &authorization);
        assert!(mirror.entry(company).unwrap().execution().is_none());
        assert_eq!(mirror.plan(personal).unwrap().filled_qty(), 0);
        assert_eq!(mirror.investor_plans().map(|plan| plan.account()).collect::<Vec<_>>(), vec![AccountId(1)]);
        assert_eq!(session.state.accounts.get(&owner).unwrap().cash(), Money::ZERO);
        session.set_runtime_plans(mirror);
    }
}
